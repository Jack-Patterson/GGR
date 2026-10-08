//! Quests: posting onto the board, adventurers choosing their own work, and the itinerary — a
//! flat list of legs, every die pre-rolled at departure and committed as scheduled events over
//! the quest's duration. A death stops the rest; nothing branches.

use ggr_content::LegKind;

use crate::sched::Handler;
use crate::types::*;
use crate::World;

impl World {
    /// The highest rank among living adventurers, or None with nobody to send.
    fn highest_living_rank(&self) -> Option<u8> {
        self.s
            .chars
            .iter()
            .filter(|c| c.is_adventurer() && c.on_roster())
            .map(|c| c.rank)
            .max()
    }

    pub(crate) fn generation_interval(&self) -> i64 {
        let r = &self.content.rules;
        (r.quest_generation_interval_minutes
            + r.quest_generation_interval_per_tier * self.s.guild.renown_tier as i64)
            .max(60)
    }

    pub(crate) fn on_quest_generation(&mut self) {
        let now = self.s.minute;
        let next = now + self.generation_interval();
        self.s
            .sched
            .schedule(now, next, Handler::QuestGeneration, 0);

        let posted = self.s.quests.iter().filter(|q| q.is_posted()).count();
        if posted >= self.content.rules.board_capacity {
            return;
        }
        let Some(max_rank) = self.highest_living_rank() else {
            return;
        };
        let procedural = self
            .s
            .rng
            .questgen()
            .chance(self.content.rules.procedural_quest_percent);
        let quest = if procedural {
            self.roll_procedural(max_rank)
        } else {
            self.roll_curated(max_rank)
        };
        if let Some(q) = quest {
            let id = q.id;
            self.s.quests.push(q);
            self.s.stats.posted += 1;
            let expires = self.s.quests[id as usize].expires;
            self.s
                .sched
                .schedule(now, expires, Handler::QuestExpiry, u64::from(id));
            self.emit(SimEvent::QuestPosted { quest: id });
        }
    }

    fn new_quest(&self, source: QuestSource, rank: u8) -> Quest {
        let now = self.s.minute;
        Quest {
            id: self.s.quests.len() as QuestId,
            source,
            rank,
            gold: (0, 0),
            xp: 0,
            injury_minutes: 0,
            legs: Vec::new(),
            loot: Vec::new(),
            posted: now,
            expires: now + self.content.rules.quest_expiry_minutes,
            status: QuestStatus::Posted,
            taker: None,
            departed: -1,
            due_back: -1,
            report: Vec::new(),
            gold_won: 0,
            loot_won: Vec::new(),
            resolved_at: -1,
        }
    }

    fn roll_curated(&mut self, max_rank: u8) -> Option<Quest> {
        let eligible: Vec<usize> = (0..self.content.quests.len())
            .filter(|i| self.content.quests[*i].rank <= max_rank as usize)
            .collect();
        if eligible.is_empty() {
            return None;
        }
        let pick = self.s.rng.questgen().next_int(0, eligible.len() as i32) as usize;
        let t = &self.content.quests[eligible[pick]];
        let mut q = self.new_quest(
            QuestSource::Curated {
                template: eligible[pick] as u16,
            },
            t.rank as u8,
        );
        q.gold = t.gold;
        q.xp = t.xp;
        q.injury_minutes = t.injury_minutes;
        q.legs = t
            .legs
            .iter()
            .map(|l| QuestLeg {
                kind: l.kind,
                minutes: l.minutes,
                attr: l.attr,
                target: l.target,
            })
            .collect();
        q.loot = t.loot.iter().map(|(i, p)| (*i as u16, *p)).collect();
        Some(q)
    }

