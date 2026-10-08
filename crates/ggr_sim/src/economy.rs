//! The treasury, candidates at the door, and hiring. The treasury refuses a debit it cannot
//! cover rather than clamping; gold is never negative.

use ggr_content::{Attr, PrefabKind};

use crate::commands::Refusal;
use crate::sched::Handler;
use crate::types::*;
use crate::World;

impl World {
    pub(crate) fn credit(&mut self, amount: i64, reason: GoldReason) {
        if amount == 0 {
            return;
        }
        let g = &mut self.s.guild;
        g.gold += amount;
        g.last_change = amount;
        g.last_reason = reason;
        self.s.stats.gold_earned += amount;
        self.s.stats.max_gold = self.s.stats.max_gold.max(g.gold);
        self.emit(SimEvent::GoldChanged {
            delta: amount,
            reason,
        });
    }

    pub(crate) fn debit(&mut self, amount: i64, reason: GoldReason) -> Result<(), Refusal> {
        if amount == 0 {
            return Ok(());
        }
        let g = &mut self.s.guild;
        if g.gold < amount {
            return Err(Refusal::InsufficientGold);
        }
        g.gold -= amount;
        g.last_change = -amount;
        g.last_reason = reason;
        self.s.stats.gold_spent += amount;
        self.emit(SimEvent::GoldChanged {
            delta: -amount,
            reason,
        });
        Ok(())
    }

    /// True when a clerk is behind any desk.
    pub fn desk_manned(&self) -> bool {
        self.ready_of_kind(PrefabKind::Desk).any(|i| i.is_staffed())
    }

    /// The desk can sign papers when manned — or when the guild employs no clerk at all, in
    /// which case the guildmaster signs personally (the soft-lock floor: a guild whose last
    /// clerk quit can still hire a new one).
    pub fn desk_can_sign(&self) -> bool {
        self.desk_manned() || !self.employs_clerk()
    }

    fn employs_clerk(&self) -> bool {
        let clerk = self.content.role_for_station(PrefabKind::Desk);
        self.s.chars.iter().any(|c| {
            c.on_roster()
                && c.staff
                    .as_ref()
                    .is_some_and(|s| Some(s.role as usize) == clerk)
        })
    }

    fn roll_attributes(&mut self, rank: u8) -> [i32; Attr::COUNT] {
        let mut attrs = [0; Attr::COUNT];
        for (i, a) in self.content.attributes.clone().iter().enumerate() {
            attrs[i] = self.s.rng.chargen().next_int(a.min, a.max + 1);
        }
        // The rank floor shifts the window without changing the draw (V2's rule).
        let shift = self.content.ranks[rank as usize].attribute_floor
            - self.content.ranks[0].attribute_floor;
        for a in &mut attrs {
            *a += shift;
        }
        attrs
    }

    fn roll_aptitudes(&mut self) -> Vec<i32> {
        let (lo, hi) = (self.content.aptitude_min, self.content.aptitude_max);
        (0..self.content.branches.len())
            .map(|_| self.s.rng.chargen().next_int(lo, hi + 1))
            .collect()
    }

    fn roll_name(&mut self) -> (u16, u16) {
        let g = self
            .s
            .rng
            .names()
            .next_int(0, self.content.given_names.len() as i32) as u16;
        let f = self
            .s
            .rng
            .names()
            .next_int(0, self.content.family_names.len() as i32) as u16;
        (g, f)
    }

    pub(crate) fn new_adventurer_info(&self, aptitude: Vec<i32>) -> AdventurerInfo {
        // Base class: the branch they have the most aptitude for.
        let best = aptitude
            .iter()
            .enumerate()
            .max_by_key(|(i, a)| (**a, std::cmp::Reverse(*i)))
            .map_or(0, |(i, _)| i);
        AdventurerInfo {
            xp: 0,
            class: self.content.base_class_for_branch(best) as u8,
            aspiration: None,
            aptitude,
            skill_xp: vec![0; self.content.skills.len()],
            equipment: [None; 5],
            quests_at_rank: 0,
            quests_succeeded: 0,
            quests_failed: 0,
            gold_earned: 0,
            rested_until: 0,
        }
    }

    pub(crate) fn spawn_starting_roster(&mut self) {
        for rank in self.content.starting_adventurers.clone() {
            let (g, f) = self.roll_name();
            let attrs = self.roll_attributes(rank as u8);
            let apt = self.roll_aptitudes();
            let info = self.new_adventurer_info(apt);
            self.spawn_character(g, f, attrs, rank as u8, None, Some(info));
        }
        for st in self.content.starting_staff.clone() {
            let (g, f) = self.roll_name();
            let attrs = self.roll_attributes(0);
            let info = self.new_staff_info(st.role as u8, st.start_hour, st.length);
            self.spawn_character(g, f, attrs, 0, Some(info), None);
        }
    }

