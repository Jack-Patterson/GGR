//! Build mode: a ghost of the chosen prefab follows the cursor, coloured by the same
//! validation the sim's command runs, so the preview and the commit can never disagree.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use ggr_sim::{Command, InstanceStatus};

use crate::state::*;
use crate::theme;

fn square(g: &mut Gizmos, c: (i32, i32), color: Color, y: f32, inset: f32) {
    let (x0, z0) = (c.0 as f32 + inset, c.1 as f32 + inset);
    let (x1, z1) = (c.0 as f32 + 1.0 - inset, c.1 as f32 + 1.0 - inset);
    g.line(Vec3::new(x0, y, z0), Vec3::new(x1, y, z0), color);
    g.line(Vec3::new(x1, y, z0), Vec3::new(x1, y, z1), color);
    g.line(Vec3::new(x1, y, z1), Vec3::new(x0, y, z1), color);
    g.line(Vec3::new(x0, y, z1), Vec3::new(x0, y, z0), color);
}

#[allow(clippy::too_many_arguments)]
pub fn ghost(
    mut gizmos: Gizmos,
    mouse: Res<ButtonInput<MouseButton>>,
    mut session: ResMut<Session>,
    mut ui: ResMut<UiState>,
    content: Res<ContentRes>,
    mut notices: ResMut<Notices>,
    time: Res<Time>,
    _window: Single<&Window, With<PrimaryWindow>>,
    egui_input: Option<Res<bevy_egui::input::EguiWantsInput>>,
) {
    if session.mode != Mode::Playing {
        return;
    }
    let Some(build) = ui.build else {
        // Outside build mode, outline the selection.
        if let (Some(world), Some(id)) = (session.world.as_ref(), ui.selected_instance) {
            if let Some(inst) = world.instances().get(id as usize) {
                if inst.status != InstanceStatus::Demolished {
                    let p = &world.content().prefabs[inst.prefab as usize];
                    for (dx, dy) in &p.footprint {
                        square(
                            &mut gizmos,
                            (inst.origin.0 + dx, inst.origin.1 + dy),
                            theme::bevy_color(theme::ACCENT),
                            0.03,
                            0.04,
                        );
                    }
                }
            }
        }
        return;
    };
    let Some(world) = session.world.as_mut() else {
        return;
    };
    // The build grid over every buildable cell.
    let g = world.grid();
    let grid_color = Color::srgba(0.13, 0.12, 0.1, 0.12);
    for x in 1..g.width {
        g_line(
            &mut gizmos,
            Vec3::new(x as f32, 0.02, 1.0),
            Vec3::new(x as f32, 0.02, (g.height - 1) as f32),
            grid_color,
        );
    }
    for y in 1..g.height {
        g_line(
            &mut gizmos,
            Vec3::new(1.0, 0.02, y as f32),
            Vec3::new((g.width - 1) as f32, 0.02, y as f32),
            grid_color,
        );
    }
    let Some(cell) = ui.hover_cell else { return };
    let over_ui = egui_input.is_some_and(|w| w.wants_any_pointer_input());
    let click = mouse.just_pressed(MouseButton::Left) && !over_ui && !ui.modal_open();
    if mouse.just_pressed(MouseButton::Right) && !over_ui {
        ui.build = Some(BuildState::default());
        return;
    }
    if build.demolish {
        if let Some(inst) = world.grid().owner(cell.0, cell.1) {
            let i = &world.instances()[inst as usize];
            let p = &world.content().prefabs[i.prefab as usize];
            for (dx, dy) in &p.footprint {
                square(
                    &mut gizmos,
                    (i.origin.0 + dx, i.origin.1 + dy),
                    theme::bevy_color(theme::ACCENT),
                    0.04,
                    0.02,
                );
            }
            if click {
                match world.execute(Command::Demolish { instance: inst }) {
                    Ok(_) => {}
                    Err(r) => notices.push(
                        world.minute(),
                        content.t(r.key()).to_string(),
                        Tone::Alert,
                        time.elapsed_secs_f64(),
                    ),
                }
            }
        }
        return;
    }
    let Some(prefab) = build.prefab else { return };
    let verdict = world.validate_placement(prefab, cell);
    ui.ghost_ok = Some(verdict);
    let color = if verdict.is_ok() {
        theme::bevy_color(theme::GAIN)
    } else {
        theme::bevy_color(theme::ACCENT)
    };
    let p = world.content().prefabs[prefab].clone();
    for (dx, dy) in &p.footprint {
        let c = (cell.0 + dx, cell.1 + dy);
        square(&mut gizmos, c, color, 0.04, 0.03);
        square(&mut gizmos, c, color, p.height.min(1.5), 0.1);
    }
    for (dx, dy) in p.slots.iter().chain(p.staff_slot.iter()) {
        let c = (cell.0 + dx, cell.1 + dy);
        square(&mut gizmos, c, color.with_alpha(0.6), 0.04, 0.3);
    }
    if click {
        match world.execute(Command::Place {
            prefab,
            x: cell.0,
            y: cell.1,
        }) {
            Ok(_) => {}
            Err(r) => notices.push(
                world.minute(),
                content.t(r.key()).to_string(),
                Tone::Alert,
                time.elapsed_secs_f64(),
            ),
        }
    }
}

fn g_line(g: &mut Gizmos, a: Vec3, b: Vec3, c: Color) {
    g.line(a, b, c);
}
