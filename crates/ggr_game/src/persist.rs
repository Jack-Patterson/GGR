//! Files: settings and save slots under the platform's user folders (resolved by the
//! `directories` crate, joined with `Path::join` — never a literal separator), plus the
//! day-boundary autosave.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::{MonitorSelection, WindowMode};
use serde::{Deserialize, Serialize};

use crate::input::{default_bindings, Action};
use crate::state::*;

#[derive(Resource, Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub master_volume: f32,
    pub sfx: bool,
    pub fullscreen: bool,
    pub resolution: (u32, u32),
    pub ui_scale: f32,
    pub language: String,
    /// Gameplay bindings, action name -> key name.
    pub bindings: BTreeMap<String, String>,
    pub seen_intro: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master_volume: 0.6,
            sfx: true,
            fullscreen: false,
            resolution: (1440, 900),
            ui_scale: 1.0,
            language: "English".into(),
            bindings: default_bindings(),
            seen_intro: false,
        }
    }
}

impl Settings {
    pub fn key_for(&self, a: Action) -> Option<KeyCode> {
        self.bindings
            .get(a.token())
            .and_then(|k| crate::input::key_from_name(k))
    }
}

fn dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "Halkyon Studios", "Guildmasters Seat")
}

pub fn data_dir() -> PathBuf {
    dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".").join("saves"))
}

fn settings_path() -> PathBuf {
    dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("settings.json")
}

pub fn slots() -> ggr_save::SaveSlots {
    ggr_save::SaveSlots {
        dir: data_dir().join("saves"),
    }
}

pub fn load_settings() -> Settings {
    let mut s: Settings = std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    // New actions added since the file was written get their defaults.
    for (k, v) in default_bindings() {
        s.bindings.entry(k).or_insert(v);
    }
    s
}

pub fn save_settings(s: &Settings) {
    let text = serde_json::to_string_pretty(s).unwrap_or_default();
    if let Err(e) = ggr_save::write_atomic(&settings_path(), &text) {
        warn!("could not save settings: {e}");
    }
}

pub fn window_mode(fullscreen: bool) -> WindowMode {
    if fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    }
}

/// The camera rig rides in the save as a section the sim does not own.
pub fn stash_camera(world: &mut ggr_sim::World, rig: &crate::camera::CameraRig) {
    world.set_foreign_section(
        "view.camera",
        1,
        serde_json::json!({ "target": [rig.target.x, rig.target.z], "distance": rig.distance, "yaw": rig.yaw }),
    );
}

pub fn restore_camera(world: &ggr_sim::World, rig: &mut crate::camera::CameraRig) {
    if let Some((1, v)) = world.foreign_section("view.camera") {
        if let (Some(x), Some(z)) = (v["target"][0].as_f64(), v["target"][1].as_f64()) {
            rig.target = Vec3::new(x as f32, 0.0, z as f32);
        }
        if let Some(d) = v["distance"].as_f64() {
            rig.distance = d as f32;
        }
        if let Some(y) = v["yaw"].as_f64() {
            rig.yaw = y as f32;
        }
    }
}

pub fn save_to(
    path: &std::path::Path,
    session: &mut Session,
    rig: &crate::camera::CameraRig,
    label: &str,
) -> Result<(), String> {
    let Some(world) = session.world.as_mut() else {
        return Err("no game".into());
    };
    stash_camera(world, rig);
    ggr_save::save_world(path, world, label).map_err(|e| e.to_string())
}

pub fn load_from(
    path: &std::path::Path,
    content: &ContentRes,
    session: &mut Session,
    rig: &mut crate::camera::CameraRig,
) -> Result<(), String> {
    let world = ggr_save::load_world(path, content.0.clone()).map_err(|e| e.to_string())?;
    restore_camera(&world, rig);
    session.replace(world, Mode::Playing);
    Ok(())
}

/// Autosaves at each day boundary.
pub fn autosave(
    mut events: MessageReader<SimEventMsg>,
    mut session: ResMut<Session>,
    rig: Res<crate::camera::CameraRig>,
    args: Res<crate::automation::Args>,
) {
    let mut day = false;
    for e in events.read() {
        if matches!(e.0, ggr_sim::SimEvent::DayStarted { .. }) {
            day = true;
        }
    }
    if day && session.mode == Mode::Playing && !args.no_autosave {
        let path = slots().autosave_path();
        if let Err(e) = save_to(&path, &mut session, &rig, "autosave") {
            warn!("autosave failed: {e}");
        }
    }
}
