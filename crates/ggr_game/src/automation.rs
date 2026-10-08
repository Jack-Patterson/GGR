//! Command-line automation, for screenshots and unattended checks:
//!
//! ```text
//! guildmasters_seat [--seed N] [--new-game] [--autoplay] [--speed S]
//!                   [--screenshot PATH --after-minutes M [--scene hud|build|sheet|staff|quest|menu|options|howto]]
//!                   [--no-autosave] [--exit-after-minutes M]
//! ```
//!
//! `--autoplay` plays a scripted opening through the ordinary command surface (hire, build a
//! canteen, hire a cook, set a shift, equip loot) so a screenshot shows a living guild.

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use ggr_content::PrefabKind;
use ggr_sim::{CharState, Command};

use crate::state::*;

#[derive(Resource, Debug, Clone, Default)]
pub struct Args {
    pub seed: Option<u64>,
    pub new_game: bool,
    pub autoplay: bool,
    pub speed: Option<u32>,
    pub screenshot: Option<String>,
    pub after_minutes: i64,
    pub scene: String,
    pub no_autosave: bool,
    pub exit_after: Option<i64>,
    pub selftest_save: bool,
}

impl Args {
    pub fn parse() -> Args {
        let a: Vec<String> = std::env::args().collect();
        let val = |n: &str| {
            a.iter()
                .position(|x| x == n)
                .and_then(|i| a.get(i + 1).cloned())
        };
        let has = |n: &str| a.iter().any(|x| x == n);
        Args {
            seed: val("--seed").and_then(|v| v.parse().ok()),
            new_game: (has("--new-game")
                || has("--autoplay")
                || has("--screenshot")
                || has("--selftest-save"))
                && val("--scene").as_deref() != Some("main"),
            autoplay: has("--autoplay"),
            speed: val("--speed").and_then(|v| v.parse().ok()),
            screenshot: val("--screenshot"),
            after_minutes: val("--after-minutes")
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            scene: val("--scene").unwrap_or_else(|| "hud".into()),
            no_autosave: has("--no-autosave") || has("--screenshot"),
            exit_after: val("--exit-after-minutes").and_then(|v| v.parse().ok()),
            selftest_save: has("--selftest-save"),
        }
    }
}

#[derive(Default)]
pub struct Driver {
    started_at: Option<i64>,
    canteen: bool,
    cook: bool,
    shift: bool,
    stage: u32,
    frames: u32,
}

