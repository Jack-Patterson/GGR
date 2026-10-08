//! Menus: the main menu (over an attract-mode guild), the pause menu, save and load slots,
//! options (audio, display, rebinding, language) and the "how the guild works" reference.

use bevy::prelude::*;
use bevy_egui::egui;
use egui::RichText;

use crate::automation::Args;
use crate::hud::Hud;
use crate::input::{key_from_name, Action};
use crate::persist::{self, Settings};
use crate::state::*;
use crate::theme::{self, c32};

/// Startup: an attract-mode guild behind the main menu, or straight into play when asked.
pub fn boot(
    mut session: ResMut<Session>,
    content: Res<ContentRes>,
    args: Res<Args>,
    mut settings: ResMut<Settings>,
) {
    if args.screenshot.is_some() {
        // Unattended: never stop on the first-run welcome.
        settings.seen_intro = true;
    }
    let attract = ggr_sim::World::new(content.0.clone(), 7);
    session.replace(attract, Mode::MainMenu);
    if args.new_game {
        let seed = args.seed.unwrap_or_else(fresh_seed);
        session.replace(ggr_sim::World::new(content.0.clone(), seed), Mode::Playing);
    }
}

fn fresh_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42)
}

fn start_new(h: &mut Hud) {
    let world = ggr_sim::World::new(h.content.0.clone(), fresh_seed());
    h.session.replace(world, Mode::Playing);
    *h.ui = UiState::default();
    *h.pace = Pace::default();
    h.notices.log.clear();
    h.notices.current = None;
    *h.rig = crate::camera::CameraRig::default();
}

fn big_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).size(16.0)).min_size(egui::vec2(260.0, 36.0)),
    )
    .clicked()
}

pub fn main_menu(ctx: &egui::Context, h: &mut Hud) {
    let c = h.content.clone();
    let slots = persist::slots();
    let auto = slots.autosave_path();
    let continue_summary = slots.summary(&auto);
    egui::Area::new(egui::Id::new("main-menu"))
        .anchor(egui::Align2::LEFT_CENTER, egui::vec2(80.0, 0.0))
        .show(ctx, |ui| {
            theme::panel_frame()
                .inner_margin(egui::Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(320.0);
                    ui.label(RichText::new(c.t("menu.title")).size(30.0));
                    ui.label(
                        RichText::new(c.t("menu.tagline"))
                            .italics()
                            .color(c32(theme::TEXT_SECONDARY)),
                    );
                    theme::double_rule(ui);
                    ui.add_space(8.0);
                    if big_button(ui, c.t("menu.new_game"), true) {
                        start_new(h);
                    }
                    let cont_label = match &continue_summary {
                        Some(s) => c.f("menu.continue_day", &[&s.day.to_string()]),
                        None => c.t("menu.continue").to_string(),
                    };
                    if big_button(ui, &cont_label, continue_summary.is_some()) {
                        if let Err(e) =
                            persist::load_from(&auto, &h.content, &mut h.session, &mut h.rig)
                        {
                            h.ui.status = Some(e);
                        } else {
                            h.notices.log.clear();
                            *h.pace = Pace::default();
                        }
                    }
                    if big_button(ui, c.t("menu.load"), true) {
                        h.ui.menu = Some(MenuPage::Load);
                    }
                    if big_button(ui, c.t("menu.options"), true) {
                        h.ui.menu = Some(MenuPage::Options);
                    }
                    if big_button(ui, c.t("menu.howto"), true) {
                        h.ui.menu = Some(MenuPage::HowTo);
                    }
                    if big_button(ui, c.t("menu.quit"), true) {
                        h.exit.write(AppExit::Success);
                    }
                    ui.add_space(10.0);
                    if let Some(s) = &h.ui.status {
                        ui.label(RichText::new(s).color(c32(theme::ACCENT)));
                    }
                    ui.label(
                        RichText::new(c.t("menu.demo_note"))
                            .size(10.5)
                            .italics()
                            .color(c32(theme::TEXT_MUTED)),
                    );
                    ui.label(
                        theme::mono(format!("v{}", ggr_core::BUILD_VERSION))
                            .size(10.0)
                            .color(c32(theme::TEXT_FAINT)),
                    );
                });
        });
    pause_menus(ctx, h);
}