    fn roll_procedural(&mut self, max_rank: u8) -> Option<Quest> {
        let c = self.content.clone();
        if c.categories.is_empty() {
            return None;
        }
        let rng = self.s.rng.questgen();
        let rank = rng.next_int(0, i32::from(max_rank) + 1) as u8;
        let category = rng.next_int(0, c.categories.len() as i32) as usize;
        let cat = &c.categories[category];
        let target = rng.next_int(0, cat.targets.len() as i32) as u8;
        let location = rng.next_int(0, c.locations.len() as i32) as u8;
        let r = &c.ranks[rank as usize];
        let mut legs = Vec::new();
        for (kind, attr) in &cat.legs {
            let minutes = 60 + 30 * i64::from(rng.next_int(0, 4));
            let jitter = rng.next_int(-1, 2);
            legs.push(QuestLeg {
                kind: *kind,
                minutes,
                attr: *attr,
                target: r.target_base + c.rank_offset(*kind) + jitter,
            });
        }
        // One random item of loot at a modest chance.
        let loot_item = rng.next_int(0, c.items.len() as i32) as u16;
        let mut q = self.new_quest(
            QuestSource::Procedural {
                category: category as u8,
                target,
                location,
            },
            rank,
        );
        q.gold = (r.gold_min, r.gold_max);
        q.xp = r.xp;
        q.injury_minutes = r.injury_minutes;
        q.legs = legs;
        q.loot = vec![(loot_item, 12)];
        Some(q)
    }

    pub(crate) fn eligible_quests(&self, id: CharId) -> impl Iterator<Item = &Quest> {
        let rank = self.s.chars[id as usize].rank;
        self.s
            .quests
            .iter()
            .filter(move |q| q.is_posted() && q.rank <= rank)
    }

    pub(crate) fn has_eligible_quest(&self, id: CharId) -> bool {
        self.eligible_quests(id).next().is_some()
    }

    /// At the board: an adventurer with no quest who is well takes one of the posted quests
    /// they are ranked for, uniformly. Returns true if they took one and are heading out.
    pub(crate) fn try_take_quest(&mut self, id: CharId) -> bool {
        let now = self.s.minute;
        let c = &self.s.chars[id as usize];
        if !c.is_adventurer() || c.quest.is_some() || c.is_injured(now) {
            return false;
        }
        // Adventurers reach for the hardest work they are ranked for: the best-paid jobs on the
        // board at their own rank if there are any, else the next rank down.
        let Some(best) = self.eligible_quests(id).map(|q| q.rank).max() else {
            return false;
        };
        let eligible: Vec<QuestId> = self
            .eligible_quests(id)
            .filter(|q| q.rank == best)
            .map(|q| q.id)
            .collect();
        let pick = self.s.rng.questgen().next_int(0, eligible.len() as i32) as usize;
        let qid = eligible[pick];
        let q = &mut self.s.quests[qid as usize];
        q.status = QuestStatus::Taken;
        q.taker = Some(id);
        self.s.chars[id as usize].quest = Some(qid);
        self.s.stats.taken += 1;
        self.emit(SimEvent::QuestTaken { quest: qid, by: id });
        self.walk_to_door(id, WalkPurpose::DepartQuest);
        true
    }

