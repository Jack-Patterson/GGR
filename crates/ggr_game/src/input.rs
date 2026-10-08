//! Input: rebindable gameplay actions, the keyboard map, and click-to-select in the hall.
//! Gameplay keys are ignored while a modal is open or egui has the keyboard, so the gameplay
//! and UI maps are never both live.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::persist::Settings;
use crate::state::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    PanUp,
    PanDown,
    PanLeft,
    PanRight,
    RotateLeft,
    RotateRight,
    ZoomIn,
    ZoomOut,
    Pause,
    Speed1,
    Speed2,
    Speed4,
    Speed8,
    Speed16,
    NextCharacter,
    PrevCharacter,
    Inspect,
    Build,
    Objectives,
    Debug,
}

impl Action {
    pub const ALL: [Action; 20] = [
        Action::PanUp,
        Action::PanDown,
        Action::PanLeft,
        Action::PanRight,
        Action::RotateLeft,
        Action::RotateRight,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::Pause,
        Action::Speed1,
        Action::Speed2,
        Action::Speed4,
        Action::Speed8,
        Action::Speed16,
        Action::NextCharacter,
        Action::PrevCharacter,
        Action::Inspect,
        Action::Build,
        Action::Objectives,
        Action::Debug,
    ];

    pub fn token(self) -> &'static str {
        match self {
            Action::PanUp => "pan_up",
            Action::PanDown => "pan_down",
            Action::PanLeft => "pan_left",
            Action::PanRight => "pan_right",
            Action::RotateLeft => "rotate_left",
            Action::RotateRight => "rotate_right",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::Pause => "pause",
            Action::Speed1 => "speed_1",
            Action::Speed2 => "speed_2",
            Action::Speed4 => "speed_4",
            Action::Speed8 => "speed_8",
            Action::Speed16 => "speed_16",
            Action::NextCharacter => "next_character",
            Action::PrevCharacter => "prev_character",
            Action::Inspect => "inspect",
            Action::Build => "build",
            Action::Objectives => "objectives",
            Action::Debug => "debug",
        }
    }

    pub fn label_key(self) -> String {
        format!("action.{}", self.token())
    }

    fn default_key(self) -> &'static str {
        match self {
            Action::PanUp => "W",
            Action::PanDown => "S",
            Action::PanLeft => "A",
            Action::PanRight => "D",
            Action::RotateLeft => "Z",
            Action::RotateRight => "X",
            Action::ZoomIn => "R",
            Action::ZoomOut => "F",
            Action::Pause => "Space",
            Action::Speed1 => "1",
            Action::Speed2 => "2",
            Action::Speed4 => "3",
            Action::Speed8 => "4",
            Action::Speed16 => "5",
            Action::NextCharacter => "E",
            Action::PrevCharacter => "Q",
            Action::Inspect => "Enter",
            Action::Build => "B",
            Action::Objectives => "O",
            Action::Debug => "F3",
        }
    }
}

pub fn default_bindings() -> BTreeMap<String, String> {
    Action::ALL
        .iter()
        .map(|a| (a.token().to_string(), a.default_key().to_string()))
        .collect()
}

