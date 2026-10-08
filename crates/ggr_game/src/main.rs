//! Guildmaster's Seat — the composition root.
//!
//! This crate pumps and presents; the sim decides. It owns the window, the 3D grey-box hall
//! and its capsules, the egui HUD, input, audio, settings and save files. Every game decision
//! is a `ggr_sim::Command`; every reaction is to a `ggr_sim::SimEvent`.

// Bevy systems take their dependencies as parameters and queries as types; both lints fight
// the engine's idiom rather than catch anything here.
#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod audio;
mod automation;
mod build_mode;
mod camera;
mod hud;
mod input;
mod menus;
mod persist;
mod sheet;
mod state;
mod theme;
mod view3d;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

use crate::state::*;

fn main() {
    let args = automation::Args::parse();
    let settings = persist::load_settings();
    let content = match ggr_content::Content::load_embedded() {
        Ok(c) => std::sync::Arc::new(c),
        Err(e) => {
            // Content is embedded and validated in tests; failing here means a broken build.
            eprintln!("Guildmaster's Seat could not load its content: {e}");
            std::process::exit(2);
        }
    };
    println!(
        "Guildmaster's Seat {} starting (content {:016x})",
        ggr_core::BUILD_VERSION,
        content.fingerprint
    );

    let (w, h) = settings.resolution;
    App::new()
        .insert_resource(ClearColor(theme::bevy_color(theme::PAGE)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Guildmaster's Seat — Demo".into(),
                resolution: WindowResolution::new(w, h),
                present_mode: PresentMode::AutoVsync,
                mode: persist::window_mode(settings.fullscreen),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .insert_resource(ContentRes(content))
        .insert_resource(settings)
        .insert_resource(args)
        .init_resource::<Session>()
        .init_resource::<UiState>()
        .init_resource::<Pace>()
        .init_resource::<Notices>()
        .init_resource::<view3d::ViewIndex>()
        .init_resource::<camera::CameraRig>()
        .add_message::<SimEventMsg>()
        .add_systems(
            Startup,
            (
                view3d::setup_scene,
                camera::spawn_camera,
                audio::setup,
                menus::boot,
            ),
        )
        .add_systems(
            Update,
            (
                input::gameplay_input,
                state::advance_sim,
                state::dispatch_events,
                audio::play_events,
                persist::autosave,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                view3d::sync_hall,
                view3d::sync_instances,
                view3d::sync_characters,
                view3d::sync_candidates,
                build_mode::ghost,
                camera::drive_camera,
                input::pick,
                automation::drive,
            )
                .after(state::dispatch_events),
        )
        .add_systems(
            EguiPrimaryContextPass,
            (theme::apply_once, hud::draw).chain(),
        )
        .run();
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// Every literal key passed to `t(...)`/`f(...)` in the game's source resolves in the
    /// English table, as do the keys built at runtime (actions, how-to sections, renown tiers).
    /// The interface's half of V2's GG0301: no user-facing string escapes the registry.
    #[test]
    fn every_interface_key_resolves() {
        let content = ggr_content::Content::load_embedded().unwrap();
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let text = std::fs::read_to_string(&path).unwrap();
            for pat in [".t(\"", ".f(\""] {
                for (i, _) in text.match_indices(pat) {
                    let rest = &text[i + pat.len()..];
                    let key: String = rest.chars().take_while(|c| *c != '"').collect();
                    if !content.loc.has(&key) {
                        missing.push(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        for a in crate::input::Action::ALL {
            if !content.loc.has(&a.label_key()) {
                missing.push(a.label_key());
            }
        }
        for i in 1..=9 {
            for part in ["title", "body"] {
                let k = format!("howto.{i}.{part}");
                if !content.loc.has(&k) {
                    missing.push(k);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "missing localisation keys: {missing:#?}"
        );
    }

    #[test]
    fn default_bindings_name_real_keys() {
        for (action, key) in crate::input::default_bindings() {
            assert!(
                crate::input::key_from_name(&key).is_some(),
                "{action} -> {key}"
            );
        }
    }
}