    /// At the door: pre-roll the whole itinerary and leave the map.
    pub(crate) fn depart_on_quest(&mut self, id: CharId) {
        let now = self.s.minute;
        let Some(qid) = self.s.chars[id as usize].quest else {
            self.enter_idle(id);
            return;
        };
        let serial_state = CharState::OffMap(OffMapReason::Quest);
        {
            let c = &mut self.s.chars[id as usize];
            c.serial = c.serial.wrapping_add(1);
            c.state = serial_state;
            c.state_entered = now;
            c.walk = None;
            c.activity = Activity::None;
        }
        let legs = self.s.quests[qid as usize].legs.clone();
        let rules = self.content.rules.clone();
        let hunger = self.s.chars[id as usize].hunger;
        let mut report = Vec::with_capacity(legs.len());
        for leg in &legs {
            let b = self.check_breakdown(id, leg.attr);
            let food = if hunger < rules.hunger_starving_below {
                -rules.starving_leg_penalty
            } else if hunger >= rules.well_fed_at_least && leg.kind != LegKind::Objective {
                rules.well_fed_leg_bonus
            } else {
                0
            };
            // Both draws always happen, pass or fail, so streams stay aligned (V2's rule).
            let roll = self.s.rng.dice().next_int(1, 21);
            let w = self.content.consequence(leg.kind);
            let total_w = w.nothing + w.injury + w.death;
            let cons_roll = self.s.rng.dice().next_int(0, total_w);
            let total = roll + b.attr_value + b.gear_capped + food;
            let passed = total >= leg.target;
            let consequence = if passed || cons_roll < w.nothing {
                Consequence::None
            } else if cons_roll < w.nothing + w.injury {
                Consequence::Injury
            } else {
                Consequence::Death
            };
            report.push(LegReport {
                roll,
                attr_value: b.attr_value,
                gear: b.gear_capped,
                food,
                target: leg.target,
                passed,
                consequence,
                resolved: false,
            });
        }
        // Rewards are rolled now too, so a save mid-quest needs no RNG to finish it.
        let q = &self.s.quests[qid as usize];
        let (gmin, gmax) = q.gold;
        let loot = q.loot.clone();
        let gold = i64::from(self.s.rng.economy().next_int(gmin, gmax + 1));
        let mut loot_won = Vec::new();
        for (item, percent) in loot {
            if self.s.rng.economy().chance(percent) {
                loot_won.push(item);
            }
        }
        let q = &mut self.s.quests[qid as usize];
        q.status = QuestStatus::Underway;
        q.departed = now;
        q.due_back = now + q.total_minutes();
        q.report = report;
        q.gold_won = gold;
        q.loot_won = loot_won;
        let mut at = now;
        for (i, leg) in legs.iter().enumerate() {
            at += leg.minutes;
            self.s.sched.schedule(
                now,
                at,
                Handler::QuestLeg,
                ((i as u64) << 32) | u64::from(qid),
            );
        }
        self.emit(SimEvent::QuestDeparted { quest: qid, by: id });
    }

    pub(crate) fn on_quest_leg(&mut self, payload: u64) {
        let leg = (payload >> 32) as usize;
        let qid = payload as u32 as QuestId;
        let Some(q) = self.s.quests.get_mut(qid as usize) else {
            return;
        };
        if q.status != QuestStatus::Underway {
            self.s.stats.stale_events += 1;
            return;
        }
        let Some(taker) = q.taker else { return };
        let now = self.s.minute;
        q.report[leg].resolved = true;
        let r = q.report[leg];
        let kind = q.legs[leg].kind;
        let attr = q.legs[leg].attr;
        let injury = q.injury_minutes;
        let last = leg + 1 == q.legs.len();

        // Skill experience accrues per leg checked.
        self.award_leg_skill_xp(taker, attr, r.passed);

        match r.consequence {
            Consequence::Death => {
                self.kill(taker, qid);
                return;
            }
            Consequence::Injury => {
                let c = &mut self.s.chars[taker as usize];
                c.injured_until = c.injured_until.max(now + injury);
                self.emit(SimEvent::Injured { character: taker });
            }
            Consequence::None => {}
        }
        if kind == LegKind::Objective && !r.passed {
            // The job failed; the road home still has to be walked.
        }
        if last {
            self.return_from_quest(taker, qid);
        }
    }

    fn objective_passed(q: &Quest) -> bool {
        q.legs
            .iter()
            .zip(&q.report)
            .any(|(l, r)| l.kind == LegKind::Objective && r.passed)
    }

    fn return_from_quest(&mut self, id: CharId, qid: QuestId) {
        let now = self.s.minute;
        let q = &self.s.quests[qid as usize];
        let succeeded = Self::objective_passed(q);
        let outcome = if succeeded {
            Outcome::Succeeded
        } else {
            Outcome::Failed
        };
        let (gold, loot, xp, rank) = (q.gold_won, q.loot_won.clone(), q.xp, q.rank);
        {
            let q = &mut self.s.quests[qid as usize];
            q.status = QuestStatus::Resolved(outcome);
            q.resolved_at = now;
            if !succeeded {
                q.gold_won = 0;
                q.loot_won.clear();
            }
        }
        let after_quest = self.content.rules.hunger_after_quest;
        {
            let c = &mut self.s.chars[id as usize];
            c.quest = None;
            c.cell = self.grid.door;
            c.hunger = after_quest;
            let adv = c.adv.as_mut().expect("adventurer");
            adv.rested_until = now + self.content.rules.rest_after_quest_minutes;
            if succeeded {
                adv.xp += xp;
                adv.quests_at_rank += u32::from(rank == c.rank);
                adv.quests_succeeded += 1;
                adv.gold_earned += gold;
            } else {
                adv.xp += xp / 2;
                adv.quests_failed += 1;
            }
        }
        if succeeded {
            self.s.stats.succeeded += 1;
            // The adventurer keeps their share; the guild banks the rest.
            let share = gold * self.content.rules.guild_share_percent / 100;
            self.credit(share, GoldReason::QuestReward);
            for item in loot {
                self.s.guild.stash[item as usize] += 1;
            }
            let renown = self.content.rules.renown_per_success[rank as usize];
            self.add_renown(renown);
        } else {
            self.s.stats.failed += 1;
        }
        self.enter_idle(id);
        self.emit(SimEvent::QuestReturned {
            quest: qid,
            by: id,
            outcome,
        });
        self.complete_objective(Objective::WatchReturn);
        self.check_class_progress(id);
        self.check_promotion(id);
    }

