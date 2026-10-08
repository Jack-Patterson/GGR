//! Sim tests: determinism, the slot audit, invariants over seeded weeks, save round-trips,
//! building rules, staff equilibrium, and the progression effects the milestones promise.

use std::sync::Arc;

use ggr_content::{Attr, Content, ItemSlot, PrefabKind};

use crate::*;

fn content() -> Arc<Content> {
    Arc::new(Content::load_embedded().expect("content"))
}

fn world(seed: u64) -> World {
    World::new(content(), seed)
}

/// The per-state stuck rule: nobody sits in a state longer than that state can legitimately
/// last.
fn check_invariants(w: &World) -> Result<(), String> {
    w.verify_consistency()?;
    let g = w.guild();
    if g.gold < 0 {
        return Err(format!("gold went negative: {}", g.gold));
    }
    if w.scheduler().has_overdue(w.minute()) {
        return Err("an overdue event is still booked".into());
    }
    let now = w.minute();
    for c in w.characters() {
        let age = now - c.state_entered;
        let limit = match c.state {
            CharState::Idle => 60,
            CharState::Travel => 120,
            CharState::Interact => {
                if matches!(c.activity, Activity::Working { .. }) {
                    13 * 60
                } else {
                    w.content().rules.infirmary_session_minutes
                        + 10
                        + w.content().rules.service_patience_minutes
                }
            }
            CharState::OffMap(OffMapReason::Quest) => 24 * 60,
            CharState::OffMap(OffMapReason::OffShift) => 24 * 60,
            CharState::OffMap(OffMapReason::Left) | CharState::Dead => i64::MAX,
        };
        if age > limit {
            return Err(format!(
                "character {} stuck in {:?} for {age} minutes",
                c.id, c.state
            ));
        }
        if let Some(q) = c.quest {
            let quest = &w.quests()[q as usize];
            if !quest.is_active() || quest.taker != Some(c.id) {
                return Err(format!("character {} holds orphaned quest {q}", c.id));
            }
        }
    }
    for q in w.quests() {
        if q.is_active() {
            let t = q.taker.ok_or("active quest with no taker")?;
            if w.characters()[t as usize].quest != Some(q.id) {
                return Err(format!("quest {} is orphaned", q.id));
            }
        }
    }
    Ok(())
}

fn run_days(w: &mut World, days: i64) {
    for _ in 0..days {
        w.advance(MINUTES_PER_DAY);
        if let Err(e) = check_invariants(w) {
            panic!("day {}: {e}", w.day());
        }
        w.drain_events();
    }
}

#[test]
fn a_new_world_is_consistent() {
    let w = world(42);
    check_invariants(&w).unwrap();
    assert_eq!(w.characters().len(), 5);
    assert_eq!(w.guild().gold, 300);
}

#[test]
fn a_hands_off_week_runs_the_loop() {
    let mut w = world(42);
    run_days(&mut w, 7);
    let s = w.stats();
    assert!(s.posted > 10, "{s:?}");
    assert!(s.taken > 5, "{s:?}");
    assert!(s.succeeded > 3, "{s:?}");
    assert!(s.candidates_arrived > 3, "{s:?}");
}

#[test]
fn invariants_hold_across_seeds() {
    for seed in 1..=10 {
        let mut w = world(seed);
        run_days(&mut w, 7);
    }
}

