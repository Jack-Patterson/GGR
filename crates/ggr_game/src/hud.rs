//! The HUD, laid out after V2's DESIGN.md: a top strip (day, time, pace, notice, renown,
//! treasury), the roster on the left, the quest board and hiring on the right, and the hall in
//! the hole between. Nothing floats over the hall except the one modal sheet. The HUD decides
//! nothing: every button is a `Command` the sim validates.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui::{Color32, RichText};
use ggr_content::PrefabKind;
use ggr_sim::{CharState, Character, Command, OffMapReason, QuestStatus, World};

use crate::camera::{CameraRig, MainCamera};
use crate::persist::Settings;
use crate::state::*;
use crate::theme::{self, c32};
use crate::view3d::CharView;

#[derive(SystemParam)]
pub struct Hud<'w, 's> {
    pub session: ResMut<'w, Session>,
    pub content: Res<'w, ContentRes>,
    pub ui: ResMut<'w, UiState>,
    pub pace: ResMut<'w, Pace>,
    pub notices: ResMut<'w, Notices>,
    pub settings: ResMut<'w, Settings>,
    pub rig: ResMut<'w, CameraRig>,
    pub time: Res<'w, Time>,
    pub exit: MessageWriter<'w, AppExit>,
    pub camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<MainCamera>>,
    pub chars: Query<
        'w,
        's,
        (
            &'static CharView,
            &'static GlobalTransform,
            &'static Visibility,
        ),
    >,
    pub windows: Query<'w, 's, &'static mut Window>,
    pub diagnostics: Res<'w, bevy::diagnostic::DiagnosticsStore>,
}

/// Runs a command, putting any refusal on the notice line. Returns true on success.
pub fn run(h: &mut Hud, cmd: Command) -> bool {
    let now = h.time.elapsed_secs_f64();
    let Some(world) = h.session.world.as_mut() else {
        return false;
    };
    match world.execute(cmd) {
        Ok(_) => true,
        Err(r) => {
            let text = h.content.t(r.key()).to_string();
            let m = world.minute();
            h.notices.push(m, text.clone(), Tone::Alert, now);
            h.ui.status = Some(text);
            false
        }
    }
}

pub fn clock_text(minute: i64) -> String {
    format!("{:02}:{:02}", (minute % 1440) / 60, minute % 60)
}

pub fn duration_text(c: &ContentRes, minutes: i64) -> String {
    let m = minutes.max(0);
    if m >= 1440 {
        c.f(
            "time.days_hours",
            &[&(m / 1440).to_string(), &((m % 1440) / 60).to_string()],
        )
    } else if m >= 60 {
        c.f(
            "time.hours_minutes",
            &[&(m / 60).to_string(), &(m % 60).to_string()],
        )
    } else {
        c.f("time.minutes", &[&m.to_string()])
    }
}

/// A character's state as a short phrase.
pub fn state_text(world: &World, c: &ContentRes, ch: &Character) -> String {
    let now = world.minute();
    let inst_name = |i: u32| {
        let p = world.instances()[i as usize].prefab as usize;
        c.t(&world.content().prefabs[p].name_key).to_string()
    };
    match ch.state {
        CharState::Dead => c.t("state.dead").into(),
        CharState::OffMap(OffMapReason::Quest) => c.t("state.away").into(),
        CharState::OffMap(OffMapReason::OffShift) => c.t("state.off_duty").into(),
        CharState::OffMap(OffMapReason::Left) => c.t("state.left").into(),
        CharState::Travel => match ch.walk.map(|w| w.purpose) {
            Some(ggr_sim::WalkPurpose::ToSlot { inst, .. }) => {
                c.f("state.walking_to", &[&inst_name(inst)])
            }
            Some(ggr_sim::WalkPurpose::DepartQuest) => c.t("state.setting_out").into(),
            Some(ggr_sim::WalkPurpose::EndShift) => c.t("state.going_home").into(),
            _ => c.t("state.walking").into(),
        },
        CharState::Interact => match ch.activity {
            ggr_sim::Activity::Working { inst } => c.f("state.working_at", &[&inst_name(inst)]),
            ggr_sim::Activity::Queueing { inst, .. } => c.f("state.queueing", &[&inst_name(inst)]),
            ggr_sim::Activity::Using { inst, .. } => {
                if ch.is_injured(now) && world.kind_of(inst) == PrefabKind::Infirmary {
                    c.t("state.recovering").into()
                } else {
                    c.f("state.at", &[&inst_name(inst)])
                }
            }
            ggr_sim::Activity::None => c.t("state.idle").into(),
        },
        CharState::Idle => {
            if ch.is_injured(now) {
                c.t("state.injured").into()
            } else {
                c.t("state.idle").into()
            }
        }
    }
}

