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

/// A scripted "reasonable player", acting only through commands: hires within means, builds a
/// canteen and the services, staffs them, accepts promotions, equips loot, gives everyone an
/// aspiration, and opens the East Wing. The balance tests use it to measure the demo's road
/// (how many days to Renowned) and to prove a reasonably played guild never death-spirals.
pub fn reasonable_player(w: &mut World) {
    use crate::Command;
    use ggr_content::PrefabKind;
    let c = w.content_arc();
    let adventurers = |w: &World| {
        w.characters()
            .iter()
            .filter(|x| x.is_adventurer() && x.on_roster())
            .count()
    };
    let staff_of = |w: &World, kind: PrefabKind| {
        let role = c.role_for_station(kind).map(|r| r as u8);
        w.characters()
            .iter()
            .filter(|x| x.on_roster() && x.staff.as_ref().map(|s| s.role) == role)
            .count()
    };
    let has = |w: &World, kind: PrefabKind| {
        w.instances()
            .iter()
            .any(|i| i.status != InstanceStatus::Demolished && w.kind_of(i.id) == kind)
    };

    // Hire.
    let waiting: Vec<(u32, i64, Option<u8>)> = w
        .waiting_candidates()
        .map(|x| (x.id, x.cost, x.staff_role))
        .collect();
    for (id, cost, role) in waiting {
        let want = match role {
            None => adventurers(w) < 10,
            Some(r) => {
                let kind = c.roles[r as usize].station;
                let n = staff_of(w, kind);
                match kind {
                    PrefabKind::Desk => n < 2,
                    PrefabKind::Canteen => n < 1 && has(w, PrefabKind::Canteen),
                    PrefabKind::Infirmary => n < 1 && has(w, PrefabKind::Infirmary),
                    _ => false,
                }
            }
        };
        if want && w.guild().gold - cost >= 80 {
            let _ = w.execute(Command::Hire { candidate: id });
        }
    }
    // Shifts: two clerks cover the day and the evening.
    let clerks: Vec<CharId> = w
        .characters()
        .iter()
        .filter(|x| {
            x.on_roster()
                && x.staff
                    .as_ref()
                    .is_some_and(|s| c.roles[s.role as usize].station == PrefabKind::Desk)
        })
        .map(|x| x.id)
        .collect();
    for (n, id) in clerks.iter().enumerate() {
        let (start, len) = if n == 0 { (6, 10) } else { (16, 8) };
        let st = w.characters()[*id as usize].staff.as_ref().unwrap().shift;
        if st.start_hour != start || st.length != len {
            let _ = w.execute(Command::SetShift {
                character: *id,
                start_hour: start,
                length: len,
            });
        }
    }
    // Build.
    let gold = w.guild().gold;
    let wish: [(&str, i64, bool); 5] = [
        ("prefab.canteen", 260, !has(w, PrefabKind::Canteen)),
        (
            "prefab.target_archery",
            200,
            w.instances()
                .iter()
                .filter(|i| w.kind_of(i.id) == PrefabKind::Training)
                .count()
                < 2,
        ),
        ("prefab.infirmary", 320, !has(w, PrefabKind::Infirmary)),
        ("prefab.banner", 150, w.decor_count() < 2),
        (
            "prefab.focus_arcane",
            300,
            w.instances()
                .iter()
                .filter(|i| w.kind_of(i.id) == PrefabKind::Training)
                .count()
                < 3,
        ),
    ];
    for (id, floor, need) in wish {
        let Some(p) = c.prefab_index(id) else {
            continue;
        };
        if !need || gold < floor || !w.prefab_unlocked(p) {
            continue;
        }
        'spot: for y in 2..26 {
            for x in 3..26 {
                if w.execute(Command::Place { prefab: p, x, y }).is_ok() {
                    break 'spot;
                }
            }
        }
        break;
    }
    if !w.east_wing_open() && w.guild().gold > c.rules.east_wing_cost + 250 {
        let _ = w.execute(Command::OpenEastWing);
    }
    // Promotions, equipment, aspirations.
    let offers: Vec<(CharId, i64)> = w
        .promotions()
        .iter()
        .map(|p| (p.character, p.fee))
        .collect();
    for (id, fee) in offers {
        if w.guild().gold - fee >= 60 {
            let _ = w.execute(Command::AcceptPromotion { character: id });
        }
    }
    for item in 0..c.items.len() {
        if w.guild().stash[item] == 0 {
            continue;
        }
        let slot = c.items[item].slot.index();
        let target = w
            .characters()
            .iter()
            .find(|x| x.on_map() && x.adv.as_ref().is_some_and(|a| a.equipment[slot].is_none()));
        if let Some(t) = target.map(|t| t.id) {
            let _ = w.execute(Command::Equip { character: t, item });
        }
    }
    let unaspired: Vec<(CharId, usize)> = w
        .characters()
        .iter()
        .filter(|x| x.on_roster())
        .filter_map(|x| {
            let a = x.adv.as_ref()?;
            if a.aspiration.is_some() || c.classes[a.class as usize].tier > 0 {
                return None;
            }
            Some((x.id, c.classes[a.class as usize].branch))
        })
        .collect();
    for (id, branch) in unaspired {
        if let Some(k) = c
            .classes
            .iter()
            .position(|k| k.branch == branch && k.tier == 1)
        {
            let _ = w.execute(Command::SetAspiration {
                character: id,
                class: Some(k as u8),
            });
        }
    }
}

/// Plays `days` with the reasonable player acting every game-hour. Returns the day each renown
/// tier was first reached (None if never).
pub fn play_reasonably(w: &mut World, days: i64) -> Vec<Option<i64>> {
    let tiers = w.content().rules.renown_tiers.len();
    let mut reached = vec![None; tiers];
    reached[0] = Some(1);
    for _ in 0..(days * 24) {
        reasonable_player(w);
        w.advance(60);
        let t = w.guild().renown_tier;
        for r in reached.iter_mut().take(t + 1) {
            if r.is_none() {
                *r = Some(w.day());
            }
        }
        w.drain_events();
    }
    reached
}
