//! Staff with lives: shifts picked from a template and a start hour, happiness that drifts
//! toward a target every modifier of which is inspectable, work speed tied to happiness, wages
//! as a daily drain, and quitting when it all goes on too long.

use ggr_content::PrefabKind;

use crate::commands::Refusal;
use crate::sched::{pack, Handler};
use crate::types::*;
use crate::World;

/// One line of the happiness breakdown: a localisation key and its contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HappinessModifier {
    pub key: &'static str,
    pub value: i32,
}

impl World {
    /// The target happiness drifts toward, itemised.
    pub fn happiness_modifiers(&self, id: CharId) -> Vec<HappinessModifier> {
        let r = &self.content.rules;
        let Some(st) = &self.s.chars[id as usize].staff else {
            return Vec::new();
        };
        let mut out = vec![HappinessModifier {
            key: "happiness.base",
            value: r.happiness_base,
        }];
        let long = i32::from(st.shift.length) - r.happiness_long_shift_from_hours;
        if long > 0 {
            out.push(HappinessModifier {
                key: "happiness.long_shift",
                value: long * r.happiness_per_long_hour,
            });
        }
        let night = st.shift.night_hours();
        if night > 0 {
            out.push(HappinessModifier {
                key: "happiness.night",
                value: night * r.happiness_per_night_hour,
            });
        }
        if st.unpaid_since >= 0 && self.s.minute - st.unpaid_since < crate::MINUTES_PER_DAY {
            out.push(HappinessModifier {
                key: "happiness.unpaid",
                value: r.happiness_unpaid,
            });
        }
        let decor = (self.decor_count() * r.happiness_per_decor).min(r.happiness_decor_max);
        if decor > 0 {
            out.push(HappinessModifier {
                key: "happiness.decor",
                value: decor,
            });
        }
        if self
            .ready_of_kind(PrefabKind::Canteen)
            .any(|i| i.is_staffed())
        {
            out.push(HappinessModifier {
                key: "happiness.fed",
                value: r.happiness_well_fed_staff,
            });
        }
        out
    }

    pub fn happiness_target(&self, id: CharId) -> i32 {
        self.happiness_modifiers(id)
            .iter()
            .map(|m| m.value)
            .sum::<i32>()
            .clamp(0, 100)
    }

    /// Work speed in percent: 75 at happiness 0, 125 at 100.
    pub fn work_speed_percent(&self, id: CharId) -> i64 {
        let r = &self.content.rules;
        let h = self.s.chars[id as usize]
            .staff
            .as_ref()
            .map_or(50, |s| s.happiness);
        i64::from(r.work_speed_min_percent)
            + i64::from(r.work_speed_max_percent - r.work_speed_min_percent) * i64::from(h) / 100
    }

    pub(crate) fn hourly_staff(&mut self) {
        let hour = self.hour();
        let r = self.content.rules.clone();
        for i in 0..self.s.chars.len() {
            let id = i as CharId;
            if !self.s.chars[i].on_roster() || self.s.chars[i].staff.is_none() {
                continue;
            }
            // Happiness drifts one step toward its target.
            let target = self.happiness_target(id);
            let quit = {
                let st = self.s.chars[i].staff.as_mut().unwrap();
                let step = r.happiness_drift_per_hour;
                if st.happiness < target {
                    st.happiness = (st.happiness + step).min(target);
                } else if st.happiness > target {
                    st.happiness = (st.happiness - step).max(target);
                }
                if st.happiness < r.quit_below {
                    st.low_hours += 1;
                } else {
                    st.low_hours = 0;
                }
                st.low_hours >= r.quit_after_hours
            };
            if quit {
                self.s.stats.quits += 1;
                self.leave_for_good(id);
                self.emit(SimEvent::StaffQuit { character: id });
                continue;
            }
            let on_shift = self.s.chars[i].staff.as_ref().unwrap().shift.covers(hour);
            match self.s.chars[i].state {
                CharState::OffMap(OffMapReason::OffShift) if on_shift => {
                    self.s.chars[i].cell = self.grid.door;
                    self.enter_idle(id);
                    self.emit(SimEvent::ShiftStarted { character: id });
                }
                CharState::Interact if !on_shift => {
                    if let Activity::Working { .. } = self.s.chars[i].activity {
                        self.release_any(id);
                        self.walk_to_door(id, WalkPurpose::EndShift);
                    }
                }
                // Anyone else on shift but resting finishes their rest; the idle decision after
                // it sends them to a station.
                _ => {}
            }
        }
    }

    pub(crate) fn pay_wages(&mut self) {
        let now = self.s.minute;
        for i in 0..self.s.chars.len() {
            let c = &self.s.chars[i];
            let Some(st) = &c.staff else { continue };
            // Hired within the last half-day: their first wage is due tomorrow.
            if !c.on_roster() || now - c.hired_minute < crate::MINUTES_PER_DAY / 2 {
                continue;
            }
            let wage =
                self.content.roles[st.role as usize].wage_per_hour * i64::from(st.shift.length);
            if self.debit(wage, GoldReason::Wage).is_ok() {
                let st = self.s.chars[i].staff.as_mut().unwrap();
                st.wages_paid += wage;
                st.unpaid_since = -1;
            } else {
                self.s.chars[i].staff.as_mut().unwrap().unpaid_since = now;
            }
        }
    }

    pub(crate) fn set_shift(
        &mut self,
        id: CharId,
        start_hour: u8,
        length: u8,
    ) -> Result<(), Refusal> {
        if start_hour > 23 || !self.content.shift_lengths.contains(&length) {
            return Err(Refusal::BadShift);
        }
        let c = self
            .s
            .chars
            .get_mut(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        if !c.on_roster() {
            return Err(Refusal::UnknownCharacter);
        }
        let st = c.staff.as_mut().ok_or(Refusal::NotStaff)?;
        st.shift = Shift { start_hour, length };
        self.complete_objective(Objective::SetShift);
        self.reconcile_shift(id);
        Ok(())
    }

    /// Applies a shift change at once rather than at the next hour: someone now off shift heads
    /// home, someone now on shift comes in.
    fn reconcile_shift(&mut self, id: CharId) {
        let hour = self.hour();
        let c = &self.s.chars[id as usize];
        let on = c.staff.as_ref().unwrap().shift.covers(hour);
        match c.state {
            CharState::OffMap(OffMapReason::OffShift) if on => {
                self.s.chars[id as usize].cell = self.grid.door;
                self.enter_idle(id);
            }
            CharState::Interact if !on => {
                self.release_any(id);
                self.walk_to_door(id, WalkPurpose::EndShift);
            }
            CharState::Idle if !on => {
                let serial = c.serial;
                let now = self.s.minute;
                self.s
                    .sched
                    .schedule(now, now + 1, Handler::IdleDecision, pack(serial, id));
            }
            _ => {}
        }
    }

    pub(crate) fn assign_workstation(
        &mut self,
        id: CharId,
        inst: Option<InstId>,
    ) -> Result<(), Refusal> {
        let c = self
            .s
            .chars
            .get(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        let st = c.staff.as_ref().ok_or(Refusal::NotStaff)?;
        if let Some(i) = inst {
            let instance = self
                .s
                .instances
                .get(i as usize)
                .ok_or(Refusal::UnknownInstance)?;
            let kind = self.content.prefabs[instance.prefab as usize].kind;
            if kind != self.content.roles[st.role as usize].station
                || instance.status == InstanceStatus::Demolished
            {
                return Err(Refusal::WrongStation);
            }
        }
        self.s.chars[id as usize]
            .staff
            .as_mut()
            .unwrap()
            .workstation = inst;
        Ok(())
    }
}