pub fn draw(mut contexts: EguiContexts, mut h: Hud) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let ctx = ctx.clone();
    if h.session.mode == Mode::MainMenu {
        crate::menus::main_menu(&ctx, &mut h);
        return;
    }
    if h.session.world.is_none() {
        return;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "hud-root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    top_strip(&mut root, &mut h);
    left_column(&mut root, &mut h);
    right_column(&mut root, &mut h);
    let center = root.available_rect_before_wrap();
    let full = ctx.viewport_rect();
    let z = ctx.zoom_factor();
    h.rig.insets = [
        (center.left() - full.left()) * z,
        (full.right() - center.right()) * z,
        (center.top() - full.top()) * z,
        (full.bottom() - center.bottom()) * z,
    ];
    labels(&ctx, &mut h, center);
    if h.ui.objectives_open {
        objectives_overlay(&ctx, &mut h, center);
    }
    crate::sheet::draw(&ctx, &mut h);
    crate::menus::pause_menus(&ctx, &mut h);
    crate::menus::goal_dialog(&ctx, &mut h);
    crate::menus::intro(&ctx, &mut h);
    if h.ui.debug {
        debug_window(&ctx, &mut h);
    }
}

fn top_strip(root: &mut egui::Ui, h: &mut Hud) {
    let frame = egui::Frame::new()
        .fill(c32(theme::SURFACE))
        .inner_margin(egui::Margin::symmetric(14, 6));
    egui::Panel::top("top-strip")
        .exact_size(58.0)
        .frame(frame)
        .resizable(false)
        .show(root, |ui| {
            let c = h.content.clone();
            let world = h.session.world.as_ref().unwrap();
            let minute = world.minute();
            let gold = world.guild().gold;
            let last = world.guild().last_change;
            let reason = world.guild().last_reason;
            let renown = world.guild().renown;
            let tier = world.guild().renown_tier;
            let tiers = world.content().rules.renown_tiers.clone();
            ui.horizontal_centered(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(c.t("hud.day").to_uppercase())
                            .size(10.0)
                            .color(c32(theme::TEXT_MUTED)),
                    );
                    ui.label(theme::mono(world.day().to_string()).size(18.0));
                });
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(c.t("hud.time").to_uppercase())
                            .size(10.0)
                            .color(c32(theme::TEXT_MUTED)),
                    );
                    ui.label(theme::mono(clock_text(minute)).size(18.0));
                });
                ui.add_space(14.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(c.t("hud.pace").to_uppercase())
                            .size(10.0)
                            .color(c32(theme::TEXT_MUTED)),
                    );
                    ui.horizontal(|ui| {
                        let paused = h.pace.paused;
                        if ui
                            .selectable_label(paused, theme::mono("II"))
                            .on_hover_text(c.t("hud.pause_tip"))
                            .clicked()
                        {
                            h.pace.paused = !paused;
                        }
                        for s in [1u32, 2, 4, 8, 16] {
                            let on = !paused && h.pace.speed == s;
                            if ui
                                .selectable_label(on, theme::mono(format!("{s}×")))
                                .on_hover_text(c.t("hud.speed_tip"))
                                .clicked()
                            {
                                h.pace.speed = s;
                                h.pace.paused = false;
                            }
                        }
                    });
                });
                ui.add_space(16.0);
                // The notice line: the newest thing the guild has heard. It takes whatever the
                // fixed clusters either side leave.
                let now = h.time.elapsed_secs_f64();
                let right_w = 620.0;
                let notice_w = (ui.available_width() - right_w).max(80.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(notice_w, 40.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_width(notice_w);
                        if let Some((text, tone, until)) = &h.notices.current {
                            if *until > now {
                                let color = match tone {
                                    Tone::Alert => c32(theme::ACCENT),
                                    Tone::Gain => c32(theme::GAIN),
                                    Tone::Plain => c32(theme::INK),
                                };
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(text).size(13.0).italics().color(color),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 44.0),
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if ui
                            .button(c.t("hud.menu"))
                            .on_hover_text(c.t("hud.menu_tip"))
                            .clicked()
                        {
                            h.ui.menu = Some(MenuPage::Pause);
                        }
                        if ui.button("?").on_hover_text(c.t("hud.howto_tip")).clicked() {
                            h.ui.menu = Some(MenuPage::HowTo);
                            h.ui.menu_return = None;
                        }
                        let building = h.ui.build.is_some();
                        if ui
                            .selectable_label(building, c.t("hud.build"))
                            .on_hover_text(c.t("hud.build_tip"))
                            .clicked()
                        {
                            h.ui.build = if building {
                                None
                            } else {
                                Some(BuildState::default())
                            };
                        }
                        ui.add_space(10.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(190.0, 44.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.label(
                                    RichText::new(c.t("hud.treasury").to_uppercase())
                                        .size(10.0)
                                        .color(c32(theme::TEXT_MUTED)),
                                );
                                ui.horizontal(|ui| {
                                    ui.label(theme::mono(format!("{gold} g")).size(18.0));
                                    let (sign, col) = if last > 0 {
                                        ("+", c32(theme::GAIN))
                                    } else if last < 0 {
                                        ("", c32(theme::ACCENT))
                                    } else {
                                        ("", c32(theme::TEXT_FAINT))
                                    };
                                    ui.label(theme::mono(format!("{sign}{last}")).color(col));
                                    ui.label(
                                        RichText::new(c.t(reason.key()))
                                            .size(10.5)
                                            .color(c32(theme::TEXT_SECONDARY)),
                                    );
                                });
                            },
                        );
                        ui.add_space(12.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(150.0, 44.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.label(
                                    RichText::new(c.t("hud.renown").to_uppercase())
                                        .size(10.0)
                                        .color(c32(theme::TEXT_MUTED)),
                                );
                                let next = tiers.get(tier + 1).copied();
                                let name = c.t(&format!("renown.tier.{tier}")).to_string();
                                let resp = ui.horizontal(|ui| {
                                    ui.label(RichText::new(name).size(14.0).strong());
                                    ui.label(theme::mono(match next {
                                        Some(n) => format!("{renown}/{n}"),
                                        None => renown.to_string(),
                                    }));
                                });
                                resp.response.on_hover_text(c.t("hud.renown_tip"));
                            },
                        );
                    },
                );
            });
            theme::double_rule(ui);
        });
}

