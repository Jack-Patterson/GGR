//! Characters in space: the idle loop, walks the sim times itself, the slot lifecycle
//! (reserve -> arrive -> leave), and what each kind of interactable does when used.
//!
//! The arrival minute is fixed when the walk is issued. The presenter only draws the walk; a
//! slow view, a dead view and a 16x skip are all the same code path.

use ggr_content::{Attr, PrefabKind};

use crate::sched::{pack, unpack, Handler};
use crate::types::*;
use crate::World;

impl World {
    /// Creates a character standing at the door, idle. Returns its id.
    pub(crate) fn spawn_character(
        &mut self,
        given: u16,
        family: u16,
        attrs: [i32; Attr::COUNT],
        rank: u8,
        staff: Option<StaffInfo>,
        adv: Option<AdventurerInfo>,
    ) -> CharId {
        let id = self.s.chars.len() as CharId;
        let now = self.s.minute;
        let hunger = self.content.rules.hunger_start;
        self.s.chars.push(Character {
            id,
            given,
            family,
            attrs,
            rank,
            state: CharState::OffMap(OffMapReason::OffShift),
            state_entered: now,
            serial: 0,
            cell: self.grid.door,
            walk: None,
            activity: Activity::None,
            hunger,
            injured_until: NO_INJURY,
            quest: None,
            hired_minute: now,
            staff,
            adv,
        });
        let on_shift = self.s.chars[id as usize]
            .staff
            .as_ref()
            .is_none_or(|st| st.shift.covers(self.hour()));
        if on_shift {
            self.enter_idle(id);
        }
        id
    }

    fn bump(&mut self, id: CharId, state: CharState) -> u32 {
        let now = self.s.minute;
        let c = &mut self.s.chars[id as usize];
        c.serial = c.serial.wrapping_add(1);
        c.state = state;
        c.state_entered = now;
        c.serial
    }

    /// Idle at the current cell; decides again after a short dwell.
    pub(crate) fn enter_idle(&mut self, id: CharId) {
        let serial = self.bump(id, CharState::Idle);
        let c = &mut self.s.chars[id as usize];
        c.walk = None;
        c.activity = Activity::None;
        let (lo, hi) = (
            self.content.rules.idle_dwell_min,
            self.content.rules.idle_dwell_max,
        );
        let dwell = i64::from(self.s.rng.behaviour().next_int(lo, hi));
        let now = self.s.minute;
        self.s
            .sched
            .schedule(now, now + dwell, Handler::IdleDecision, pack(serial, id));
    }

    fn retry_idle_later(&mut self, id: CharId) {
        let c = &self.s.chars[id as usize];
        let now = self.s.minute;
        let retry = self.content.rules.idle_retry_minutes;
        self.s
            .sched
            .schedule(now, now + retry, Handler::IdleDecision, pack(c.serial, id));
    }

    /// Resolves a serial-guarded payload to a live character, or counts it stale.
    fn guard(&mut self, payload: u64) -> Option<CharId> {
        let (serial, id) = unpack(payload);
        match self.s.chars.get(id as usize) {
            Some(c) if c.serial == serial => Some(id),
            _ => {
                self.s.stats.stale_events += 1;
                None
            }
        }
    }

    pub(crate) fn on_idle_decision(&mut self, payload: u64) {
        let Some(id) = self.guard(payload) else {
            return;
        };
        if self.s.chars[id as usize].state != CharState::Idle {
            return;
        }
        if self.s.chars[id as usize].is_staff() {
            self.decide_staff(id);
        } else {
            self.decide_adventurer(id);
        }
    }

    /// Travel minutes for `steps` cells at this character's agility (V2's formula, integer
    /// maths with truncation).
    pub fn travel_minutes(&self, agility: i32, steps: i32) -> i64 {
        let agi = i64::from(agility.clamp(3, 20));
        let pace = 85 + ((agi - 3) * 30) / 17;
        let tenths = self.content.rules.minutes_per_cell_tenths;
        ((i64::from(steps) * tenths * 100) / (10 * pace)).max(1)
    }

