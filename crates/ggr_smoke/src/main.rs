//! The headless smoke run: boots the content, simulates N days hands-off with one scripted
//! hire, checks the invariants every day, round-trips a save, and prints one PASS/FAIL line
//! per check. Exit code 0 only if every check passed.
//!
//! ```text
//! ggr_smoke [--days N] [--seed S] [--balance RUNS] [--write-fixture PATH]
//! ```

use std::process::ExitCode;
use std::sync::Arc;

use ggr_content::Content;
use ggr_sim::{harness, CharState, Command, World, MINUTES_PER_DAY};

struct Report {
    failures: u32,
}

impl Report {
    fn check(&mut self, ok: bool, msg: impl AsRef<str>) {
        if ok {
            println!("[smoke] PASS {}", msg.as_ref());
        } else {
            println!("[smoke] FAIL {}", msg.as_ref());
            self.failures += 1;
        }
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let days: i64 = arg(&args, "--days")
        .and_then(|v| v.parse().ok())
        .unwrap_or(7);
    let seed: u64 = arg(&args, "--seed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(42);
    let balance: Option<u32> = arg(&args, "--balance").and_then(|v| v.parse().ok());
    let fixture = arg(&args, "--write-fixture");
    let mut r = Report { failures: 0 };

    r.check(true, format!("build {}", ggr_core::BUILD_VERSION));
    let content = match Content::load_embedded() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            r.check(false, format!("content validates: {e}"));
            return ExitCode::FAILURE;
        }
    };
    r.check(
        true,
        format!(
            "content sealed: {} quests, {} prefabs, {} classes, {} items, {} skills",
            content.quests.len(),
            content.prefabs.len(),
            content.classes.len(),
            content.items.len(),
            content.skills.len()
        ),
    );
    let mut rng = ggr_core::RngStreams::new(42);
    let first_dice = rng.dice().next_u64();
    r.check(
        first_dice == 370_125_584_613_700_256,
        format!("first dice value for seed 42 is {first_dice}, V2's golden is 370125584613700256"),
    );

    let mut w = World::new(content.clone(), seed);
    let start_gold = w.guild().gold;
    r.check(
        start_gold == content.rules.starting_gold,
        format!("the guild opened with {start_gold} gold"),
    );
    let mut hired_at = None;
    for _ in 0..days {
        for _ in 0..MINUTES_PER_DAY {
            w.advance(1);
            if hired_at.is_none() && w.desk_manned() {
                let cand = w
                    .waiting_candidates()
                    .find(|c| c.cost <= w.guild().gold)
                    .map(|c| (c.id, c.cost));
                if let Some((id, cost)) = cand {
                    if w.execute(Command::Hire { candidate: id }).is_ok() {
                        hired_at = Some((id, w.minute(), cost));
                    }
                }
            }
        }
        let day = w.day();
        match w.verify_consistency() {
            Ok(()) => r.check(true, format!("day {day}: slots consistent, audit balanced")),
            Err(e) => r.check(false, format!("day {day}: {e}")),
        }
        r.check(
            !w.scheduler().has_overdue(w.minute()),
            format!("day {day}: no stale scheduled events"),
        );
        r.check(
            w.guild().gold >= 0,
            format!("day {day}: treasury never negative"),
        );
        w.drain_events();
    }
    r.check(
        w.day() == days + 1,
        format!("clock reached day {}", w.day()),
    );
    let s = w.stats().clone();
    r.check(
        s.succeeded + s.failed + s.died >= 1,
        format!(
            "{} quest(s) resolved in {days} day(s): {} posted, {} taken, {} succeeded, {} failed, {} died, {} expired",
            s.succeeded + s.failed + s.died,
            s.posted,
            s.taken,
            s.succeeded,
            s.failed,
            s.died,
            s.expired
        ),
    );
    r.check(
        s.candidates_arrived >= 1,
        format!("{} candidate(s) arrived at the door", s.candidates_arrived),
    );
    match hired_at {
        Some((id, m, cost)) => r.check(
            true,
            format!("hired candidate {id} at minute {m} for {cost} gold"),
        ),
        None => r.check(false, "a scripted hire went through"),
    }
    let dead = w
        .characters()
        .iter()
        .filter(|c| c.state == CharState::Dead)
        .count();
    println!(
        "[smoke] INFO gold {start_gold} -> {}, renown {}, roster {}, dead {dead}, meals {}, wages paid via {} quits",
        w.guild().gold,
        w.guild().renown,
        w.characters().iter().filter(|c| c.on_roster()).count(),
        s.meals,
        s.quits
    );