    fn kill(&mut self, id: CharId, qid: QuestId) {
        let now = self.s.minute;
        {
            let q = &mut self.s.quests[qid as usize];
            q.status = QuestStatus::Resolved(Outcome::Died);
            q.resolved_at = now;
            q.gold_won = 0;
            q.loot_won.clear();
        }
        let c = &mut self.s.chars[id as usize];
        c.serial = c.serial.wrapping_add(1);
        c.state = CharState::Dead;
        c.state_entered = now;
        c.quest = None;
        // What they carried is lost with them.
        if let Some(adv) = &mut c.adv {
            adv.equipment = [None; 5];
        }
        self.s.stats.died += 1;
        self.s.promotions.retain(|p| p.character != id);
        let penalty = self.content.rules.renown_per_death;
        self.add_renown(penalty);
        self.emit(SimEvent::Died {
            character: id,
            quest: qid,
        });
    }

    pub(crate) fn on_quest_expiry(&mut self, payload: u64) {
        let qid = payload as QuestId;
        let now = self.s.minute;
        if let Some(q) = self.s.quests.get_mut(qid as usize) {
            if q.status == QuestStatus::Posted {
                q.status = QuestStatus::Resolved(Outcome::Expired);
                q.resolved_at = now;
                self.s.stats.expired += 1;
                self.emit(SimEvent::QuestExpired { quest: qid });
            }
        }
    }

    pub(crate) fn add_renown(&mut self, delta: i64) {
        let g = &mut self.s.guild;
        g.renown = (g.renown + delta).max(0);
        let tier = self.content.renown_tier(g.renown);
        // Tiers only ratchet up: losing renown never re-locks a building.
        if tier > g.renown_tier {
            g.renown_tier = tier;
            self.emit(SimEvent::RenownTierReached { tier });
            if tier >= self.content.rules.renown_goal_tier
                && !self.s.objectives.is_done(Objective::ReachRenowned)
            {
                self.complete_objective(Objective::ReachRenowned);
                self.emit(SimEvent::DemoGoalReached);
            }
        }
    }

    /// The quest's display title, through the localisation registry.
    pub fn quest_title(&self, q: &Quest) -> String {
        let c = self.content();
        match q.source {
            QuestSource::Curated { template } => {
                c.loc.t(&c.quests[template as usize].title_key).to_string()
            }
            QuestSource::Procedural {
                category, target, ..
            } => {
                let cat = &c.categories[category as usize];
                c.loc.f(
                    "proc.title",
                    &[
                        c.loc.t(&cat.verb_key),
                        c.loc.t(&cat.targets[target as usize]),
                    ],
                )
            }
        }
    }

    pub fn quest_description(&self, q: &Quest) -> String {
        let c = self.content();
        match q.source {
            QuestSource::Curated { template } => {
                c.loc.t(&c.quests[template as usize].desc_key).to_string()
            }
            QuestSource::Procedural {
                category,
                target,
                location,
            } => {
                let cat = &c.categories[category as usize];
                c.loc.f(
                    "proc.desc",
                    &[
                        c.loc.t(&cat.verb_key),
                        c.loc.t(&cat.targets[target as usize]),
                        c.loc.t(&c.locations[location as usize]),
                    ],
                )
            }
        }
    }
}