fn window(ctx: &egui::Context, title: &str, width: f32, add: impl FnOnce(&mut egui::Ui)) -> bool {
    let resp = egui::Modal::new(egui::Id::new(("menu", title)))
        .frame(theme::panel_frame().inner_margin(egui::Margin::same(22)))
        .backdrop_color(theme::scrim())
        .show(ctx, |ui| {
            ui.set_width(width);
            ui.label(RichText::new(title).size(22.0));
            theme::double_rule(ui);
            add(ui);
        });
    resp.should_close()
}

pub fn pause_menus(ctx: &egui::Context, h: &mut Hud) {
    let Some(page) = h.ui.menu else { return };
    let c = h.content.clone();
    let playing = h.session.mode == Mode::Playing;
    let mut next: Option<Option<MenuPage>> = None;
    let close = match page {
        MenuPage::Pause => window(ctx, c.t("menu.paused"), 300.0, |ui| {
            if big_button(ui, c.t("menu.resume"), true) {
                next = Some(None);
            }
            if big_button(ui, c.t("menu.save"), playing) {
                next = Some(Some(MenuPage::Save));
            }
            if big_button(ui, c.t("menu.load"), true) {
                next = Some(Some(MenuPage::Load));
            }
            if big_button(ui, c.t("menu.options"), true) {
                next = Some(Some(MenuPage::Options));
            }
            if big_button(ui, c.t("menu.howto"), true) {
                next = Some(Some(MenuPage::HowTo));
            }
            if big_button(ui, c.t("menu.main_menu"), true) {
                next = Some(Some(MenuPage::Confirm(ConfirmAction::MainMenu)));
            }
            if big_button(ui, c.t("menu.quit"), true) {
                next = Some(Some(MenuPage::Confirm(ConfirmAction::Quit)));
            }
        }),
        MenuPage::Save | MenuPage::Load => {
            let saving = page == MenuPage::Save;
            let title = if saving {
                c.t("menu.save")
            } else {
                c.t("menu.load")
            };
            let slots = persist::slots();
            let mut result: Option<Result<String, String>> = None;
            let close = window(ctx, title, 420.0, |ui| {
                ui.label(
                    RichText::new(c.t("menu.demo_note"))
                        .size(10.5)
                        .italics()
                        .color(c32(theme::TEXT_MUTED)),
                );
                let mut paths: Vec<(String, std::path::PathBuf)> = (1..=ggr_save::SaveSlots::SLOTS)
                    .map(|n| (c.f("menu.slot", &[&n.to_string()]), slots.slot_path(n)))
                    .collect();
                if !saving {
                    paths.push((c.t("menu.autosave").to_string(), slots.autosave_path()));
                }
                for (label, path) in paths {
                    let summary = slots.summary(&path);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&label).size(14.0));
                            let line = match &summary {
                                Some(s) => c.f(
                                    "menu.slot_summary",
                                    &[
                                        &s.day.to_string(),
                                        &s.gold.to_string(),
                                        &s.roster.to_string(),
                                        &s.renown.to_string(),
                                    ],
                                ),
                                None => c.t("menu.slot_empty").to_string(),
                            };
                            ui.label(
                                RichText::new(line)
                                    .size(11.0)
                                    .color(c32(theme::TEXT_SECONDARY)),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if saving {
                                if ui.button(c.t("menu.save_here")).clicked() {
                                    result = Some(
                                        persist::save_to(&path, &mut h.session, &h.rig, &label)
                                            .map(|_| c.f("menu.saved", &[&label])),
                                    );
                                }
                            } else if ui
                                .add_enabled(
                                    summary.is_some(),
                                    egui::Button::new(c.t("menu.load_this")),
                                )
                                .clicked()
                            {
                                result = Some(
                                    persist::load_from(
                                        &path,
                                        &h.content,
                                        &mut h.session,
                                        &mut h.rig,
                                    )
                                    .map(|_| c.f("menu.loaded", &[&label])),
                                );
                            }
                        });
                    });
                    theme::row_rule(ui);
                }
                if let Some(s) = &h.ui.status {
                    ui.label(RichText::new(s).size(12.0));
                }
                if ui.button(c.t("menu.back")).clicked() {
                    next = Some(if playing { Some(MenuPage::Pause) } else { None });
                }
            });
            match result {
                Some(Ok(msg)) => {
                    h.ui.status = Some(msg);
                    if !saving {
                        *h.pace = Pace::default();
                        h.notices.log.clear();
                        h.ui.sheet.clear();
                        next = Some(None);
                    }
                }
                Some(Err(e)) => h.ui.status = Some(e),
                None => {}
            }
            close
        }
        MenuPage::Options => window(ctx, c.t("menu.options"), 460.0, |ui| {
            options(ui, h, &mut next)
        }),
        MenuPage::HowTo => window(ctx, c.t("menu.howto"), 620.0, |ui| {
            egui::ScrollArea::vertical()
                .max_height(ctx.viewport_rect().height() - 200.0)
                .show(ui, |ui| {
                    // Keep the key column clear of the scrollbar.
                    ui.set_max_width(604.0);
                    for i in 1..=9 {
                        theme::section_label(ui, c.t(&format!("howto.{i}.title")));
                        ui.label(RichText::new(c.t(&format!("howto.{i}.body"))).size(13.0));
                    }
                    theme::section_label(ui, c.t("howto.controls"));
                    for a in Action::ALL {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(c.t(&a.label_key())).size(12.0));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(theme::mono(
                                        h.settings
                                            .bindings
                                            .get(a.token())
                                            .cloned()
                                            .unwrap_or_default(),
                                    ));
                                },
                            );
                        });
                    }
                    ui.label(RichText::new(c.t("howto.mouse")).size(12.0));
                });
            if ui.button(c.t("menu.back")).clicked() {
                next = Some(if playing {
                    h.ui.menu_return
                        .take()
                        .or(Some(MenuPage::Pause))
                        .filter(|_| playing)
                } else {
                    None
                });
            }
        }),
        MenuPage::Confirm(what) => window(ctx, c.t("menu.are_you_sure"), 320.0, |ui| {
            ui.label(c.t("menu.unsaved"));
            ui.horizontal(|ui| {
                if ui.button(c.t("menu.yes")).clicked() {
                    match what {
                        ConfirmAction::Quit => {
                            h.exit.write(AppExit::Success);
                        }
                        ConfirmAction::MainMenu => {
                            let attract = ggr_sim::World::new(h.content.0.clone(), 7);
                            h.session.replace(attract, Mode::MainMenu);
                            *h.ui = UiState::default();
                        }
                    }
                    next = Some(None);
                }
                if ui.button(c.t("menu.no")).clicked() {
                    next = Some(Some(MenuPage::Pause));
                }
            });
        }),
    };
    if let Some(n) = next {
        if h.ui.menu.is_some() {
            h.ui.menu = n;
        }
        if n.is_none() || n == Some(MenuPage::Pause) {
            h.ui.status = None;
        }
    } else if close {
        h.ui.menu = if playing && page != MenuPage::Pause {
            Some(MenuPage::Pause)
        } else {
            None
        };
    }
}