fn filter_button(ui: &mut egui::Ui, on: bool, text: &str) -> bool {
    ui.selectable_label(on, RichText::new(text).size(11.0))
        .clicked()
}

fn left_column(root: &mut egui::Ui, h: &mut Hud) {
    egui::Panel::left("roster")
        .exact_size(262.0)
        .resizable(false)
        .frame(theme::panel_frame())
        .show(root, |ui| {
            let c = h.content.clone();
            ui.horizontal(|ui| {
                ui.label(theme::heading(c.t("roster.title").to_uppercase()).size(11.0));
            });
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(
                        h.ui.roster_tab == RosterTab::Adventurers,
                        c.t("roster.adventurers"),
                    )
                    .clicked()
                {
                    h.ui.roster_tab = RosterTab::Adventurers;
                }
                if ui
                    .selectable_label(h.ui.roster_tab == RosterTab::Staff, c.t("roster.staff"))
                    .clicked()
                {
                    h.ui.roster_tab = RosterTab::Staff;
                }
            });
            theme::double_rule(ui);
            ui.add(
                egui::TextEdit::singleline(&mut h.ui.search)
                    .hint_text(c.t("roster.search"))
                    .desired_width(f32::INFINITY),
            );
            let world = h.session.world.as_ref().unwrap();
            let now = world.minute();
            if h.ui.roster_tab == RosterTab::Adventurers {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(c.t("roster.rank"))
                            .size(10.0)
                            .color(c32(theme::TEXT_MUTED)),
                    );
                    if filter_button(
                        ui,
                        h.ui.rank_filter.is_none() && !h.ui.injured_only,
                        c.t("filter.all"),
                    ) {
                        h.ui.rank_filter = None;
                        h.ui.injured_only = false;
                    }
                    for r in 0..=world.content().demo_rank_cap as u8 {
                        let letter = world.content().ranks[r as usize].letter.clone();
                        if filter_button(ui, h.ui.rank_filter == Some(r), &letter) {
                            h.ui.rank_filter = Some(r);
                            h.ui.injured_only = false;
                        }
                    }
                    if filter_button(ui, h.ui.injured_only, c.t("filter.injured")) {
                        h.ui.injured_only = !h.ui.injured_only;
                    }
                    if filter_button(ui, h.ui.show_dead, c.t("filter.dead")) {
                        h.ui.show_dead = !h.ui.show_dead;
                    }
                });
            }
            let staff_tab = h.ui.roster_tab == RosterTab::Staff;
            let search = h.ui.search.to_lowercase();
            let all: Vec<&Character> = world
                .characters()
                .iter()
                .filter(|ch| ch.is_staff() == staff_tab)
                .filter(|ch| ch.state != CharState::OffMap(OffMapReason::Left))
                .collect();
            let shown: Vec<&Character> = all
                .iter()
                .copied()
                .filter(|ch| h.ui.show_dead || ch.alive())
                .filter(|ch| staff_tab || h.ui.rank_filter.is_none_or(|r| ch.rank == r))
                .filter(|ch| staff_tab || !h.ui.injured_only || ch.is_injured(now))
                .filter(|ch| search.is_empty() || ch.name(&c.0).to_lowercase().contains(&search))
                .collect();
            ui.label(
                RichText::new(c.f(
                    "roster.showing",
                    &[&shown.len().to_string(), &all.len().to_string()],
                ))
                .size(10.5)
                .color(c32(theme::TEXT_FAINT)),
            );
            let mut clicked = None;
            let mut double = None;
            let avail = ui.available_height() - 150.0;
            egui::ScrollArea::vertical()
                .id_salt("roster-scroll")
                .max_height(avail.max(120.0))
                .show(ui, |ui| {
                    if shown.is_empty() {
                        ui.label(
                            RichText::new(c.t("roster.empty"))
                                .italics()
                                .color(c32(theme::TEXT_MUTED)),
                        );
                    }
                    for ch in &shown {
                        let selected = h.ui.selected_character == Some(ch.id);
                        let resp = ui
                            .scope_builder(
                                egui::UiBuilder::new().sense(egui::Sense::click()),
                                |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.add_space(3.0);
                                    ui.horizontal(|ui| {
                                        let name = ch.name(&c.0);
                                        let col = if ch.alive() {
                                            c32(theme::INK)
                                        } else {
                                            c32(theme::TEXT_FAINT)
                                        };
                                        ui.label(RichText::new(name).size(14.0).color(col));
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if let Some(st) = &ch.staff {
                                                    ui.label(
                                                        RichText::new(
                                                            c.t(&world.content().roles
                                                                [st.role as usize]
                                                                .name_key),
                                                        )
                                                        .size(10.5)
                                                        .color(c32(theme::TEXT_FAINT)),
                                                    );
                                                } else {
                                                    ui.label(
                                                        theme::mono(
                                                            world.content().ranks[ch.rank as usize]
                                                                .letter
                                                                .clone(),
                                                        )
                                                        .strong(),
                                                    );
                                                    let class = ch
                                                        .adv
                                                        .as_ref()
                                                        .map(|a| a.class as usize)
                                                        .unwrap_or(0);
                                                    ui.label(
                                                        RichText::new(
                                                            c.t(&world.content().classes[class]
                                                                .name_key),
                                                        )
                                                        .size(10.5)
                                                        .color(c32(theme::TEXT_FAINT)),
                                                    );
                                                }
                                            },
                                        );
                                    });
                                    let mut line = state_text(world, &c, ch);
                                    if world.promotions().iter().any(|p| p.character == ch.id) {
                                        line = format!("{line} · {}", c.t("roster.promotion"));
                                    }
                                    let col = if ch.is_injured(now) || !ch.alive() {
                                        c32(theme::ACCENT)
                                    } else {
                                        c32(theme::TEXT_SECONDARY)
                                    };
                                    ui.label(RichText::new(line).size(11.0).color(col));
                                    ui.add_space(3.0);
                                },
                            )
                            .response;
                        if selected {
                            let r = resp.rect;
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(r.left() - 6.0, r.top()),
                                    egui::vec2(2.0, r.height()),
                                ),
                                0.0,
                                c32(theme::ACCENT),
                            );
                        }
                        if resp.hovered() {
                            ui.painter().rect_filled(
                                resp.rect,
                                0.0,
                                Color32::from_rgba_unmultiplied(0x22, 0x20, 0x1B, 10),
                            );
                        }
                        if resp.double_clicked() {
                            double = Some(ch.id);
                        } else if resp.clicked() {
                            clicked = Some(ch.id);
                        }
                        theme::row_rule(ui);
                    }
                });
            if let Some(id) = clicked {
                if h.ui.selected_character == Some(id) {
                    h.ui.open_sheet(Sheet::Character(id));
                }
                h.ui.selected_character = Some(id);
                h.ui.selected_instance = None;
                follow(h, id);
            }
            if let Some(id) = double {
                h.ui.selected_character = Some(id);
                h.ui.open_sheet(Sheet::Character(id));
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                objectives_panel(ui, h);
            });
        });
}