    /// Roles with a built station but nobody on the roster to work it.
    pub(crate) fn unstaffed_roles(&self) -> Vec<u8> {
        (0..self.content.roles.len())
            .filter(|r| {
                let station = self.content.roles[*r].station;
                let built = self.s.instances.iter().any(|i| {
                    i.status != InstanceStatus::Demolished
                        && self.content.prefabs[i.prefab as usize].kind == station
                });
                let employed = self.s.chars.iter().any(|c| {
                    c.on_roster() && c.staff.as_ref().is_some_and(|s| s.role as usize == *r)
                });
                built && !employed
            })
            .map(|r| r as u8)
            .collect()
    }

    pub(crate) fn new_staff_info(&self, role: u8, start_hour: u8, length: u8) -> StaffInfo {
        StaffInfo {
            role,
            shift: Shift { start_hour, length },
            happiness: self.content.rules.happiness_start,
            low_hours: 0,
            unpaid_since: -1,
            workstation: None,
            wages_paid: 0,
        }
    }

    pub(crate) fn on_candidate_arrival(&mut self) {
        let now = self.s.minute;
        // Staff applicants come more often, and for the right job, when the guild has built a
        // station nobody is employed to work (a canteen with no cook).
        let percent = if self.unstaffed_roles().is_empty() {
            self.content.rules.candidate_staff_percent
        } else {
            self.content.rules.candidate_staff_percent_when_needed
        };
        let staff = self.s.rng.economy().chance(percent);
        let cand = self.roll_candidate(staff, false);
        self.push_candidate(cand);
        // The next arrival: sooner for a better-known guild and a happy clerk.
        let r = &self.content.rules;
        let base = i64::from(self.s.rng.economy().next_int(
            r.candidate_arrival_min_minutes,
            r.candidate_arrival_max_minutes,
        ));
        let tier = self.s.guild.renown_tier as i64;
        let clerk_speed = self
            .ready_of_kind(PrefabKind::Desk)
            .filter(|i| i.is_staffed())
            .map(|i| self.staff_speed_at(i.id))
            .max()
            .unwrap_or(100);
        let interval = (base * 100 / (100 + 15 * tier) * 100 / clerk_speed).max(60);
        self.s
            .sched
            .schedule(now, now + interval, Handler::CandidateArrival, 0);
    }

    fn roll_candidate(&mut self, staff: bool, volunteer: bool) -> Candidate {
        let now = self.s.minute;
        let (given, family) = self.roll_name();
        let tier = self
            .s
            .guild
            .renown_tier
            .min(self.content.rules.candidate_rank_weights.len() - 1);
        let staff_role = if staff {
            let needed = self.unstaffed_roles();
            let pool: Vec<u8> = if needed.is_empty() {
                (0..self.content.roles.len() as u8).collect()
            } else {
                needed
            };
            Some(pool[self.s.rng.chargen().next_int(0, pool.len() as i32) as usize])
        } else {
            None
        };
        let rank = if staff {
            0
        } else {
            let w = self.content.rules.candidate_rank_weights[tier].clone();
            let total: i32 = w.iter().sum();
            let roll = self.s.rng.chargen().next_int(0, total);
            let mut acc = 0;
            let mut rank = 0u8;
            for (i, wt) in w.iter().enumerate() {
                acc += wt;
                if roll < acc {
                    rank = i as u8;
                    break;
                }
            }
            rank.min(self.content.demo_rank_cap as u8)
        };
        let attrs = self.roll_attributes(rank);
        let aptitude = self.roll_aptitudes();
        let r = &self.content.rules;
        let cost = if volunteer {
            0
        } else if staff {
            r.staff_hire_cost
        } else {
            let sum: i32 = attrs.iter().sum();
            let baseline = self.content.ranks[rank as usize].hire_baseline;
            r.hire_cost_base
                + r.hire_cost_per_rank * i64::from(rank)
                + r.hire_cost_per_point * i64::from((sum - baseline).max(0))
        };
        let id = self.s.next_candidate_id;
        self.s.next_candidate_id += 1;
        Candidate {
            id,
            given,
            family,
            attrs,
            rank,
            staff_role,
            aptitude,
            cost,
            arrived: now,
            expires: now + r.candidate_window_minutes,
            status: CandidateStatus::Waiting,
            volunteer,
        }
    }

    fn push_candidate(&mut self, cand: Candidate) {
        let now = self.s.minute;
        let (id, expires) = (cand.id, cand.expires);
        self.s.candidates.push(cand);
        self.s.stats.candidates_arrived += 1;
        self.s
            .sched
            .schedule(now, expires, Handler::CandidateExpiry, u64::from(id));
        self.emit(SimEvent::CandidateArrived { candidate: id });
    }

