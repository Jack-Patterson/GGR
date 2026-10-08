//! The camera rig: a target on the floor, a distance, a yaw and a fixed pitch. Pan, orbit and
//! zoom, clamped to the hall. The 3D view renders only into the hole the HUD leaves, so nothing
//! of the hall hides under a panel.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Viewport};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;
use bevy::window::PrimaryWindow;

use crate::input::{pressed, Action};
use crate::persist::Settings;
use crate::state::*;

#[derive(Component)]
pub struct MainCamera;

#[derive(Resource, Debug, Clone)]
pub struct CameraRig {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Panel sizes in logical pixels (left, right, top, bottom), written by the HUD.
    pub insets: [f32; 4],
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            target: Vec3::new(14.0, 0.0, 14.0),
            distance: 30.0,
            yaw: 0.0,
            pitch: 0.95,
            insets: [0.0; 4],
        }
    }
}

pub fn spawn_camera(
    mut commands: Commands,
    mut egui_settings: ResMut<bevy_egui::EguiGlobalSettings>,
) {
    commands.spawn((Camera3d::default(), Transform::default(), MainCamera));
    // The HUD gets a camera of its own, full-window and drawn last, so the 3D camera's
    // viewport can shrink to the hole between the panels without dragging the HUD with it.
    egui_settings.auto_create_primary_context = false;
    commands.spawn((
        bevy_egui::PrimaryEguiContext,
        Camera2d,
        RenderLayers::none(),
        Camera {
            order: 1,
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::ALPHA_BLENDING),
                clear_color: ClearColorConfig::None,
            },
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
    ));
}

#[allow(clippy::too_many_arguments)]
pub fn drive_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    settings: Res<Settings>,
    mut rig: ResMut<CameraRig>,
    ui: Res<UiState>,
    session: Res<Session>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cam: Single<(&mut Camera, &mut Transform), With<MainCamera>>,
    egui_input: Option<Res<bevy_egui::input::EguiWantsInput>>,
) {
    let dt = time.delta_secs();
    let playing = session.mode == Mode::Playing;
    let egui_kb = egui_input
        .as_ref()
        .is_some_and(|w| w.wants_any_keyboard_input());
    let egui_ptr = egui_input
        .as_ref()
        .is_some_and(|w| w.wants_any_pointer_input());
    if playing && !ui.modal_open() && !egui_kb {
        let speed = 6.0 + rig.distance * 0.6;
        let fwd = Vec3::new(-rig.yaw.sin(), 0.0, -rig.yaw.cos());
        let right = Vec3::new(-fwd.z, 0.0, fwd.x);
        let mut mv = Vec3::ZERO;
        if pressed(&keys, &settings, Action::PanUp) {
            mv += fwd;
        }
        if pressed(&keys, &settings, Action::PanDown) {
            mv -= fwd;
        }
        if pressed(&keys, &settings, Action::PanRight) {
            mv += right;
        }
        if pressed(&keys, &settings, Action::PanLeft) {
            mv -= right;
        }
        rig.target += mv.normalize_or_zero() * speed * dt;
        if pressed(&keys, &settings, Action::RotateLeft) {
            rig.yaw += 1.6 * dt;
        }
        if pressed(&keys, &settings, Action::RotateRight) {
            rig.yaw -= 1.6 * dt;
        }
        if pressed(&keys, &settings, Action::ZoomIn) {
            rig.distance -= 20.0 * dt;
        }
        if pressed(&keys, &settings, Action::ZoomOut) {
            rig.distance += 20.0 * dt;
        }
    }
    if playing && !ui.modal_open() && !egui_ptr {
        if scroll.delta.y != 0.0 {
            rig.distance *= 1.0 - scroll.delta.y.clamp(-3.0, 3.0) * 0.08;
        }
        if mouse.pressed(MouseButton::Middle) || mouse.pressed(MouseButton::Right) {
            rig.yaw -= motion.delta.x * 0.005;
            rig.pitch = (rig.pitch + motion.delta.y * 0.003).clamp(0.45, 1.35);
        }
    }
    if !playing {
        // Attract mode on the main menu: a slow turn around the hall.
        rig.yaw += 0.04 * dt;
    }
    rig.distance = rig.distance.clamp(8.0, 60.0);
    rig.target.x = rig.target.x.clamp(0.0, 40.0);
    rig.target.z = rig.target.z.clamp(0.0, 28.0);

    let (camera, tf) = &mut *cam;
    let offset = Vec3::new(
        rig.yaw.sin() * rig.pitch.cos(),
        rig.pitch.sin(),
        rig.yaw.cos() * rig.pitch.cos(),
    ) * rig.distance;
    **tf = Transform::from_translation(rig.target + offset).looking_at(rig.target, Vec3::Y);

    // Render into the hole between the panels.
    let scale = window.scale_factor();
    let [l, r, t, b] = if playing { rig.insets } else { [0.0; 4] };
    let w = window.physical_width();
    let h = window.physical_height();
    let x = (l * scale) as u32;
    let y = (t * scale) as u32;
    let vw = w.saturating_sub(x + (r * scale) as u32).max(1);
    let vh = h.saturating_sub(y + (b * scale) as u32).max(1);
    camera.viewport = Some(Viewport {
        physical_position: UVec2::new(x.min(w.saturating_sub(1)), y.min(h.saturating_sub(1))),
        physical_size: UVec2::new(vw, vh),
        ..default()
    });
}