/// Pans the camera to a character on the map.
fn follow(h: &mut Hud, id: u32) {
    for (cv, tf, vis) in &h.chars {
        if cv.id == id && *vis != Visibility::Hidden {
            h.rig.target = Vec3::new(tf.translation().x, 0.0, tf.translation().z);
        }
    }
}

fn objectives_panel(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let obj = world.objectives();
    let next = ggr_sim::Objective::ALL
        .iter()
        .find(|o| !obj.is_done(**o))
        .copied();
    ui.add_space(2.0);
    if ui
        .button(
            RichText::new(c.f(
                "objectives.button",
                &[
                    &obj.done.len().to_string(),
                    &ggr_sim::Objective::ALL.len().to_string(),
                ],
            ))
            .size(11.0),
        )
        .on_hover_text(c.t("objectives.tip"))
        .clicked()
    {
        h.ui.objectives_open = !h.ui.objectives_open;
    }
    if let Some(o) = next {
        ui.label(
            RichText::new(c.t(o.hint_key()))
                .size(11.0)
                .italics()
                .color(c32(theme::TEXT_SECONDARY)),
        );
        ui.label(RichText::new(c.t(o.key())).size(13.0).strong());
        theme::section_label(ui, c.t("objectives.next"));
    }
    theme::row_rule(ui);
}