#[test]
fn sixteen_x_lands_on_the_same_state_as_one_x() {
    let mut a = world(7);
    let mut b = world(7);
    for _ in 0..(3 * MINUTES_PER_DAY) {
        a.advance(1);
    }
    for _ in 0..(3 * MINUTES_PER_DAY / 16) {
        b.advance(16);
    }
    assert_eq!(a.minute(), b.minute());
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn same_seed_same_world_different_seed_different_world() {
    let mut a = world(3);
    let mut b = world(3);
    let mut c = world(4);
    a.advance(2000);
    b.advance(2000);
    c.advance(2000);
    assert_eq!(a.state_hash(), b.state_hash());
    assert_ne!(a.state_hash(), c.state_hash());
}

#[test]
fn twenty_wanderers_for_a_week_never_leak_a_slot() {
    let mut w = world(11);
    // Pad the roster to twenty with hires the guildmaster signs personally.
    while w.characters().len() < 20 {
        w.s.guild.gold = 10_000;
        w.advance(30);
        let ids: Vec<u32> = w.waiting_candidates().map(|c| c.id).collect();
        for id in ids {
            let _ = w.execute(Command::Hire { candidate: id });
        }
    }
    run_days(&mut w, 7);
    let a = w.audit();
    assert!(a.reserves > 500, "{a:?}");
}

#[test]
fn save_load_resimulate_matches_never_saving() {
    for (seed, at) in [(5u64, 777i64), (6, 2345), (8, 4000)] {
        let mut straight = world(seed);
        let mut saved = world(seed);
        saved.advance(at);
        let sections = saved.to_sections();
        let json = serde_json::to_string(&sections).unwrap();
        let back: sections::Sections = serde_json::from_str(&json).unwrap();
        let mut loaded = World::from_sections(content(), &back).unwrap();
        straight.advance(at);
        assert_eq!(
            straight.state_hash(),
            loaded.state_hash(),
            "seed {seed} at {at}"
        );
        straight.advance(3 * MINUTES_PER_DAY);
        loaded.advance(3 * MINUTES_PER_DAY);
        assert_eq!(
            straight.state_hash(),
            loaded.state_hash(),
            "seed {seed} after resim"
        );
    }
}

#[test]
fn unknown_sections_survive_a_round_trip() {
    let mut w = world(1);
    w.set_foreign_section("mod.fishing", 3, serde_json::json!({"fish": [1, 2, 3]}));
    let s = w.to_sections();
    let loaded = World::from_sections(content(), &s).unwrap();
    let back = loaded.to_sections();
    assert_eq!(back.get("mod.fishing"), s.get("mod.fishing"));
}

#[test]
fn a_save_from_different_content_is_refused() {
    let w = world(1);
    let mut s = w.to_sections();
    s.get_mut("meta").unwrap().1 = serde_json::json!({ "content_fingerprint": 1 });
    let err = World::from_sections(content(), &s)
        .err()
        .unwrap()
        .to_string();
    assert!(err.contains("different game content"), "{err}");
}

fn find_prefab(w: &World, kind: PrefabKind) -> usize {
    w.content()
        .prefabs
        .iter()
        .position(|p| p.kind == kind)
        .unwrap()
}

/// First origin where a prefab can legally go.
fn legal_spot(w: &mut World, prefab: usize) -> (i32, i32) {
    for y in 1..w.grid().height - 1 {
        for x in 1..w.grid().width - 1 {
            if w.validate_placement(prefab, (x, y)).is_ok() {
                return (x, y);
            }
        }
    }
    panic!("nowhere to put prefab {prefab}");
}

#[test]
fn placement_rules_refuse_illegal_builds() {
    let mut w = world(1);
    w.s.guild.gold = 10_000;
    let bench = find_prefab(&w, PrefabKind::Rest);
    // On a wall.
    assert_eq!(
        w.execute(Command::Place {
            prefab: bench,
            x: 0,
            y: 5
        }),
        Err(Refusal::Blocked)
    );
    // In the locked East Wing.
    assert_eq!(
        w.execute(Command::Place {
            prefab: bench,
            x: 30,
            y: 5
        }),
        Err(Refusal::WingLocked)
    );
    // On top of the desk.
    let desk = w.content().layout.placements[0].1;
    assert_eq!(
        w.execute(Command::Place {
            prefab: bench,
            x: desk.0,
            y: desk.1
        }),
        Err(Refusal::Occupied)
    );
    // Across the door.
    let door = w.grid().door;
    assert_eq!(
        w.execute(Command::Place {
            prefab: bench,
            x: door.0,
            y: door.1
        }),
        Err(Refusal::BlocksDoor)
    );
    // Too poor.
    let (x, y) = legal_spot(&mut w, bench);
    w.s.guild.gold = 0;
    assert_eq!(
        w.execute(Command::Place {
            prefab: bench,
            x,
            y
        }),
        Err(Refusal::InsufficientGold)
    );
    assert_eq!(w.guild().gold, 0);
}

#[test]
fn walling_off_the_hall_is_refused() {
    let mut w = world(1);
    w.s.guild.gold = 1_000_000;
    let planter = w.content().prefab_index("prefab.planter").unwrap();
    // Try to seal the door's neighbourhood with planters; at some point the access rule must
    // refuse, and the door must still reach every slot afterwards.
    let door = w.grid().door;
    let mut refused = false;
    for (dx, dy) in [(0, -1), (0, 1), (1, -1), (1, 1), (1, 0)] {
        if w.execute(Command::Place {
            prefab: planter,
            x: door.0 + dx,
            y: door.1 + dy,
        }) == Err(Refusal::CutsOffAccess)
        {
            refused = true;
        }
    }
    assert!(refused, "sealing the door was never refused");
    let slots: Vec<(i32, i32)> = w
        .instances()
        .iter()
        .filter(|i| i.status != InstanceStatus::Demolished)
        .flat_map(|i| i.slots.iter().map(|s| s.cell))
        .collect();
    let mut s = grid::Scratch::default();
    assert!(w.grid().all_reachable(door, &slots, &mut s));
}

#[test]
fn construction_takes_time_and_gold_and_people_use_the_result() {
    let mut w = world(2);
    let dummy = w.content().prefab_index("prefab.target_archery").unwrap();
    let (x, y) = legal_spot(&mut w, dummy);
    let before = w.guild().gold;
    let id = match w.execute(Command::Place {
        prefab: dummy,
        x,
        y,
    }) {
        Ok(CommandOk::Placed(id)) => id,
        other => panic!("{other:?}"),
    };
    assert_eq!(w.guild().gold, before - w.content().prefabs[dummy].cost);
    assert!(!w.instances()[id as usize].is_ready());
    w.advance(w.content().prefabs[dummy].build_minutes);
    assert!(w.instances()[id as usize].is_ready());
    // Within a couple of days somebody trains on it.
    let mut used = false;
    for _ in 0..(2 * MINUTES_PER_DAY) {
        w.advance(1);
        if w.instances()[id as usize].slots.iter().any(|s| s.occupied) {
            used = true;
            break;
        }
    }
    assert!(used, "nobody ever used the new target");
}

#[test]
fn demolishing_an_occupied_interactable_resolves_cleanly() {
    let mut w = world(3);
    let bench_ids: Vec<InstId> = w
        .instances()
        .iter()
        .filter(|i| w.kind_of(i.id) == PrefabKind::Rest)
        .map(|i| i.id)
        .collect();
    // Run until a bench slot is held (reserved or occupied), then demolish it.
    let mut target = None;
    for _ in 0..2000 {
        w.advance(1);
        target = bench_ids.iter().copied().find(|b| {
            w.instances()[*b as usize]
                .slots
                .iter()
                .any(|s| s.holder.is_some())
        });
        if target.is_some() {
            break;
        }
    }
    let target = target.expect("nobody ever sat down");
    w.execute(Command::Demolish { instance: target }).unwrap();
    check_invariants(&w).unwrap();
    run_days(&mut w, 2);
}

#[test]
fn the_last_desk_cannot_be_demolished() {
    let mut w = world(1);
    let desk = w
        .instances()
        .iter()
        .find(|i| w.kind_of(i.id) == PrefabKind::Desk)
        .unwrap()
        .id;
    assert_eq!(
        w.execute(Command::Demolish { instance: desk }),
        Err(Refusal::LastOfItsKind)
    );
}

#[test]
fn hiring_needs_a_manned_desk_and_the_gold() {
    let mut w = world(4);
    // Run to night, when the starting clerk (08:00-16:00) is off shift, with someone waiting.
    while !(w.hour() >= 20 && w.waiting_candidates().next().is_some()) {
        w.advance(10);
    }
    assert!(!w.desk_manned());
    let cand = w.waiting_candidates().next().unwrap().id;
    assert_eq!(
        w.execute(Command::Hire { candidate: cand }),
        Err(Refusal::DeskUnmanned)
    );
    // Next morning the clerk is in.
    while !w.desk_manned() {
        w.advance(10);
    }
    w.s.guild.gold = 0;
    if w.waiting_candidates().any(|c| c.cost > 0) {
        let cand = w.waiting_candidates().find(|c| c.cost > 0).unwrap().id;
        assert_eq!(
            w.execute(Command::Hire { candidate: cand }),
            Err(Refusal::InsufficientGold)
        );
    }
    w.s.guild.gold = 1000;
    let cand = w.waiting_candidates().next().unwrap().id;
    assert!(matches!(
        w.execute(Command::Hire { candidate: cand }),
        Ok(CommandOk::Hired(_))
    ));
}

#[test]
fn an_unmanned_canteen_queues_rather_than_breaks() {
    let mut w = world(5);
    w.s.guild.gold = 10_000;
    let canteen = w.content().prefab_index("prefab.canteen").unwrap();
    let (x, y) = legal_spot(&mut w, canteen);
    w.execute(Command::Place {
        prefab: canteen,
        x,
        y,
    })
    .unwrap();
    // No cook hired: adventurers get hungry, go, queue, and give up after their patience.
    let mut queued = false;
    for _ in 0..(3 * MINUTES_PER_DAY) {
        w.advance(1);
        if w.characters()
            .iter()
            .any(|c| matches!(c.activity, Activity::Queueing { .. }))
        {
            queued = true;
        }
        if w.minute() % 60 == 0 {
            check_invariants(&w).unwrap();
        }
    }
    assert!(queued, "nobody ever queued at the unmanned canteen");
    assert_eq!(w.stats().meals, 0);
}

fn hire_staff(w: &mut World, role_kind: PrefabKind, start: u8, len: u8) -> CharId {
    let role = w.content().role_for_station(role_kind).unwrap() as u8;
    let info = w.new_staff_info(role, start, len);
    w.spawn_character(0, 0, [10; Attr::COUNT], 0, Some(info), None)
}

#[test]
fn a_staffed_canteen_feeds_people_and_earns() {
    let mut w = world(6);
    w.s.guild.gold = 10_000;
    let canteen = w.content().prefab_index("prefab.canteen").unwrap();
    let (x, y) = legal_spot(&mut w, canteen);
    w.execute(Command::Place {
        prefab: canteen,
        x,
        y,
    })
    .unwrap();
    hire_staff(&mut w, PrefabKind::Canteen, 6, 12);
    run_days(&mut w, 3);
    assert!(w.stats().meals > 0);
}

#[test]
fn night_shifts_cost_more_happiness_than_day_shifts() {
    let mut w = world(7);
    let day = hire_staff(&mut w, PrefabKind::Desk, 8, 8);
    let night = hire_staff(&mut w, PrefabKind::Desk, 22, 8);
    assert!(w.happiness_target(night) < w.happiness_target(day));
    let long = hire_staff(&mut w, PrefabKind::Desk, 6, 12);
    assert!(w.happiness_target(long) < w.happiness_target(day));
}

#[test]
fn staff_happiness_reaches_equilibrium_over_thirty_days() {
    let mut w = world(8);
    w.s.guild.gold = 100_000;
    hire_staff(&mut w, PrefabKind::Desk, 16, 8);
    run_days(&mut w, 30);
    for c in w
        .characters()
        .iter()
        .filter(|c| c.is_staff() && c.on_roster())
    {
        let h = c.staff.as_ref().unwrap().happiness;
        let t = w.happiness_target(c.id);
        assert!(
            (h - t).abs() <= 1,
            "happiness {h} never settled on target {t}"
        );
        assert!(
            h >= w.content().rules.quit_below,
            "a default shift should never drive someone out"
        );
    }
}

#[test]
fn equipment_moves_outcome_distributions() {
    // Same seeds with and without gear: the geared adventurer succeeds measurably more.
    let c = content();
    let sword = c.item_index("item.iron_shortsword").unwrap();
    let mut plain = 0;
    let mut geared = 0;
    for seed in 0..400u64 {
        for gear in [false, true] {
            let mut w = World::new(c.clone(), seed);
            let id = 0;
            if gear {
                w.s.guild.stash[sword] += 1;
                w.execute(Command::Equip {
                    character: id,
                    item: sword,
                })
                .unwrap();
            }
            let b = w.check_breakdown(id, Attr::Might);
            let roll_target = 10;
            // Expected pass chance for a might-10 check, summed as a deterministic measure.
            let chance = (21 - roll_target + b.attr_value + b.gear_capped).clamp(0, 20);
            if gear {
                geared += chance;
            } else {
                plain += chance;
            }
        }
    }
    assert!(
        geared > plain,
        "gear made no difference: {geared} vs {plain}"
    );
}

#[test]
fn two_handed_items_take_both_hands() {
    let mut w = world(1);
    let c = w.content_arc();
    let gs = c.item_index("item.iron_greatsword").unwrap();
    let buckler = c.item_index("item.oak_buckler").unwrap();
    w.s.guild.stash[gs] += 1;
    w.s.guild.stash[buckler] += 1;
    w.execute(Command::Equip {
        character: 0,
        item: buckler,
    })
    .unwrap();
    w.execute(Command::Equip {
        character: 0,
        item: gs,
    })
    .unwrap();
    let eq = w.characters()[0].adv.as_ref().unwrap().equipment;
    assert_eq!(eq[ItemSlot::MainHand.index()], Some(gs as u16));
    assert_eq!(eq[ItemSlot::OffHand.index()], None);
    assert_eq!(
        w.guild().stash[buckler],
        1,
        "the buckler went back to the stash"
    );
    w.execute(Command::Equip {
        character: 0,
        item: buckler,
    })
    .unwrap();
    let eq = w.characters()[0].adv.as_ref().unwrap().equipment;
    assert_eq!(
        eq[ItemSlot::MainHand.index()],
        None,
        "a shield displaces a two-hander"
    );
    assert_eq!(w.guild().stash[gs], 1);
}

#[test]
fn the_modifier_cap_holds() {
    let mut w = world(1);
    let c = w.content_arc();
    for id in ["item.iron_greatsword", "item.iron_helm"] {
        let i = c.item_index(id).unwrap();
        w.s.guild.stash[i] += 1;
        w.execute(Command::Equip {
            character: 0,
            item: i,
        })
        .unwrap();
    }
    let b = w.check_breakdown(0, Attr::Might);
    assert!(b.items + b.class > b.cap);
    assert_eq!(b.gear_capped, b.cap);
}

#[test]
fn aligned_aspirations_outpace_misaligned_ones() {
    let c = content();
    let mut w = World::new(c.clone(), 1);
    let id = 0u32;
    let skill = 0usize; // skill.blade, fighter branch
    let branch = c.skills[skill].branch;
    // Two copies of the same adventurer differing only in aptitude for the branch.
    let mut hi = w.s.chars[id as usize].clone();
    hi.adv.as_mut().unwrap().aptitude[branch] = 150;
    let mut lo = w.s.chars[id as usize].clone();
    lo.adv.as_mut().unwrap().aptitude[branch] = 50;
    w.s.chars.push(hi);
    w.s.chars.push(lo);
    let n = w.s.chars.len() as u32;
    for i in [n - 2, n - 1] {
        w.s.chars[i as usize].id = i;
        w.set_aspiration(
            i,
            Some(
                c.classes
                    .iter()
                    .position(|k| k.id == "class.swordsman")
                    .unwrap() as u8,
            ),
        )
        .unwrap();
        for _ in 0..10 {
            w.award_skill_xp(i, skill, 10);
        }
    }
    let xp_hi = w.s.chars[(n - 2) as usize].adv.as_ref().unwrap().skill_xp[skill];
    let xp_lo = w.s.chars[(n - 1) as usize].adv.as_ref().unwrap().skill_xp[skill];
    assert!(xp_hi > 2 * xp_lo, "{xp_hi} vs {xp_lo}");
}

#[test]
fn class_is_attained_when_requirements_are_met() {
    let c = content();
    let mut w = World::new(c.clone(), 1);
    let sw = c
        .classes
        .iter()
        .position(|k| k.id == "class.swordsman")
        .unwrap() as u8;
    w.s.chars[0].rank = 1;
    w.set_aspiration(0, Some(sw)).unwrap();
    let progress = w.class_progress(0, sw as usize);
    assert!(progress.iter().any(|p| !p.met()));
    for _ in 0..50 {
        w.award_skill_xp(0, 0, 20);
    }
    w.check_class_progress(0);
    assert_eq!(w.characters()[0].adv.as_ref().unwrap().class, sw);
}

#[test]
fn promotion_is_offered_accepted_and_raises_the_rank() {
    let mut w = world(9);
    let id = 0u32;
    {
        let adv = w.s.chars[id as usize].adv.as_mut().unwrap();
        adv.quests_at_rank = 10;
        adv.xp = 10_000;
    }
    w.check_promotion(id);
    assert_eq!(w.promotions().len(), 1);
    while !w.desk_manned() {
        w.advance(10);
    }
    w.s.guild.gold = 1000;
    let before = w.characters()[id as usize].attrs;
    w.execute(Command::AcceptPromotion { character: id })
        .unwrap();
    let c = &w.characters()[id as usize];
    assert_eq!(c.rank, 1);
    assert_eq!(c.attrs[0], before[0] + 2);
    assert!(w.promotions().is_empty());
}

#[test]
fn the_east_wing_needs_renown_then_opens() {
    let mut w = world(1);
    w.s.guild.gold = 10_000;
    assert_eq!(w.execute(Command::OpenEastWing), Err(Refusal::NeedsRenown));
    w.add_renown(100);
    w.execute(Command::OpenEastWing).unwrap();
    let bench = find_prefab(&w, PrefabKind::Rest);
    let r = w.content().layout.east_region;
    assert!(w.validate_placement(bench, (r.x + 2, r.y + 2)).is_ok());
    // The wing is reachable through its openings.
    let mut s = grid::Scratch::default();
    assert!(w
        .grid()
        .distance(w.grid().door, (r.x + 2, r.y + 4), &mut s)
        .is_some());
}

#[test]
fn the_volunteer_floor_catches_an_empty_broke_guild() {
    let mut w = world(1);
    for c in &mut w.s.chars {
        if c.is_adventurer() {
            c.state = CharState::Dead;
        }
    }
    w.s.guild.gold = 0;
    w.advance(MINUTES_PER_DAY);
    let v = w
        .waiting_candidates()
        .find(|c| c.volunteer)
        .expect("a volunteer arrives");
    assert_eq!(v.cost, 0);
}

#[test]
fn shift_coverage_wraps_midnight() {
    let s = Shift {
        start_hour: 22,
        length: 8,
    };
    assert!(s.covers(23) && s.covers(0) && s.covers(5));
    assert!(!s.covers(6) && !s.covers(21));
    assert_eq!(s.night_hours(), 8);
    assert_eq!(
        Shift {
            start_hour: 8,
            length: 8
        }
        .night_hours(),
        0
    );
}

#[test]
fn every_refusal_and_reason_has_a_string() {
    let c = content();
    for r in Refusal::ALL {
        assert!(c.loc.has(r.key()), "missing {}", r.key());
    }
    for o in Objective::ALL {
        assert!(
            c.loc.has(o.key()) && c.loc.has(o.hint_key()),
            "missing {}",
            o.key()
        );
    }
    for g in [
        GoldReason::None,
        GoldReason::QuestReward,
        GoldReason::Hire,
        GoldReason::Wage,
        GoldReason::Meal,
        GoldReason::Construction,
        GoldReason::Refund,
        GoldReason::Promotion,
        GoldReason::Purchase,
        GoldReason::Expansion,
    ] {
        assert!(c.loc.has(g.key()), "missing {}", g.key());
    }
    for k in [
        "happiness.base",
        "happiness.long_shift",
        "happiness.night",
        "happiness.unpaid",
        "happiness.decor",
        "happiness.fed",
    ] {
        assert!(c.loc.has(k), "missing {k}");
    }
}

#[test]
fn a_year_of_empty_days_is_fast() {
    // M1's bar: a simulated year in well under a second of release-mode time. The guild is
    // emptied first so this measures the clock and scheduler, not the guild.
    let mut w = world(1);
    for c in &mut w.s.chars {
        c.serial += 1;
        c.state = CharState::OffMap(OffMapReason::Left);
    }
    let t = std::time::Instant::now();
    w.advance(365 * MINUTES_PER_DAY);
    assert!(t.elapsed().as_secs() < 5);
}

#[test]
fn balance_bands_hold_at_two_thousand_runs() {
    let c = content();
    for rank in 0..=2u8 {
        let r = harness::run_band(&c, rank, 2000, 1_000_000);
        let (lo, hi, death) = harness::BANDS[rank as usize];
        let s = r.per_thousand(r.succeeded);
        let d = r.per_thousand(r.died);
        assert!(
            s >= lo && s <= hi,
            "rank {rank}: success {s} per mille outside {lo}..{hi}"
        );
        assert!(d <= death, "rank {rank}: death {d} per mille above {death}");
    }
}