/// The keys a gameplay action may be bound to, by display name.
pub const KEYS: &[(&str, KeyCode)] = &[
    ("A", KeyCode::KeyA),
    ("B", KeyCode::KeyB),
    ("C", KeyCode::KeyC),
    ("D", KeyCode::KeyD),
    ("E", KeyCode::KeyE),
    ("F", KeyCode::KeyF),
    ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH),
    ("I", KeyCode::KeyI),
    ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK),
    ("L", KeyCode::KeyL),
    ("M", KeyCode::KeyM),
    ("N", KeyCode::KeyN),
    ("O", KeyCode::KeyO),
    ("P", KeyCode::KeyP),
    ("Q", KeyCode::KeyQ),
    ("R", KeyCode::KeyR),
    ("S", KeyCode::KeyS),
    ("T", KeyCode::KeyT),
    ("U", KeyCode::KeyU),
    ("V", KeyCode::KeyV),
    ("W", KeyCode::KeyW),
    ("X", KeyCode::KeyX),
    ("Y", KeyCode::KeyY),
    ("Z", KeyCode::KeyZ),
    ("0", KeyCode::Digit0),
    ("1", KeyCode::Digit1),
    ("2", KeyCode::Digit2),
    ("3", KeyCode::Digit3),
    ("4", KeyCode::Digit4),
    ("5", KeyCode::Digit5),
    ("6", KeyCode::Digit6),
    ("7", KeyCode::Digit7),
    ("8", KeyCode::Digit8),
    ("9", KeyCode::Digit9),
    ("Space", KeyCode::Space),
    ("Enter", KeyCode::Enter),
    ("Tab", KeyCode::Tab),
    ("Up", KeyCode::ArrowUp),
    ("Down", KeyCode::ArrowDown),
    ("Left", KeyCode::ArrowLeft),
    ("Right", KeyCode::ArrowRight),
    ("PageUp", KeyCode::PageUp),
    ("PageDown", KeyCode::PageDown),
    ("Home", KeyCode::Home),
    ("End", KeyCode::End),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F4", KeyCode::F4),
    ("F5", KeyCode::F5),
    ("F6", KeyCode::F6),
    ("F7", KeyCode::F7),
    ("F8", KeyCode::F8),
    ("Comma", KeyCode::Comma),
    ("Period", KeyCode::Period),
    ("Minus", KeyCode::Minus),
    ("Equal", KeyCode::Equal),
];

pub fn key_from_name(name: &str) -> Option<KeyCode> {
    KEYS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

pub fn key_name(code: KeyCode) -> Option<&'static str> {
    KEYS.iter().find(|(_, k)| *k == code).map(|(n, _)| *n)
}

pub fn pressed(keys: &ButtonInput<KeyCode>, s: &Settings, a: Action) -> bool {
    s.key_for(a).is_some_and(|k| keys.pressed(k))
}

pub fn just_pressed(keys: &ButtonInput<KeyCode>, s: &Settings, a: Action) -> bool {
    s.key_for(a).is_some_and(|k| keys.just_pressed(k))
}