fn objectives_overlay(ctx: &egui::Context, h: &mut Hud, center: egui::Rect) {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let obj = world.objectives().clone();
    egui::Area::new(egui::Id::new("objectives"))
        .fixed_pos(egui::pos2(center.left() + 12.0, center.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            theme::panel_frame().show(ui, |ui| {
                ui.set_width(330.0);
                ui.label(theme::heading(c.t("objectives.title")));
                theme::double_rule(ui);
                for o in ggr_sim::Objective::ALL {
                    let done = obj.is_done(o);
                    ui.horizontal(|ui| {
                        ui.label(theme::mono(if done { "✔" } else { "·" }).color(if done {
                            c32(theme::GAIN)
                        } else {
                            c32(theme::TEXT_FAINT)
                        }));
                        let r = ui.label(RichText::new(c.t(o.key())).color(if done {
                            c32(theme::TEXT_FAINT)
                        } else {
                            c32(theme::INK)
                        }));
                        r.on_hover_text(c.t(o.hint_key()));
                    });
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(c.t("objectives.footer"))
                        .size(10.5)
                        .italics()
                        .color(c32(theme::TEXT_MUTED)),
                );
            });
        });
}

fn right_column(root: &mut egui::Ui, h: &mut Hud) {
    egui::Panel::right("right")
        .exact_size(310.0)
        .resizable(false)
        .frame(theme::panel_frame())
        .show(root, |ui| {
            if h.ui.build.is_some() {
                build_palette(ui, h);
                return;
            }
            let c = h.content.clone();
            ui.horizontal(|ui| {
                for (tab, key) in [
                    (RightTab::Board, "board.title"),
                    (RightTab::Chronicle, "chronicle.title"),
                    (RightTab::Outfitter, "outfitter.title"),
                ] {
                    if ui
                        .selectable_label(h.ui.right_tab == tab, RichText::new(c.t(key)).size(12.0))
                        .clicked()
                    {
                        h.ui.right_tab = tab;
                    }
                }
            });
            theme::double_rule(ui);
            let hiring_h = desk_queue_height(h);
            let list_h = (ui.available_height() - hiring_h - 8.0).max(160.0);
            ui.allocate_ui(egui::vec2(ui.available_width(), list_h), |ui| {
                match h.ui.right_tab {
                    RightTab::Board => quest_board(ui, h),
                    RightTab::Chronicle => chronicle(ui, h),
                    RightTab::Outfitter => outfitter(ui, h),
                }
            });
            desk_queue(ui, h);
        });
}

fn quest_board(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    ui.horizontal(|ui| {
        if ui
            .selectable_label(!h.ui.underway_tab, c.t("board.posted"))
            .clicked()
        {
            h.ui.underway_tab = false;
        }
        if ui
            .selectable_label(h.ui.underway_tab, c.t("board.underway"))
            .clicked()
        {
            h.ui.underway_tab = true;
        }
    });
    let world = h.session.world.as_ref().unwrap();
    let now = world.minute();
    let quests: Vec<&ggr_sim::Quest> = world
        .quests()
        .iter()
        .filter(|q| {
            if h.ui.underway_tab {
                q.is_active()
            } else {
                q.is_posted()
            }
        })
        .collect();
    let cap = world.content().rules.board_capacity;
    if !h.ui.underway_tab {
        ui.label(
            RichText::new(c.f(
                "board.capacity",
                &[&quests.len().to_string(), &cap.to_string()],
            ))
            .size(10.5)
            .color(c32(theme::TEXT_FAINT)),
        );
    }
    let mut open = None;
    egui::ScrollArea::vertical()
        .id_salt("board-scroll")
        .show(ui, |ui| {
            if quests.is_empty() {
                let key = if h.ui.underway_tab {
                    "board.empty_underway"
                } else {
                    "board.empty_posted"
                };
                ui.label(
                    RichText::new(c.t(key))
                        .italics()
                        .color(c32(theme::TEXT_MUTED)),
                );
            }
            for q in quests {
                let resp = ui
                    .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                        ui.set_width(ui.available_width());
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Label::new(RichText::new(world.quest_title(q)).size(13.0))
                                    .truncate(),
                            );
                        });
                        ui.horizontal(|ui| {
                            ui.label(
                                theme::mono(world.content().ranks[q.rank as usize].letter.clone())
                                    .strong(),
                            );
                            ui.label(
                                theme::mono(format!("{} – {} g", q.gold.0, q.gold.1))
                                    .color(c32(theme::GAIN)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let text = if q.is_active() {
                                        match q.taker {
                                            Some(t) if q.status == QuestStatus::Underway => c.f(
                                                "board.due",
                                                &[
                                                    &world.characters()[t as usize].name(&c.0),
                                                    &duration_text(&c, q.due_back - now),
                                                ],
                                            ),
                                            Some(t) => world.characters()[t as usize].name(&c.0),
                                            None => String::new(),
                                        }
                                    } else {
                                        c.f("board.expires", &[&duration_text(&c, q.expires - now)])
                                    };
                                    ui.label(
                                        RichText::new(text)
                                            .size(10.5)
                                            .color(c32(theme::TEXT_SECONDARY)),
                                    );
                                },
                            );
                        });
                        ui.add_space(3.0);
                    })
                    .response
                    .on_hover_text(world.quest_description(q));
                if resp.clicked() {
                    open = Some(q.id);
                }
                theme::row_rule(ui);
            }
        });
    if let Some(q) = open {
        h.ui.open_sheet(Sheet::Quest(q));
    }
}