    /// Starts a timed walk. Returns false (and changes nothing) if `to` is unreachable.
    fn start_walk(&mut self, id: CharId, to: Cell, purpose: WalkPurpose) -> bool {
        let from = self.s.chars[id as usize].cell;
        let Some(steps) = self.grid.distance(from, to, &mut self.scratch) else {
            return false;
        };
        let agi = self.s.chars[id as usize].attrs[Attr::Agility.index()];
        let minutes = self.travel_minutes(agi, steps);
        let serial = self.bump(id, CharState::Travel);
        let now = self.s.minute;
        let c = &mut self.s.chars[id as usize];
        c.activity = Activity::None;
        c.walk = Some(Walk {
            from,
            to,
            depart: now,
            arrive: now + minutes,
            purpose,
        });
        self.s
            .sched
            .schedule(now, now + minutes, Handler::Arrival, pack(serial, id));
        true
    }

    /// Reserves a slot and walks to it. False if the slot cannot be reached.
    pub(crate) fn go_to_slot(&mut self, id: CharId, inst: InstId, slot: usize) -> bool {
        let cell = self.s.instances[inst as usize].slots[slot].cell;
        if self.s.chars[id as usize].cell == cell {
            // Already standing there: reserve and arrive at once, one minute later.
        }
        let ok = self.start_walk(
            id,
            cell,
            WalkPurpose::ToSlot {
                inst,
                slot: slot as u8,
            },
        );
        if ok {
            let s = &mut self.s.instances[inst as usize].slots[slot];
            debug_assert!(s.holder.is_none(), "reserving a held slot");
            s.holder = Some(id);
            s.occupied = false;
            self.s.audit.reserves += 1;
        }
        ok
    }

    pub(crate) fn walk_to_door(&mut self, id: CharId, purpose: WalkPurpose) {
        let door = self.grid.door;
        if self.s.chars[id as usize].cell == door || !self.start_walk(id, door, purpose) {
            // At the door already (or somehow walled in): leave from here.
            self.arrive_at_door(id, purpose);
        }
    }

    fn decide_adventurer(&mut self, id: CharId) {
        let now = self.s.minute;
        let rules = &self.content.rules;
        let c = &self.s.chars[id as usize];
        let injured = c.is_injured(now);
        let hungry = c.hunger < rules.hunger_hungry_below;
        let adv = c.adv.as_ref().expect("adventurer");
        let rested = now >= adv.rested_until;
        let can_quest = !injured && c.quest.is_none() && rested;

        if injured {
            if let Some((i, s)) = self.free_customer(PrefabKind::Infirmary) {
                if self.go_to_slot(id, i, s) {
                    return;
                }
            }
        }
        // Only to a canteen somebody is cooking at (or about to be): an empty kitchen is not
        // worth the walk, though someone already there when the cook leaves will queue.
        if hungry && self.cook_on_duty() {
            if let Some((i, s)) = self.free_customer(PrefabKind::Canteen) {
                if self.go_to_slot(id, i, s) {
                    return;
                }
            }
        }
        if can_quest && self.has_eligible_quest(id) {
            if let Some((i, s)) = self.free_customer(PrefabKind::Board) {
                if self.go_to_slot(id, i, s) {
                    return;
                }
            }
        }
        if !injured {
            if let Some(skills) = self.aspired_skills(id) {
                let pref = self.content.rules.training_preference_percent;
                if self.s.rng.behaviour().chance(pref) {
                    let found = self.s.instances.iter().find_map(|inst| {
                        let p = &self.content.prefabs[inst.prefab as usize];
                        (inst.is_ready()
                            && p.kind == PrefabKind::Training
                            && p.skills.iter().any(|s| skills.contains(s)))
                        .then(|| inst.free_customer_slot().map(|s| (inst.id, s)))
                        .flatten()
                    });
                    if let Some((i, s)) = found {
                        if self.go_to_slot(id, i, s) {
                            return;
                        }
                    }
                }
            }
        }
        // Otherwise wander: a uniform draw over every free place worth idling at.
        let mut options: Vec<(InstId, usize)> = Vec::new();
        for inst in &self.s.instances {
            if !inst.is_ready() {
                continue;
            }
            let kind = self.content.prefabs[inst.prefab as usize].kind;
            let fits = match kind {
                PrefabKind::Rest | PrefabKind::Board => true,
                PrefabKind::Training => !injured,
                _ => false,
            };
            if fits {
                if let Some(s) = inst.free_customer_slot() {
                    options.push((inst.id, s));
                }
            }
        }
        if options.is_empty() {
            self.retry_idle_later(id);
            return;
        }
        let pick = self.s.rng.behaviour().next_int(0, options.len() as i32) as usize;
        let (i, s) = options[pick];
        if !self.go_to_slot(id, i, s) {
            self.retry_idle_later(id);
        }
    }