/// Keyboard: Escape always (back, then close, then the pause menu); gameplay actions only
/// with no modal open and egui not holding the keyboard.
#[allow(clippy::too_many_arguments)]
pub fn gameplay_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<Settings>,
    mut ui: ResMut<UiState>,
    mut pace: ResMut<Pace>,
    session: Res<Session>,
    egui_kb: Option<Res<bevy_egui::input::EguiWantsInput>>,
) {
    if session.mode != Mode::Playing {
        return;
    }
    // Rebinding captures the next key press, whatever it is.
    if let Some(action) = ui.rebinding {
        if let Some(k) = keys.get_just_pressed().next() {
            if *k != KeyCode::Escape {
                if let Some(name) = key_name(*k) {
                    settings.bindings.insert(action.token().into(), name.into());
                    crate::persist::save_settings(&settings);
                }
            }
            ui.rebinding = None;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if ui.goal_dialog {
            ui.goal_dialog = false;
        } else if ui.sheet.len() > 1 {
            ui.sheet.pop();
        } else if !ui.sheet.is_empty() {
            ui.sheet.clear();
        } else if let Some(m) = ui.menu {
            ui.menu = match m {
                MenuPage::Pause => None,
                _ => ui.menu_return.take().or(Some(MenuPage::Pause)),
            };
        } else if ui.build.is_some() {
            ui.build = None;
        } else {
            ui.menu = Some(MenuPage::Pause);
        }
        return;
    }
    let egui_has_keys = egui_kb.is_some_and(|w| w.wants_any_keyboard_input());
    if ui.modal_open() || egui_has_keys {
        return;
    }
    let s = &*settings;
    if just_pressed(&keys, s, Action::Pause) {
        pace.paused = !pace.paused;
    }
    for (a, speed) in [
        (Action::Speed1, 1),
        (Action::Speed2, 2),
        (Action::Speed4, 4),
        (Action::Speed8, 8),
        (Action::Speed16, 16),
    ] {
        if just_pressed(&keys, s, a) {
            pace.speed = speed;
            pace.paused = false;
        }
    }
    if just_pressed(&keys, s, Action::Build) {
        ui.build = match ui.build {
            Some(_) => None,
            None => Some(BuildState::default()),
        };
    }
    if just_pressed(&keys, s, Action::Debug) {
        ui.debug = !ui.debug;
    }
    if just_pressed(&keys, s, Action::Objectives) {
        ui.objectives_open = !ui.objectives_open;
    }
    let Some(world) = session.world.as_ref() else {
        return;
    };
    let next = just_pressed(&keys, s, Action::NextCharacter);
    let prev = just_pressed(&keys, s, Action::PrevCharacter);
    if next || prev {
        let ids: Vec<u32> = world
            .characters()
            .iter()
            .filter(|c| c.on_roster())
            .map(|c| c.id)
            .collect();
        if !ids.is_empty() {
            let pos = ui
                .selected_character
                .and_then(|s| ids.iter().position(|i| *i == s));
            let n = ids.len();
            let i = match (pos, next) {
                (Some(p), true) => (p + 1) % n,
                (Some(p), false) => (p + n - 1) % n,
                (None, _) => 0,
            };
            ui.selected_character = Some(ids[i]);
            ui.selected_instance = None;
        }
    }
    if just_pressed(&keys, s, Action::Inspect) {
        if let Some(c) = ui.selected_character {
            ui.open_sheet(Sheet::Character(c));
        } else if let Some(i) = ui.selected_instance {
            ui.open_sheet(Sheet::Instance(i));
        } else if let Some(c) = world.waiting_candidates().next() {
            ui.open_sheet(Sheet::Candidate(c.id));
        }
    }
}

/// The cell under the cursor, by casting onto the floor plane.
pub fn cursor_cell(
    window: &Window,
    camera: &Camera,
    cam_tf: &GlobalTransform,
) -> Option<((i32, i32), Vec3)> {
    let cursor = window.cursor_position()?;
    let ray = camera.viewport_to_world(cam_tf, cursor).ok()?;
    let p = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
    Some(((p.x.floor() as i32, p.z.floor() as i32), p))
}

/// Left click in the hall selects the nearest character, else the interactable under the
/// cursor. In build mode clicks belong to the build tool instead.
#[allow(clippy::too_many_arguments)]
pub fn pick(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::MainCamera>>,
    mut ui: ResMut<UiState>,
    session: Res<Session>,
    chars: Query<(&crate::view3d::CharView, &Transform, &Visibility)>,
    egui_pointer: Option<Res<bevy_egui::input::EguiWantsInput>>,
) {
    if session.mode != Mode::Playing {
        return;
    }
    let (cam, tf) = *camera;
    let hit = cursor_cell(&window, cam, tf);
    ui.hover_cell = hit.map(|h| h.0);
    if egui_pointer.is_some_and(|w| w.wants_any_pointer_input()) || ui.modal_open() {
        return;
    }
    if ui.build.is_some() || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(((cx, cy), p)) = hit else { return };
    let mut best: Option<(f32, u32)> = None;
    for (cv, t, vis) in &chars {
        if *vis == Visibility::Hidden {
            continue;
        }
        let d = Vec2::new(t.translation.x - p.x, t.translation.z - p.z).length();
        if d < 0.7 && best.is_none_or(|b| d < b.0) {
            best = Some((d, cv.id));
        }
    }
    if let Some((_, id)) = best {
        if ui.selected_character == Some(id) {
            ui.open_sheet(Sheet::Character(id));
        }
        ui.selected_character = Some(id);
        ui.selected_instance = None;
        return;
    }
    let Some(world) = session.world.as_ref() else {
        return;
    };
    if let Some(inst) = world.grid().owner(cx, cy) {
        if ui.selected_instance == Some(inst) {
            ui.open_sheet(Sheet::Instance(inst));
        }
        ui.selected_instance = Some(inst);
        ui.selected_character = None;
    } else {
        ui.selected_character = None;
        ui.selected_instance = None;
    }
}