fn chronicle(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    egui::ScrollArea::vertical()
        .id_salt("chronicle-scroll")
        .show(ui, |ui| {
            if h.notices.log.is_empty() {
                ui.label(
                    RichText::new(c.t("chronicle.empty"))
                        .italics()
                        .color(c32(theme::TEXT_MUTED)),
                );
            }
            for (m, text, tone) in &h.notices.log {
                let color = match tone {
                    Tone::Alert => c32(theme::ACCENT),
                    Tone::Gain => c32(theme::GAIN),
                    Tone::Plain => c32(theme::INK),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        theme::mono(format!(
                            "{} {}",
                            c.f("chronicle.day", &[&(m / 1440 + 1).to_string()]),
                            clock_text(*m)
                        ))
                        .size(10.5)
                        .color(c32(theme::TEXT_FAINT)),
                    );
                    ui.label(RichText::new(text).size(12.0).color(color));
                });
                theme::row_rule(ui);
            }
        });
}

fn outfitter(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    ui.label(
        RichText::new(c.t("outfitter.intro"))
            .size(11.0)
            .italics()
            .color(c32(theme::TEXT_SECONDARY)),
    );
    let mut buy = None;
    {
        let world = h.session.world.as_ref().unwrap();
        let gold = world.guild().gold;
        egui::ScrollArea::vertical()
            .id_salt("outfitter-scroll")
            .show(ui, |ui| {
                for (i, item) in world.content().items.iter().enumerate() {
                    let have = world.guild().stash[i];
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(c.t(&item.name_key)).size(13.0));
                            ui.label(
                                RichText::new(bonus_text(&c, &item.bonus, item.two_handed))
                                    .size(10.5)
                                    .color(c32(theme::TEXT_SECONDARY)),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if item.price > 0 {
                                let afford = gold >= item.price;
                                let b = ui.add_enabled(
                                    afford,
                                    egui::Button::new(theme::mono(format!("{} g", item.price))),
                                );
                                if b.on_hover_text(c.t("outfitter.buy_tip")).clicked() {
                                    buy = Some(i);
                                }
                            } else {
                                ui.label(
                                    RichText::new(c.t("outfitter.loot_only"))
                                        .size(10.5)
                                        .color(c32(theme::TEXT_FAINT)),
                                );
                            }
                            ui.label(
                                theme::mono(c.f("outfitter.have", &[&have.to_string()]))
                                    .color(c32(theme::TEXT_SECONDARY)),
                            );
                        });
                    });
                    theme::row_rule(ui);
                }
            });
    }
    if let Some(i) = buy {
        run(h, Command::Buy { item: i });
    }
}

pub fn bonus_text(c: &ContentRes, bonus: &[i32; 6], two_handed: bool) -> String {
    let mut parts: Vec<String> = ggr_content::Attr::ALL
        .iter()
        .filter(|a| bonus[a.index()] != 0)
        .map(|a| {
            format!(
                "{:+} {}",
                bonus[a.index()],
                c.t(&c.0.attributes[a.index()].short_key)
            )
        })
        .collect();
    if two_handed {
        parts.push(c.t("item.two_handed").to_string());
    }
    parts.join(" · ")
}

fn desk_queue_height(h: &Hud) -> f32 {
    let world = h.session.world.as_ref().unwrap();
    let n = world.waiting_candidates().count().min(3) + world.promotions().len().min(3);
    if n == 0 {
        70.0
    } else {
        60.0 + n as f32 * 54.0
    }
}