fn options(ui: &mut egui::Ui, h: &mut Hud, next: &mut Option<Option<MenuPage>>) {
    let c = h.content.clone();
    let playing = h.session.mode == Mode::Playing;
    let before = h.settings.clone();
    theme::section_label(ui, c.t("options.audio"));
    ui.add(egui::Slider::new(&mut h.settings.master_volume, 0.0..=1.0).text(c.t("options.volume")));
    ui.checkbox(&mut h.settings.sfx, c.t("options.sfx"));
    theme::section_label(ui, c.t("options.display"));
    ui.checkbox(&mut h.settings.fullscreen, c.t("options.fullscreen"));
    ui.horizontal(|ui| {
        ui.label(c.t("options.resolution"));
        for r in [
            (1280u32, 800u32),
            (1440, 900),
            (1600, 900),
            (1920, 1080),
            (2560, 1440),
        ] {
            if ui
                .selectable_label(
                    h.settings.resolution == r,
                    theme::mono(format!("{}×{}", r.0, r.1)),
                )
                .clicked()
            {
                h.settings.resolution = r;
            }
        }
    });
    ui.add(egui::Slider::new(&mut h.settings.ui_scale, 0.8..=1.5).text(c.t("options.ui_scale")));
    theme::section_label(ui, c.t("options.language"));
    egui::ComboBox::from_id_salt("language")
        .selected_text(c.0.loc.language())
        .show_ui(ui, |ui| {
            let lang = c.0.loc.language().to_string();
            ui.selectable_value(&mut h.settings.language, lang.clone(), lang);
        });
    theme::section_label(ui, c.t("options.controls"));
    ui.label(
        RichText::new(c.t("options.rebind_help"))
            .size(11.0)
            .italics()
            .color(c32(theme::TEXT_SECONDARY)),
    );
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .show(ui, |ui| {
            ui.set_max_width(444.0);
            for a in Action::ALL {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(c.t(&a.label_key())).size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let waiting = h.ui.rebinding == Some(a);
                        let key = h
                            .settings
                            .bindings
                            .get(a.token())
                            .cloned()
                            .unwrap_or_default();
                        let label = if waiting {
                            c.t("options.press_key").to_string()
                        } else {
                            key.clone()
                        };
                        let enabled = playing;
                        if ui
                            .add_enabled(enabled, egui::Button::new(theme::mono(label)))
                            .clicked()
                        {
                            h.ui.rebinding = Some(a);
                        }
                        // Flag a key bound to two actions.
                        let dup = h
                            .settings
                            .bindings
                            .iter()
                            .filter(|(_, v)| **v == key)
                            .count()
                            > 1;
                        if dup && key_from_name(&key).is_some() {
                            ui.label(
                                RichText::new(c.t("options.duplicate"))
                                    .size(10.5)
                                    .color(c32(theme::WARN)),
                            );
                        }
                    });
                });
            }
        });
    ui.horizontal(|ui| {
        if ui.button(c.t("options.reset_keys")).clicked() {
            h.settings.bindings = crate::input::default_bindings();
        }
        if ui.button(c.t("menu.back")).clicked() {
            *next = Some(if playing { Some(MenuPage::Pause) } else { None });
        }
    });
    if !playing {
        ui.label(
            RichText::new(c.t("options.rebind_in_game"))
                .size(10.5)
                .color(c32(theme::TEXT_MUTED)),
        );
    }
    let changed = serde_json::to_string(&before).ok() != serde_json::to_string(&*h.settings).ok();
    if changed {
        apply_display(h, &before);
        persist::save_settings(&h.settings);
    }
}