    fn cook_on_duty(&self) -> bool {
        self.ready_of_kind(PrefabKind::Canteen)
            .any(|i| i.staff_slot().and_then(|s| s.holder).is_some())
    }

    fn free_customer(&self, kind: PrefabKind) -> Option<(InstId, usize)> {
        self.ready_of_kind(kind)
            .find_map(|i| i.free_customer_slot().map(|s| (i.id, s)))
    }

    fn decide_staff(&mut self, id: CharId) {
        let hour = self.hour();
        let st = self.s.chars[id as usize].staff.clone().expect("staff");
        if !st.shift.covers(hour) {
            self.walk_to_door(id, WalkPurpose::EndShift);
            return;
        }
        if let Some(inst) = self.station_for(id) {
            let slot = self.s.instances[inst as usize]
                .slots
                .iter()
                .position(|s| s.staff)
                .expect("station has a staff slot");
            if self.go_to_slot(id, inst, slot) {
                return;
            }
        }
        // No free station: loiter on a bench rather than stand rigid, and look again soon.
        if let Some((i, s)) = self.free_customer(PrefabKind::Rest) {
            if self.go_to_slot(id, i, s) {
                return;
            }
        }
        self.retry_idle_later(id);
    }

    /// The station a staff member should man: their pinned one if free, else the first free
    /// station of their role's kind.
    pub(crate) fn station_for(&self, id: CharId) -> Option<InstId> {
        let st = self.s.chars[id as usize].staff.as_ref()?;
        let kind = self.content.roles[st.role as usize].station;
        let free = |inst: &Instance| {
            inst.is_ready()
                && self.content.prefabs[inst.prefab as usize].kind == kind
                && inst
                    .staff_slot()
                    .is_some_and(|s| s.holder.is_none() || s.holder == Some(id))
        };
        if let Some(w) = st.workstation {
            if free(&self.s.instances[w as usize]) {
                return Some(w);
            }
        }
        self.s.instances.iter().find(|i| free(i)).map(|i| i.id)
    }

    pub(crate) fn on_arrival(&mut self, payload: u64) {
        let Some(id) = self.guard(payload) else {
            return;
        };
        let c = &mut self.s.chars[id as usize];
        if c.state != CharState::Travel {
            return;
        }
        let Some(walk) = c.walk.take() else {
            return;
        };
        c.cell = walk.to;
        match walk.purpose {
            WalkPurpose::ToSlot { inst, slot } => {
                let s = &mut self.s.instances[inst as usize].slots[slot as usize];
                debug_assert_eq!(s.holder, Some(id));
                s.occupied = true;
                self.s.audit.arrivals += 1;
                self.begin_interaction(id, inst, slot);
            }
            WalkPurpose::Stray => self.enter_idle(id),
            p @ (WalkPurpose::DepartQuest | WalkPurpose::EndShift | WalkPurpose::Leave) => {
                self.arrive_at_door(id, p)
            }
        }
    }

    fn arrive_at_door(&mut self, id: CharId, purpose: WalkPurpose) {
        self.s.chars[id as usize].cell = self.grid.door;
        match purpose {
            WalkPurpose::DepartQuest => self.depart_on_quest(id),
            WalkPurpose::EndShift => {
                self.bump(id, CharState::OffMap(OffMapReason::OffShift));
                self.s.chars[id as usize].walk = None;
                self.s.chars[id as usize].activity = Activity::None;
                self.emit(SimEvent::ShiftEnded { character: id });
            }
            _ => {
                self.bump(id, CharState::OffMap(OffMapReason::Left));
                self.s.chars[id as usize].walk = None;
                self.s.chars[id as usize].activity = Activity::None;
            }
        }
    }