#[allow(clippy::too_many_arguments)]
pub fn drive(
    mut commands: Commands,
    args: Res<Args>,
    mut session: ResMut<Session>,
    mut ui: ResMut<UiState>,
    mut pace: ResMut<Pace>,
    mut local: Local<Driver>,
    content: Res<ContentRes>,
    mut exit: MessageWriter<AppExit>,
) {
    if session.mode != Mode::Playing {
        // The main menu has no clock to wait on: shoot after a short warm-up.
        if let (Some(path), "main") = (args.screenshot.clone(), args.scene.as_str()) {
            local.frames += 1;
            if local.frames == 90 {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            } else if local.frames > 130 {
                exit.write(AppExit::Success);
            }
        }
        return;
    }
    let Some(world) = session.world.as_mut() else {
        return;
    };
    let start = *local.started_at.get_or_insert(world.minute());
    let elapsed = world.minute() - start;
    if args.autoplay {
        // Hire whoever the guild can afford, the moment the desk can sign.
        let waiting: Vec<(u32, i64, bool)> = world
            .waiting_candidates()
            .map(|c| (c.id, c.cost, c.staff_role.is_some()))
            .collect();
        for (id, cost, staff) in waiting {
            let roster = world
                .characters()
                .iter()
                .filter(|c| c.on_roster() && c.is_adventurer())
                .count();
            if cost <= world.guild().gold - 150 && (staff || roster < 8) {
                let _ = world.execute(Command::Hire { candidate: id });
            }
        }
        if !local.canteen && world.guild().gold >= 200 {
            let p = content
                .0
                .prefabs
                .iter()
                .position(|p| p.kind == PrefabKind::Canteen)
                .unwrap();
            'find: for y in 3..20 {
                for x in 14..25 {
                    if world.execute(Command::Place { prefab: p, x, y }).is_ok() {
                        local.canteen = true;
                        break 'find;
                    }
                }
            }
        }
        if !local.cook && local.canteen {
            // A cook from the next staff applicant, or a scripted one if none comes soon.
            let cook_role = content
                .0
                .role_for_station(PrefabKind::Canteen)
                .map(|r| r as u8);
            local.cook = world
                .characters()
                .iter()
                .any(|c| c.on_roster() && c.staff.as_ref().map(|s| s.role) == cook_role);
        }
        if !local.shift && elapsed > 60 {
            if let Some(clerk) = world
                .characters()
                .iter()
                .find(|c| c.is_staff())
                .map(|c| c.id)
            {
                local.shift = world
                    .execute(Command::SetShift {
                        character: clerk,
                        start_hour: 7,
                        length: 10,
                    })
                    .is_ok();
            }
        }
        // Equip anything in the stash on whoever is home.
        for item in 0..content.0.items.len() {
            if world.guild().stash[item] > 0 {
                if let Some(c) = world
                    .characters()
                    .iter()
                    .find(|c| c.is_adventurer() && c.on_map() && c.state != CharState::Dead)
                    .map(|c| c.id)
                {
                    let _ = world.execute(Command::Equip { character: c, item });
                }
            }
        }
    }
    if let Some(s) = args.speed {
        if local.stage == 0 {
            pace.speed = s;
        }
    }
    if args.selftest_save && elapsed >= args.after_minutes && local.stage == 0 {
        // Save through the game's own path (camera section and all), load it back, and
        // compare: the same check a player's save makes, without a mouse.
        let path = std::env::temp_dir().join(format!("ggr-selftest-{}.json", std::process::id()));
        let mut rig = crate::camera::CameraRig {
            distance: 22.5,
            ..Default::default()
        };
        let mut before = 0;
        let ok = crate::persist::save_to(&path, &mut session, &rig, "selftest").and_then(|_| {
            // The camera rides in the save as a section of its own, so hash after saving.
            before = session.world.as_ref().map_or(0, |w| w.state_hash());
            // Move the camera after saving, so restoring it is actually tested.
            rig.distance = 40.0;
            crate::persist::load_from(&path, &content, &mut session, &mut rig)
        });
        let after = session.world.as_ref().map(|w| w.state_hash());
        match ok {
            Ok(()) if after == Some(before) && (rig.distance - 22.5).abs() < 1e-3 => {
                println!("[selftest] PASS save/load through the game reproduces state hash {before:016x} and the camera");
            }
            Ok(()) => println!(
                "[selftest] FAIL hash {before:016x} -> {after:?}, camera {}",
                rig.distance
            ),
            Err(e) => println!("[selftest] FAIL {e}"),
        }
        let _ = std::fs::remove_file(&path);
        local.stage = 9;
        exit.write(AppExit::Success);
        return;
    }
    if let Some(m) = args.exit_after {
        if elapsed >= m && args.screenshot.is_none() {
            exit.write(AppExit::Success);
        }
    }
    let Some(path) = args.screenshot.clone() else {
        return;
    };
    match local.stage {
        0 => {
            if args.speed.is_none() {
                pace.speed = 16;
            }
            if elapsed >= args.after_minutes {
                pace.paused = true;
                local.stage = 1;
                arrange(&args.scene, world, &mut ui);
            }
        }
        1 => {
            local.frames += 1;
            if local.frames > 20 {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
                local.stage = 2;
                local.frames = 0;
            }
        }
        _ => {
            local.frames += 1;
            if local.frames > 30 {
                exit.write(AppExit::Success);
            }
        }
    }
}

/// Puts the UI into the state a screenshot scene asks for.
fn arrange(scene: &str, world: &ggr_sim::World, ui: &mut UiState) {
    let first_adv = world
        .characters()
        .iter()
        .filter(|c| c.is_adventurer() && c.on_roster())
        .max_by_key(|c| c.adv.as_ref().map_or(0, |a| a.xp))
        .map(|c| c.id);
    match scene {
        "build" => {
            let canteen = world
                .content()
                .prefabs
                .iter()
                .position(|p| p.kind == PrefabKind::Infirmary);
            ui.build = Some(BuildState {
                prefab: canteen,
                demolish: false,
            });
            ui.hover_cell = Some((17, 21));
        }
        "sheet" => {
            if let Some(id) = first_adv {
                ui.selected_character = Some(id);
                ui.open_sheet(Sheet::Character(id));
            }
        }
        "staff" => {
            if let Some(id) = world
                .characters()
                .iter()
                .find(|c| c.is_staff() && c.on_roster())
                .map(|c| c.id)
            {
                ui.roster_tab = RosterTab::Staff;
                ui.selected_character = Some(id);
                ui.open_sheet(Sheet::Character(id));
            }
        }
        "quest" => {
            let q = world
                .quests()
                .iter()
                .rev()
                .find(|q| q.outcome().is_some() && !q.report.is_empty())
                .or_else(|| world.quests().iter().find(|q| q.is_active()))
                .map(|q| q.id);
            if let Some(q) = q {
                ui.open_sheet(Sheet::Quest(q));
            }
        }
        "candidate" => {
            if let Some(c) = world.waiting_candidates().next() {
                ui.open_sheet(Sheet::Candidate(c.id));
            }
        }
        "menu" => ui.menu = Some(MenuPage::Pause),
        "options" => ui.menu = Some(MenuPage::Options),
        "howto" => ui.menu = Some(MenuPage::HowTo),
        "chronicle" => ui.right_tab = RightTab::Chronicle,
        "outfitter" => ui.right_tab = RightTab::Outfitter,
        _ => {
            if let Some(id) = first_adv {
                ui.selected_character = Some(id);
            }
        }
    }
}