fn apply_display(h: &mut Hud, before: &Settings) {
    if before.fullscreen == h.settings.fullscreen && before.resolution == h.settings.resolution {
        return;
    }
    if let Ok(mut w) = h.windows.single_mut() {
        w.mode = persist::window_mode(h.settings.fullscreen);
        let (x, y) = h.settings.resolution;
        w.resolution.set(x as f32, y as f32);
    }
}

pub fn goal_dialog(ctx: &egui::Context, h: &mut Hud) {
    if !h.ui.goal_dialog {
        return;
    }
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let s = world.stats().clone();
    let day = world.day();
    let mut done = false;
    window(ctx, c.t("goal.title"), 440.0, |ui| {
        ui.label(RichText::new(c.t("goal.body")).size(13.0));
        ui.add_space(6.0);
        ui.label(theme::mono(c.f(
            "goal.stats",
            &[
                &day.to_string(),
                &s.succeeded.to_string(),
                &s.died.to_string(),
                &s.hires.to_string(),
                &s.built.to_string(),
            ],
        )));
        ui.add_space(6.0);
        ui.label(
            RichText::new(c.t("goal.thanks"))
                .italics()
                .color(c32(theme::TEXT_SECONDARY)),
        );
        if ui.button(c.t("goal.keep_playing")).clicked() {
            done = true;
        }
    });
    if done {
        h.ui.goal_dialog = false;
    }
}

/// First-run welcome: what the game is, and the demo-save promise, said once in-app.
pub fn intro(ctx: &egui::Context, h: &mut Hud) {
    if h.settings.seen_intro || h.ui.modal_open() {
        return;
    }
    let c = h.content.clone();
    let mut done = false;
    window(ctx, c.t("intro.title"), 480.0, |ui| {
        ui.label(RichText::new(c.t("intro.body")).size(13.0));
        ui.add_space(6.0);
        ui.label(
            RichText::new(c.t("intro.controls"))
                .size(12.0)
                .color(c32(theme::TEXT_SECONDARY)),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(c.t("menu.demo_note"))
                .size(11.0)
                .italics()
                .color(c32(theme::TEXT_MUTED)),
        );
        if ui.button(c.t("intro.begin")).clicked() {
            done = true;
        }
    });
    if done {
        h.settings.seen_intro = true;
        persist::save_settings(&h.settings);
    }
}
