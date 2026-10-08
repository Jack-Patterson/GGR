//! The balance harness: thousands of seeded quest runs per rank, measured, so the curve is
//! seen rather than guessed at. Used by the tests (bands asserted) and by `ggr_smoke --balance`.

use std::sync::Arc;

use ggr_content::Content;

use crate::types::*;
use crate::World;

#[derive(Debug, Clone, Copy, Default)]
pub struct BandResult {
    pub runs: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub died: u32,
    pub injured: u32,
    pub gold: i64,
}

impl BandResult {
    pub fn per_thousand(&self, n: u32) -> u32 {
        n * 1000 / self.runs.max(1)
    }
}

/// Sends a freshly generated, unequipped adventurer of `rank` on one quest of that rank and
/// returns how it went.
pub fn run_one(content: &Arc<Content>, rank: u8, seed: u64) -> (Outcome, bool, i64) {
    let mut w = World::new(content.clone(), seed);
    // A clean hall: only the adventurer under test, already at the door.
    for c in &mut w.s.chars {
        c.serial += 1;
        c.state = CharState::OffMap(OffMapReason::Left);
    }
    for inst in &mut w.s.instances {
        for s in &mut inst.slots {
            s.holder = None;
            s.occupied = false;
        }
    }
    let attrs = {
        let base = w.s.chars[0].attrs;
        let shift = content.ranks[rank as usize].attribute_floor - content.ranks[0].attribute_floor;
        base.map(|a| a + shift)
    };
    let apt = w.s.chars[0].adv.as_ref().unwrap().aptitude.clone();
    let info = w.new_adventurer_info(apt);
    let id = w.spawn_character(0, 0, attrs, rank, None, Some(info));
    // Post one quest of exactly this rank.
    let quest = loop {
        w.on_quest_generation();
        if let Some(q) = w.s.quests.iter().find(|q| q.is_posted() && q.rank == rank) {
            break q.id;
        }
        for q in &mut w.s.quests {
            if q.is_posted() {
                q.status = QuestStatus::Resolved(Outcome::Expired);
            }
        }
    };
    let q = &mut w.s.quests[quest as usize];
    q.status = QuestStatus::Taken;
    q.taker = Some(id);
    w.s.chars[id as usize].quest = Some(quest);
    w.depart_on_quest(id);
    let total = w.s.quests[quest as usize].total_minutes();
    w.advance(total + 1);
    let q = &w.s.quests[quest as usize];
    let injured = q
        .report
        .iter()
        .any(|r| r.consequence == Consequence::Injury);
    (q.outcome().expect("resolved"), injured, q.gold_won)
}

pub fn run_band(content: &Arc<Content>, rank: u8, runs: u32, seed0: u64) -> BandResult {
    let mut r = BandResult::default();
    for i in 0..runs {
        let (o, injured, gold) = run_one(content, rank, seed0 + u64::from(i));
        r.runs += 1;
        match o {
            Outcome::Succeeded => r.succeeded += 1,
            Outcome::Failed => r.failed += 1,
            Outcome::Died => r.died += 1,
            Outcome::Expired => {}
        }
        r.injured += u32::from(injured);
        r.gold += gold;
    }
    r
}

/// The bands the demo's balance pass holds the curve to, per thousand departures:
/// (success min, success max, death max), by rank. Measured with the harness, then widened by
/// a margin so a seed change does not flap them.
pub const BANDS: [(u32, u32, u32); 3] = [(800, 990, 40), (700, 960, 70), (600, 930, 110)];