    /// Work speed of whoever staffs `inst`, as a percentage (100 if unstaffed).
    pub(crate) fn staff_speed_at(&self, inst: InstId) -> i64 {
        self.s.instances[inst as usize]
            .staff_slot()
            .filter(|s| s.occupied)
            .and_then(|s| s.holder)
            .map_or(100, |h| self.work_speed_percent(h))
    }

    fn begin_interaction(&mut self, id: CharId, inst: InstId, slot: u8) {
        let serial = self.bump(id, CharState::Interact);
        let now = self.s.minute;
        let staff_slot = self.s.instances[inst as usize].slots[slot as usize].staff;
        if staff_slot {
            self.s.chars[id as usize].activity = Activity::Working { inst };
            return;
        }
        let p = &self.content.prefabs[self.s.instances[inst as usize].prefab as usize];
        let rules = &self.content.rules;
        let minutes = match p.kind {
            PrefabKind::Canteen => {
                if !self.s.instances[inst as usize].is_staffed() {
                    self.s.chars[id as usize].activity = Activity::Queueing {
                        inst,
                        slot,
                        since: now,
                    };
                    let poll = rules.service_queue_poll_minutes;
                    self.s
                        .sched
                        .schedule(now, now + poll, Handler::ServicePoll, pack(serial, id));
                    return;
                }
                (p.minutes * 100 / self.staff_speed_at(inst)).max(1)
            }
            PrefabKind::Infirmary => rules.infirmary_session_minutes,
            PrefabKind::Training => rules.training_session_minutes,
            _ => p.minutes.max(1),
        };
        self.s.chars[id as usize].activity = Activity::Using { inst, slot };
        self.s.sched.schedule(
            now,
            now + minutes,
            Handler::InteractionEnd,
            pack(serial, id),
        );
    }

    pub(crate) fn on_service_poll(&mut self, payload: u64) {
        let Some(id) = self.guard(payload) else {
            return;
        };
        let Activity::Queueing { inst, slot, since } = self.s.chars[id as usize].activity else {
            return;
        };
        let now = self.s.minute;
        let rules = &self.content.rules;
        if self.s.instances[inst as usize].is_staffed() {
            // Served at last: the meal starts now.
            let serial = self.bump(id, CharState::Interact);
            let p = &self.content.prefabs[self.s.instances[inst as usize].prefab as usize];
            let minutes = (p.minutes * 100 / self.staff_speed_at(inst)).max(1);
            self.s.chars[id as usize].activity = Activity::Using { inst, slot };
            self.s.sched.schedule(
                now,
                now + minutes,
                Handler::InteractionEnd,
                pack(serial, id),
            );
        } else if now - since >= rules.service_patience_minutes {
            self.release_slot(id, inst, slot);
            self.enter_idle(id);
        } else {
            let poll = rules.service_queue_poll_minutes;
            let serial = self.s.chars[id as usize].serial;
            self.s
                .sched
                .schedule(now, now + poll, Handler::ServicePoll, pack(serial, id));
        }
    }

    pub(crate) fn release_slot(&mut self, id: CharId, inst: InstId, slot: u8) {
        let s = &mut self.s.instances[inst as usize].slots[slot as usize];
        if s.holder == Some(id) {
            s.holder = None;
            s.occupied = false;
            self.s.audit.releases += 1;
        }
    }

    pub(crate) fn on_interaction_end(&mut self, payload: u64) {
        let Some(id) = self.guard(payload) else {
            return;
        };
        let Activity::Using { inst, slot } = self.s.chars[id as usize].activity else {
            return;
        };
        self.release_slot(id, inst, slot);
        let kind = self.kind_of(inst);
        let now = self.s.minute;
        match kind {
            PrefabKind::Canteen => {
                let (after, price) = (
                    self.content.rules.hunger_after_meal,
                    self.content.rules.meal_price,
                );
                self.s.chars[id as usize].hunger = after;
                self.s.stats.meals += 1;
                self.credit(price, GoldReason::Meal);
                self.emit(SimEvent::MealServed { character: id });
            }
            PrefabKind::Infirmary => {
                let rules = &self.content.rules;
                let mult = if self.s.instances[inst as usize].is_staffed() {
                    rules.infirmary_recovery_multiplier_staffed
                } else {
                    rules.infirmary_recovery_multiplier_unstaffed
                };
                let bonus =
                    rules.infirmary_session_minutes * (mult - 1) * self.staff_speed_at(inst) / 100;
                let c = &mut self.s.chars[id as usize];
                if c.injured_until > now {
                    c.injured_until = (c.injured_until - bonus).max(now);
                    if c.injured_until <= now {
                        c.injured_until = NO_INJURY;
                    }
                }
            }
            PrefabKind::Training => {
                let skills = self.content.prefabs[self.s.instances[inst as usize].prefab as usize]
                    .skills
                    .clone();
                let xp = self.content.rules.training_xp;
                for s in skills {
                    self.award_skill_xp(id, s, xp);
                }
                self.check_class_progress(id);
            }
            PrefabKind::Board if self.try_take_quest(id) => return,
            _ => {}
        }
        self.enter_idle(id);
    }