    // Save, load, and resimulate a day: identical state either way.
    let env = ggr_save::envelope_from_world(&w, "smoke");
    let text = ggr_save::to_json(&env);
    match ggr_save::from_json(&text)
        .and_then(|e| ggr_save::world_from_envelope(content.clone(), e, &ggr_save::migrations()))
    {
        Ok(mut loaded) => {
            r.check(
                loaded.state_hash() == w.state_hash(),
                "save -> load reproduces the state hash",
            );
            let mut a = World::from_sections(content.clone(), &w.to_sections()).unwrap();
            a.advance(MINUTES_PER_DAY);
            loaded.advance(MINUTES_PER_DAY);
            r.check(
                a.state_hash() == loaded.state_hash(),
                "save -> load -> resimulate a day matches never saving",
            );
        }
        Err(e) => r.check(false, format!("save round trip: {e}")),
    }
    println!(
        "[smoke] INFO state hash after {days} day(s) at seed {seed} is {}",
        w.state_hash()
    );

    if let Some(path) = fixture {
        // A guild mid-quest and mid-walk, the fixture the persistence tests load forever after.
        let mut f = World::new(content.clone(), 2024);
        f.advance(2 * MINUTES_PER_DAY + 600);
        while !f.quests().iter().any(|q| q.is_active()) {
            f.advance(10);
        }
        let ok = ggr_save::save_world(std::path::Path::new(&path), &f, "fixture demo_v1").is_ok();
        r.check(ok, format!("wrote fixture {path}"));
    }

    if let Some(runs) = balance {
        for rank in 0..=content.demo_rank_cap as u8 {
            let b = harness::run_band(&content, rank, runs, 1_000_000);
            let (lo, hi, death) = harness::BANDS[rank as usize];
            let sp = b.per_thousand(b.succeeded);
            let dp = b.per_thousand(b.died);
            let letter = &content.ranks[rank as usize].letter;
            r.check(
                (lo..=hi).contains(&sp) && dp <= death,
                format!(
                    "rank {letter}: {runs} runs, success {sp}/1000 (band {lo}-{hi}), death {dp}/1000 (max {death}), injured {}/1000, mean gold {}",
                    b.per_thousand(b.injured),
                    b.gold / i64::from(b.runs.max(1))
                ),
            );
        }
    }

    if let Some(days) = arg(&args, "--play").and_then(|v| v.parse::<i64>().ok()) {
        for seed in 1..=5u64 {
            let mut p = World::new(content.clone(), seed);
            let reached = harness::play_reasonably(&mut p, days);
            let s = p.stats();
            let living = p
                .characters()
                .iter()
                .filter(|c| c.is_adventurer() && c.on_roster())
                .count();
            println!(
                "[smoke] INFO played seed {seed} for {days} days: tiers reached on days {:?}; gold {}; renown {}; living adventurers {living}; {} won {} lost {} died; built {}; promotions {}; meals {}; quits {}",
                reached, p.guild().gold, p.guild().renown, s.succeeded, s.failed, s.died, s.built, s.promotions, s.meals, s.quits
            );
        }
    }

    if r.failures == 0 {
        println!("[smoke] All checks passed.");
        ExitCode::SUCCESS
    } else {
        println!("[smoke] {} check(s) failed.", r.failures);
        ExitCode::FAILURE
    }
}