    pub(crate) fn on_candidate_expiry(&mut self, payload: u64) {
        let id = payload as u32;
        if let Some(c) = self.s.candidates.iter_mut().find(|c| c.id == id) {
            if c.status == CandidateStatus::Waiting {
                c.status = CandidateStatus::Left;
                self.emit(SimEvent::CandidateLeft { candidate: id });
            }
        }
        // Answered and departed candidates are dropped from the save once their window is over.
        self.s
            .candidates
            .retain(|c| c.status == CandidateStatus::Waiting);
    }

    pub(crate) fn hire(&mut self, candidate: u32) -> Result<CharId, Refusal> {
        let Some(pos) = self.s.candidates.iter().position(|c| c.id == candidate) else {
            return Err(Refusal::UnknownCandidate);
        };
        if self.s.candidates[pos].status != CandidateStatus::Waiting {
            return Err(Refusal::CandidateGone);
        }
        if !self.desk_can_sign() {
            return Err(Refusal::DeskUnmanned);
        }
        let cand = self.s.candidates[pos].clone();
        self.debit(cand.cost, GoldReason::Hire)?;
        self.s.candidates[pos].status = CandidateStatus::Hired;
        let id = if let Some(role) = cand.staff_role {
            let info = self.new_staff_info(role, 8, 8);
            let id = self.spawn_character(cand.given, cand.family, cand.attrs, 0, Some(info), None);
            if self.content.roles[role as usize].station == PrefabKind::Canteen {
                self.complete_objective(Objective::HireCook);
            }
            id
        } else {
            let info = self.new_adventurer_info(cand.aptitude.clone());
            let id = self.spawn_character(
                cand.given,
                cand.family,
                cand.attrs,
                cand.rank,
                None,
                Some(info),
            );
            self.complete_objective(Objective::HireAdventurer);
            id
        };
        self.s.stats.hires += 1;
        self.emit(SimEvent::Hired { character: id });
        Ok(id)
    }

    pub(crate) fn decline(&mut self, candidate: u32) -> Result<(), Refusal> {
        let Some(c) = self.s.candidates.iter_mut().find(|c| c.id == candidate) else {
            return Err(Refusal::UnknownCandidate);
        };
        if c.status != CandidateStatus::Waiting {
            return Err(Refusal::CandidateGone);
        }
        c.status = CandidateStatus::Declined;
        Ok(())
    }

    /// The difficulty floor: a guild with no living adventurers and too little gold to hire is
    /// sent a volunteer who works for nothing.
    pub(crate) fn check_volunteer_floor(&mut self) {
        let living = self
            .s
            .chars
            .iter()
            .any(|c| c.is_adventurer() && c.on_roster());
        let waiting_volunteer = self
            .s
            .candidates
            .iter()
            .any(|c| c.volunteer && c.status == CandidateStatus::Waiting);
        if !living
            && !waiting_volunteer
            && self.s.guild.gold < self.content.rules.volunteer_gold_below
        {
            let cand = self.roll_candidate(false, true);
            self.push_candidate(cand);
        }
    }

    pub(crate) fn complete_objective(&mut self, o: Objective) {
        if !self.s.objectives.is_done(o) {
            self.s.objectives.done.push(o);
            self.emit(SimEvent::ObjectiveDone { objective: o });
        }
    }

    pub(crate) fn dismiss(&mut self, id: CharId) -> Result<(), Refusal> {
        let c = self
            .s
            .chars
            .get(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        if !c.on_roster() {
            return Err(Refusal::UnknownCharacter);
        }
        if c.state == CharState::OffMap(OffMapReason::Quest) || c.quest.is_some() {
            return Err(Refusal::AwayOnQuest);
        }
        // Their gear stays with the guild.
        if let Some(adv) = &mut self.s.chars[id as usize].adv {
            let items: Vec<u16> = adv.equipment.iter_mut().filter_map(Option::take).collect();
            for i in items {
                self.s.guild.stash[i as usize] += 1;
            }
        }
        self.s.promotions.retain(|p| p.character != id);
        self.leave_for_good(id);
        Ok(())
    }

    /// Walks a character out of the guild forever (dismissal or quitting).
    pub(crate) fn leave_for_good(&mut self, id: CharId) {
        let c = &self.s.chars[id as usize];
        if c.on_map() {
            self.release_any(id);
            self.walk_to_door(id, WalkPurpose::Leave);
        } else {
            let now = self.s.minute;
            let c = &mut self.s.chars[id as usize];
            c.serial = c.serial.wrapping_add(1);
            c.state = CharState::OffMap(OffMapReason::Left);
            c.state_entered = now;
        }
    }
}