    /// Ends a character's claim on `inst` because it is being demolished.
    pub(crate) fn cancel_hold(&mut self, id: CharId, inst: InstId) {
        let c = &self.s.chars[id as usize];
        let slot = self.s.instances[inst as usize]
            .slots
            .iter()
            .position(|s| s.holder == Some(id));
        let Some(slot) = slot else { return };
        self.s.audit.cancellations += 1;
        match c.state {
            CharState::Travel => {
                self.release_slot(id, inst, slot as u8);
                if let Some(w) = &mut self.s.chars[id as usize].walk {
                    w.purpose = WalkPurpose::Stray;
                }
            }
            _ => {
                self.release_slot(id, inst, slot as u8);
                self.enter_idle(id);
            }
        }
    }

    /// Releases whatever slot a character holds (dismissal, shift end at a station).
    pub(crate) fn release_any(&mut self, id: CharId) {
        let held: Vec<(InstId, u8)> = self
            .s
            .instances
            .iter()
            .flat_map(|i| {
                i.slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.holder == Some(id))
                    .map(move |(n, _)| (i.id, n as u8))
            })
            .collect();
        for (i, s) in held {
            self.release_slot(id, i, s);
        }
    }

    pub(crate) fn hourly_needs(&mut self) {
        let decay = self.content.rules.hunger_decay_per_hour;
        for c in &mut self.s.chars {
            if c.is_adventurer() && c.on_map() {
                c.hunger = (c.hunger - decay).max(0);
            }
        }
    }

    /// Every slot's holder agrees with that character's state, and the audit balances.
    pub fn verify_consistency(&self) -> Result<(), String> {
        let mut held = 0u64;
        for inst in &self.s.instances {
            for (n, s) in inst.slots.iter().enumerate() {
                let Some(h) = s.holder else {
                    if s.occupied {
                        return Err(format!("slot {n} of {} occupied with no holder", inst.id));
                    }
                    continue;
                };
                held += 1;
                let c = &self.s.chars[h as usize];
                let ok = match c.state {
                    CharState::Travel => {
                        matches!(c.walk, Some(Walk { purpose: WalkPurpose::ToSlot { inst: i, slot }, .. }) if i == inst.id && slot as usize == n)
                            && !s.occupied
                    }
                    CharState::Interact => {
                        s.occupied
                            && match c.activity {
                                Activity::Using { inst: i, slot }
                                | Activity::Queueing { inst: i, slot, .. } => {
                                    i == inst.id && slot as usize == n
                                }
                                Activity::Working { inst: i } => i == inst.id && s.staff,
                                Activity::None => false,
                            }
                    }
                    _ => false,
                };
                if !ok {
                    return Err(format!(
                        "slot {n} of instance {} held by character {h} in state {:?}",
                        inst.id, c.state
                    ));
                }
            }
        }
        let a = &self.s.audit;
        if a.reserves != a.releases + held {
            return Err(format!(
                "slot audit: {} reserves != {} releases + {} held",
                a.reserves, a.releases, held
            ));
        }
        for c in &self.s.chars {
            if c.state == CharState::Interact {
                let holds = self
                    .s
                    .instances
                    .iter()
                    .any(|i| i.slots.iter().any(|s| s.holder == Some(c.id)));
                if !holds {
                    return Err(format!("character {} interacting with no slot", c.id));
                }
            }
        }
        Ok(())
    }
}