/// Who is waiting at the desk: candidates and promotion requests.
fn desk_queue(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let now = world.minute();
    let manned = world.desk_manned();
    let can_sign = world.desk_can_sign();
    let gold = world.guild().gold;
    theme::section_label(ui, c.t("desk.title"));
    theme::double_rule(ui);
    ui.horizontal(|ui| {
        let (txt, col) = if manned {
            (c.t("desk.manned"), c32(theme::GAIN))
        } else if can_sign {
            (c.t("desk.guildmaster"), c32(theme::WARN))
        } else {
            (c.t("refusal.desk_unmanned"), c32(theme::ACCENT))
        };
        ui.label(RichText::new(txt).size(11.0).color(col))
            .on_hover_text(c.t("desk.tip"));
    });
    let cands: Vec<&ggr_sim::Candidate> = world.waiting_candidates().collect();
    let total = cands.len();
    let mut open = None;
    if total == 0 && world.promotions().is_empty() {
        ui.label(
            RichText::new(c.t("desk.nobody"))
                .italics()
                .size(12.0)
                .color(c32(theme::TEXT_MUTED)),
        );
    }
    for (i, cand) in cands.iter().take(3).enumerate() {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(c.f(
                        "desk.candidate_n",
                        &[&(i + 1).to_string(), &total.to_string()],
                    ))
                    .size(10.0)
                    .color(c32(theme::TEXT_FAINT)),
                );
                ui.label(RichText::new(cand.name(&c.0)).size(15.0));
                let what = match cand.staff_role {
                    Some(r) => c.t(&c.0.roles[r as usize].name_key).to_string(),
                    None => c.f(
                        "desk.adventurer_rank",
                        &[&c.0.ranks[cand.rank as usize].letter],
                    ),
                };
                let left = cand.expires - now;
                let urgent = left < 120;
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(what)
                            .size(11.0)
                            .color(c32(theme::TEXT_SECONDARY)),
                    );
                    ui.label(
                        theme::mono(format!("{} g", cand.cost)).color(if cand.cost > gold {
                            c32(theme::ACCENT)
                        } else {
                            c32(theme::INK)
                        }),
                    );
                    ui.label(
                        RichText::new(c.f("desk.leaves_in", &[&duration_text(&c, left)]))
                            .size(10.5)
                            .color(if urgent {
                                c32(theme::ACCENT)
                            } else {
                                c32(theme::TEXT_SECONDARY)
                            }),
                    );
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(c.t("desk.details")).clicked() {
                    open = Some(Sheet::Candidate(cand.id));
                }
            });
        });
        theme::row_rule(ui);
    }
    for p in world.promotions().iter().take(3) {
        let ch = &world.characters()[p.character as usize];
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(c.t("desk.promotion"))
                        .size(10.0)
                        .color(c32(theme::GAIN)),
                );
                ui.label(RichText::new(ch.name(&c.0)).size(15.0));
                ui.horizontal(|ui| {
                    ui.label(theme::mono(format!(
                        "{} → {}",
                        c.0.ranks[ch.rank as usize].letter, c.0.ranks[p.to_rank as usize].letter
                    )));
                    ui.label(theme::mono(format!("{} g", p.fee)));
                    ui.label(
                        RichText::new(
                            c.f("desk.leaves_in", &[&duration_text(&c, p.expires - now)]),
                        )
                        .size(10.5)
                        .color(c32(theme::TEXT_SECONDARY)),
                    );
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(c.t("desk.details")).clicked() {
                    open = Some(Sheet::Character(p.character));
                }
            });
        });
        theme::row_rule(ui);
    }
    if let Some(s) = open {
        h.ui.open_sheet(s);
    }
}

fn build_palette(ui: &mut egui::Ui, h: &mut Hud) {
    let c = h.content.clone();
    ui.horizontal(|ui| {
        ui.label(theme::heading(c.t("build.title").to_uppercase()).size(11.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(c.t("build.done")).clicked() {
                h.ui.build = None;
            }
        });
    });
    theme::double_rule(ui);
    ui.label(
        RichText::new(c.t("build.help"))
            .size(11.0)
            .italics()
            .color(c32(theme::TEXT_SECONDARY)),
    );
    let Some(mut state) = h.ui.build else { return };
    let mut open_wing = false;
    {
        let world = h.session.world.as_ref().unwrap();
        let gold = world.guild().gold;
        let tier = world.guild().renown_tier;
        egui::ScrollArea::vertical()
            .id_salt("build-scroll")
            .max_height(ui.available_height() - 150.0)
            .show(ui, |ui| {
                for (i, p) in world.content().prefabs.iter().enumerate() {
                    let locked = p.renown_tier > tier;
                    let selected = state.prefab == Some(i) && !state.demolish;
                    let resp = ui
                        .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                            ui.set_width(ui.available_width());
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                let col = if locked {
                                    c32(theme::TEXT_FAINT)
                                } else {
                                    c32(theme::INK)
                                };
                                ui.label(RichText::new(c.t(&p.name_key)).size(13.0).color(col));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(theme::mono(format!("{} g", p.cost)).color(
                                            if p.cost > gold {
                                                c32(theme::ACCENT)
                                            } else {
                                                c32(theme::INK)
                                            },
                                        ));
                                    },
                                );
                            });
                            let line = if locked {
                                c.f(
                                    "build.needs_renown",
                                    &[c.t(&format!("renown.tier.{}", p.renown_tier))],
                                )
                            } else {
                                c.f("build.takes", &[&duration_text(&c, p.build_minutes)])
                            };
                            ui.label(
                                RichText::new(line)
                                    .size(10.5)
                                    .color(c32(theme::TEXT_SECONDARY)),
                            );
                            ui.add_space(2.0);
                        })
                        .response
                        .on_hover_text(c.t(&p.desc_key));
                    if selected {
                        let r = resp.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(r.left() - 6.0, r.top()),
                                egui::vec2(2.0, r.height()),
                            ),
                            0.0,
                            c32(theme::ACCENT),
                        );
                    }
                    if resp.clicked() && !locked {
                        state.prefab = Some(i);
                        state.demolish = false;
                    }
                    theme::row_rule(ui);
                }
            });
        ui.add_space(6.0);
        if ui
            .selectable_label(state.demolish, c.t("build.demolish"))
            .on_hover_text(c.t("build.demolish_tip"))
            .clicked()
        {
            state.demolish = !state.demolish;
            state.prefab = None;
        }
        if let Some(Err(r)) = h.ui.ghost_ok {
            if state.prefab.is_some() {
                ui.label(
                    RichText::new(c.t(r.key()))
                        .size(11.0)
                        .color(c32(theme::ACCENT)),
                );
            }
        }
        ui.add_space(6.0);
        if !world.east_wing_open() {
            theme::section_label(ui, c.t("build.east_wing"));
            let r = &world.content().rules;
            let can = tier >= r.east_wing_renown_tier;
            ui.label(
                RichText::new(c.t("build.east_wing_desc"))
                    .size(11.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            let label = if can {
                c.f("build.open_wing", &[&r.east_wing_cost.to_string()])
            } else {
                c.f(
                    "build.needs_renown",
                    &[c.t(&format!("renown.tier.{}", r.east_wing_renown_tier))],
                )
            };
            if ui
                .add_enabled(can && gold >= r.east_wing_cost, egui::Button::new(label))
                .clicked()
            {
                open_wing = true;
            }
        }
    }
    h.ui.build = Some(state);
    if open_wing {
        run(h, Command::OpenEastWing);
    }
}

