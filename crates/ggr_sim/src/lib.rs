//! The simulation: the whole game, with no engine in it.
//!
//! Its entire public surface is validated [`Command`]s in and [`SimEvent`]s out, plus read-only
//! views of state for presenters. Time only moves through [`World::advance`], one game-minute
//! at a time, so fast-forward is always "more ticks" and 16x lands on the same state as 1x.

mod behaviour;
mod building;
mod commands;
mod economy;
mod grid;
pub mod harness;
mod progression;
mod quests;
mod sched;
mod sections;
mod staff;
mod types;

pub use commands::{Command, CommandOk, Refusal};
pub use grid::{Grid, CELL_DOOR, CELL_FOOTPRINT, CELL_LOCKED, CELL_SLOT, CELL_WALL};
pub use progression::{CheckBreakdown, ClassProgress};
pub use sched::{Handler, ScheduledEvent, Scheduler};
pub use staff::HappinessModifier;
pub use types::*;

use std::sync::Arc;

use ggr_content::Content;
use ggr_core::RngStreams;

pub const MINUTES_PER_HOUR: i64 = 60;
pub const MINUTES_PER_DAY: i64 = 1440;

/// The world: all simulation state, the content it was built from, and the event queue the
/// presenters drain.
pub struct World {
    content: Arc<Content>,
    pub(crate) s: State,
    pub(crate) grid: Grid,
    pub(crate) events: Vec<SimEvent>,
    pub(crate) scratch: grid::Scratch,
}

