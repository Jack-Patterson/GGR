use std::path::PathBuf;
use std::sync::Arc;

use ggr_content::Content;
use ggr_sim::World;
use serde_json::json;

use super::*;

fn content() -> Arc<Content> {
    Arc::new(Content::load_embedded().unwrap())
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ggr-save-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn file_round_trip_resimulates_identically() {
    let dir = temp_dir("roundtrip");
    let path = dir.join("slot1.json");
    let mut a = World::new(content(), 21);
    a.advance(3000);
    save_world(&path, &a, "test").unwrap();
    let mut b = load_world(&path, content()).unwrap();
    assert_eq!(a.state_hash(), b.state_hash());
    a.advance(5000);
    b.advance(5000);
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn saves_taken_at_random_ticks_across_a_week_all_resimulate() {
    let dir = temp_dir("week");
    let path = dir.join("slot.json");
    let mut straight = World::new(content(), 99);
    let mut probe = ggr_core::Xoshiro256StarStar::new(5);
    let mut t = 0;
    while t < 7 * 1440 {
        let step = i64::from(probe.next_int(50, 900));
        straight.advance(step);
        t += step;
        save_world(&path, &straight, "probe").unwrap();
        let mut loaded = load_world(&path, content()).unwrap();
        let mut copy = load_world(&path, content()).unwrap();
        loaded.advance(300);
        copy.advance(300);
        assert_eq!(loaded.state_hash(), copy.state_hash());
        // And the loaded world equals the live one at the save point.
        let reloaded = load_world(&path, content()).unwrap();
        assert_eq!(
            reloaded.state_hash(),
            straight.state_hash(),
            "at minute {}",
            straight.minute()
        );
    }
}

#[test]
fn a_crash_mid_write_leaves_the_previous_save_intact() {
    let dir = temp_dir("crash");
    let path = dir.join("slot1.json");
    let mut w = World::new(content(), 3);
    w.advance(500);
    save_world(&path, &w, "good").unwrap();
    let good = w.state_hash();
    // Simulate a kill mid-write: a truncated temporary file next to the slot, never renamed.
    let text = to_json(&envelope_from_world(&w, "half"));
    fs::write(temp_path(&path), &text[..text.len() / 3]).unwrap();
    let loaded = load_world(&path, content()).unwrap();
    assert_eq!(loaded.state_hash(), good);
}

#[test]
fn a_corrupt_slot_falls_back_to_its_backup() {
    let dir = temp_dir("corrupt");
    let path = dir.join("slot1.json");
    let mut w = World::new(content(), 4);
    w.advance(200);
    save_world(&path, &w, "first").unwrap();
    let first = w.state_hash();
    w.advance(200);
    save_world(&path, &w, "second").unwrap();
    fs::write(&path, "{ not json").unwrap();
    let loaded = load_world(&path, content()).unwrap();
    assert_eq!(loaded.state_hash(), first, "loaded the backup");
}

#[test]
fn backups_rotate_and_are_bounded() {
    let dir = temp_dir("rotate");
    let path = dir.join("slot1.json");
    let mut w = World::new(content(), 4);
    for _ in 0..6 {
        w.advance(10);
        save_world(&path, &w, "x").unwrap();
    }
    for n in 1..=BACKUPS {
        assert!(backup_path(&path, n).exists(), "backup {n} missing");
    }
    assert!(!backup_path(&path, BACKUPS + 1).exists());
}

#[test]
fn an_unknown_section_rides_through_load_and_save() {
    let w = World::new(content(), 1);
    let mut env = envelope_from_world(&w, "x");
    env.sections.insert(
        "mod.fishing".into(),
        SectionEnvelope {
            version: 7,
            data: json!({"catch": ["trout", "pike"]}),
        },
    );
    let w2 = world_from_envelope(content(), env.clone(), &migrations()).unwrap();
    let env2 = envelope_from_world(&w2, "x");
    assert_eq!(
        env2.sections.get("mod.fishing"),
        env.sections.get("mod.fishing")
    );
}

fn add_field(mut v: Value) -> Result<Value, GameError> {
    v["added_in_v2"] = json!(true);
    Ok(v)
}

fn remove_field(mut v: Value) -> Result<Value, GameError> {
    v.as_object_mut().unwrap().remove("added_in_v2");
    Ok(v)
}

#[test]
fn migrations_chain_one_step_at_a_time() {
    // A synthetic history: "stats" was once v0 and went v0 -> v1 by way of a field that came
    // and went. The chain runs both steps, in order, and the result loads.
    let w = World::new(content(), 1);
    let mut env = envelope_from_world(&w, "x");
    env.sections.get_mut("stats").unwrap().version = 0;
    let registry = vec![Migration {
        section: "stats",
        from: 0,
        apply: |v| add_field(v).and_then(remove_field),
    }];
    migrate(&mut env, &registry).unwrap();
    assert_eq!(env.sections["stats"].version, 1);
    world_from_envelope(content(), env, &registry).unwrap();
}

#[test]
fn a_missing_migration_is_an_error_not_a_guess() {
    let w = World::new(content(), 1);
    let mut env = envelope_from_world(&w, "x");
    env.sections.get_mut("guild").unwrap().version = 0;
    let err = migrate(&mut env, &[]).unwrap_err().to_string();
    assert!(err.contains("no migration"), "{err}");
}

#[test]
fn a_save_from_a_newer_build_is_refused() {
    let w = World::new(content(), 1);
    let mut env = envelope_from_world(&w, "x");
    env.sections.get_mut("guild").unwrap().version = 99;
    assert!(migrate(&mut env, &[]).is_err());
}

/// The committed demo fixture: a guild saved mid-quest and mid-walk on day 3. It must load,
/// and keep running, for as long as the demo's content ids stand.
#[test]
fn the_committed_fixture_loads_and_runs() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("fixtures")
        .join("demo_v1.json");
    let mut w = load_world(&path, content()).expect("fixture loads");
    assert!(w.day() >= 3);
    assert!(
        w.quests().iter().any(|q| q.is_active()),
        "fixture should be mid-quest"
    );
    w.advance(3 * 1440);
    w.verify_consistency().unwrap();
}