/// Rank letters over every capsule, and the name of the selected one.
fn labels(ctx: &egui::Context, h: &mut Hud, center: egui::Rect) {
    let Ok((cam, cam_tf)) = h.camera.single() else {
        return;
    };
    let world = h.session.world.as_ref().unwrap();
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("labels"),
        ))
        .with_clip_rect(center);
    let z = ctx.zoom_factor();
    for (cv, tf, vis) in &h.chars {
        if *vis == Visibility::Hidden {
            continue;
        }
        let Some(ch) = world.characters().get(cv.id as usize) else {
            continue;
        };
        let head = tf.translation() + Vec3::Y * 1.45;
        let Ok(p) = cam.world_to_viewport(cam_tf, head) else {
            continue;
        };
        let pos = egui::pos2(p.x / z, p.y / z);
        let selected = h.ui.selected_character == Some(cv.id);
        let tag = match &ch.staff {
            Some(st) => c_initial(
                &h.content,
                &world.content().roles[st.role as usize].name_key,
            ),
            None => world.content().ranks[ch.rank as usize].letter.clone(),
        };
        painter.text(
            pos,
            egui::Align2::CENTER_BOTTOM,
            tag,
            egui::FontId::monospace(11.0),
            if selected {
                c32(theme::ACCENT)
            } else {
                c32(theme::INK)
            },
        );
        if selected {
            let name = ch.name(&h.content.0);
            let galley =
                painter.layout_no_wrap(name, egui::FontId::proportional(12.0), c32(theme::INK));
            let rect = egui::Rect::from_center_size(
                pos - egui::vec2(0.0, 22.0),
                galley.size() + egui::vec2(10.0, 4.0),
            );
            painter.rect_filled(rect, 0.0, c32(theme::SURFACE));
            painter.rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, c32(theme::ACCENT)),
                egui::StrokeKind::Inside,
            );
            painter.galley(rect.min + egui::vec2(5.0, 2.0), galley, c32(theme::INK));
        }
    }
}

fn c_initial(c: &ContentRes, key: &str) -> String {
    c.t(key)
        .chars()
        .next()
        .map(|ch| ch.to_string())
        .unwrap_or_default()
}

fn debug_window(ctx: &egui::Context, h: &mut Hud) {
    let world = h.session.world.as_ref().unwrap();
    let fps = h
        .diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    egui::Window::new(h.content.t("debug.title"))
        .default_pos(egui::pos2(300.0, 80.0))
        .show(ctx, |ui| {
            let s = world.stats();
            let a = world.audit();
            ui.label(theme::mono(format!(
                "minute {}  seed {}  fps {fps:.0}",
                world.minute(),
                world.rng_seed()
            )));
            ui.label(theme::mono(format!(
                "state hash {:016x}",
                world.state_hash()
            )));
            ui.label(theme::mono(format!(
                "scheduled {}  stale {}",
                world.scheduler().pending(),
                s.stale_events
            )));
            ui.label(theme::mono(format!(
                "slots: {} reserved, {} arrived, {} released, {} cancelled",
                a.reserves, a.arrivals, a.releases, a.cancellations
            )));
            ui.label(theme::mono(format!(
                "quests: {} posted {} taken {} ok {} failed {} died {} expired",
                s.posted, s.taken, s.succeeded, s.failed, s.died, s.expired
            )));
            ui.label(theme::mono(format!(
                "gold in {} out {}  meals {}  quits {}",
                s.gold_earned, s.gold_spent, s.meals, s.quits
            )));
            match world.verify_consistency() {
                Ok(()) => ui.label(theme::mono("consistency OK").color(c32(theme::GAIN))),
                Err(e) => ui.label(theme::mono(e).color(c32(theme::ACCENT))),
            };
        });
}