impl World {
    /// A new guild: the starting hall, roster and treasury, with the first events booked.
    pub fn new(content: Arc<Content>, seed: u64) -> World {
        let rules = &content.rules;
        let s = State {
            minute: rules.start_minute,
            rng: RngStreams::new(seed),
            sched: Scheduler::default(),
            instances: Vec::new(),
            east_wing_open: false,
            chars: Vec::new(),
            quests: Vec::new(),
            guild: Guild {
                gold: rules.starting_gold,
                stash: vec![0; content.items.len()],
                renown: 0,
                renown_tier: 0,
                last_change: 0,
                last_reason: GoldReason::None,
            },
            candidates: Vec::new(),
            promotions: Vec::new(),
            objectives: Objectives::default(),
            stats: Stats::default(),
            audit: SlotAudit::default(),
            next_candidate_id: 1,
            foreign_sections: Default::default(),
        };
        let grid = Grid::from_layout(&content.layout);
        let mut w = World {
            content,
            s,
            grid,
            events: Vec::new(),
            scratch: grid::Scratch::default(),
        };
        w.build_starting_hall();
        w.spawn_starting_roster();
        let now = w.s.minute;
        w.s.sched
            .schedule(now, now + 1, Handler::QuestGeneration, 0);
        let first_candidate = now + 60;
        w.s.sched
            .schedule(now, first_candidate, Handler::CandidateArrival, 0);
        w
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    pub fn content_arc(&self) -> Arc<Content> {
        self.content.clone()
    }

    pub fn minute(&self) -> i64 {
        self.s.minute
    }

    /// Day number, counted from 1.
    pub fn day(&self) -> i64 {
        self.s.minute / MINUTES_PER_DAY + 1
    }

    pub fn hour(&self) -> i64 {
        (self.s.minute % MINUTES_PER_DAY) / MINUTES_PER_HOUR
    }

    pub fn minute_of_hour(&self) -> i64 {
        self.s.minute % MINUTES_PER_HOUR
    }

    /// Moves time forward `minutes` game-minutes, one at a time.
    pub fn advance(&mut self, minutes: i64) {
        for _ in 0..minutes {
            self.tick();
        }
    }

    fn tick(&mut self) {
        self.s.minute += 1;
        let now = self.s.minute;
        if now % MINUTES_PER_DAY == 0 {
            self.on_day_start();
        }
        if now % MINUTES_PER_HOUR == 0 {
            self.on_hour();
        }
        // Handlers may book more work in this same minute only through future minutes; the
        // scheduler takes the bucket out before running it.
        if let Some(bucket) = self.s.sched.take(now) {
            for ev in &bucket {
                self.dispatch(*ev);
            }
            self.s.sched.recycle(bucket);
        }
    }

    fn dispatch(&mut self, ev: ScheduledEvent) {
        match ev.handler {
            Handler::IdleDecision => self.on_idle_decision(ev.payload),
            Handler::Arrival => self.on_arrival(ev.payload),
            Handler::InteractionEnd => self.on_interaction_end(ev.payload),
            Handler::QuestGeneration => self.on_quest_generation(),
            Handler::QuestLeg => self.on_quest_leg(ev.payload),
            Handler::QuestExpiry => self.on_quest_expiry(ev.payload),
            Handler::CandidateArrival => self.on_candidate_arrival(),
            Handler::CandidateExpiry => self.on_candidate_expiry(ev.payload),
            Handler::ConstructionDone => self.on_construction_done(ev.payload),
            Handler::ServicePoll => self.on_service_poll(ev.payload),
            Handler::PromotionExpiry => self.on_promotion_expiry(ev.payload),
        }
    }

    fn on_day_start(&mut self) {
        let day = self.day();
        self.pay_wages();
        self.pay_upkeep();
        self.check_volunteer_floor();
        self.events.push(SimEvent::DayStarted { day });
    }

    fn on_hour(&mut self) {
        self.hourly_needs();
        self.hourly_staff();
    }

    /// Takes every event raised since the last drain.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    // ---- read-only views for presenters and tests ----

    pub fn grid(&self) -> &Grid {
        &self.grid
    }
    pub fn characters(&self) -> &[Character] {
        &self.s.chars
    }
    pub fn character(&self, id: CharId) -> Option<&Character> {
        self.s.chars.get(id as usize)
    }
    pub fn instances(&self) -> &[Instance] {
        &self.s.instances
    }
    pub fn quests(&self) -> &[Quest] {
        &self.s.quests
    }
    pub fn quest(&self, id: QuestId) -> Option<&Quest> {
        self.s.quests.get(id as usize)
    }
    pub fn guild(&self) -> &Guild {
        &self.s.guild
    }
    pub fn candidates(&self) -> &[Candidate] {
        &self.s.candidates
    }
    pub fn waiting_candidates(&self) -> impl Iterator<Item = &Candidate> {
        self.s
            .candidates
            .iter()
            .filter(|c| c.status == CandidateStatus::Waiting)
    }
    pub fn promotions(&self) -> &[PromotionOffer] {
        &self.s.promotions
    }
    pub fn objectives(&self) -> &Objectives {
        &self.s.objectives
    }
    pub fn stats(&self) -> &Stats {
        &self.s.stats
    }
    pub fn audit(&self) -> &SlotAudit {
        &self.s.audit
    }
    pub fn east_wing_open(&self) -> bool {
        self.s.east_wing_open
    }
    pub fn scheduler(&self) -> &Scheduler {
        &self.s.sched
    }
    pub fn rng_seed(&self) -> u64 {
        self.s.rng.root_seed()
    }

    /// Path of cells from `from` to `to` (inclusive of both) under the current grid, for the
    /// presenter to draw a walk along. Empty if unreachable.
    pub fn path(&self, from: Cell, to: Cell) -> Vec<Cell> {
        self.grid.path(from, to)
    }

    /// FNV-1a over the canonical serialised state: walks the whole world, scheduler included.
    pub fn state_hash(&self) -> u64 {
        let sections = self.to_sections();
        let mut h = ggr_core::StateHasher::new();
        for (name, (version, value)) in &sections {
            h.str(name).u32(*version);
            h.str(&value.to_string());
        }
        h.finish()
    }

    pub(crate) fn emit(&mut self, e: SimEvent) {
        self.events.push(e);
    }
}

#[cfg(test)]
mod tests;

/// Every sim-owned section and the version this build reads, for the persistence layer.
pub const SECTION_VERSIONS: &[(&str, u32)] = &sections::SECTIONS;
pub use sections::Sections;
