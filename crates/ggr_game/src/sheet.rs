//! The one modal detail sheet, shared by characters, candidates, quests and buildings. Links
//! inside it go one level deep; Back returns to the linking sheet, Esc goes back then closes.

use bevy_egui::egui;
use egui::RichText;
use ggr_content::{Attr, ItemSlot, LegKind, PrefabKind};
use ggr_sim::{
    CharState, Command, Consequence, InstanceStatus, OffMapReason, Outcome, QuestStatus,
};

use crate::hud::{bonus_text, duration_text, run, state_text, Hud};
use crate::state::*;
use crate::theme::{self, c32};

enum Action {
    None,
    Close,
    Back,
    Link(Sheet),
    Cmd(Command),
    CmdThenClose(Command),
}

pub fn draw(ctx: &egui::Context, h: &mut Hud) {
    let Some(&top) = h.ui.sheet.last() else {
        return;
    };
    let back_to = if h.ui.sheet.len() > 1 {
        Some(h.ui.sheet[0])
    } else {
        None
    };
    let mut action = Action::None;
    let frame = egui::Frame::new()
        .fill(c32(theme::SURFACE))
        .inner_margin(egui::Margin::same(22))
        .stroke(egui::Stroke::new(1.0, c32(theme::RULE_MID)));
    let resp = egui::Modal::new(egui::Id::new("detail-sheet"))
        .frame(frame)
        .backdrop_color(theme::scrim())
        .show(ctx, |ui| {
            ui.set_width(600.0);
            let max_h = ctx.viewport_rect().height() - 140.0;
            egui::ScrollArea::vertical()
                .max_height(max_h)
                .show(ui, |ui| {
                    // Keep right-aligned values clear of the scrollbar.
                    ui.set_max_width(586.0);
                    action = match top {
                        Sheet::Character(id) => character(ui, h, id),
                        Sheet::Candidate(id) => candidate(ui, h, id),
                        Sheet::Quest(id) => quest(ui, h, id),
                        Sheet::Instance(id) => instance(ui, h, id),
                    };
                });
            theme::double_rule(ui);
            ui.horizontal(|ui| {
                if let Some(b) = back_to {
                    let name = sheet_name(h, b);
                    if ui.button(h.content.f("sheet.back_to", &[&name])).clicked() {
                        action = Action::Back;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(h.content.t("sheet.close")).clicked() {
                        action = Action::Close;
                    }
                    ui.label(
                        RichText::new(h.content.t("sheet.esc_hint"))
                            .size(10.5)
                            .color(c32(theme::TEXT_FAINT)),
                    );
                });
            });
        });
    if resp.should_close() && matches!(action, Action::None) {
        action = Action::Close;
    }
    match action {
        Action::None => {}
        Action::Close => h.ui.sheet.clear(),
        Action::Back => {
            h.ui.sheet.pop();
        }
        Action::Link(s) => h.ui.link_sheet(s),
        Action::Cmd(c) => {
            run(h, c);
        }
        Action::CmdThenClose(c) => {
            if run(h, c) {
                h.ui.sheet.clear();
            }
        }
    }
}

fn sheet_name(h: &Hud, s: Sheet) -> String {
    let world = h.session.world.as_ref().unwrap();
    match s {
        Sheet::Character(id) => world.characters()[id as usize].name(&h.content.0),
        Sheet::Quest(id) => world.quest_title(&world.quests()[id as usize]),
        Sheet::Candidate(_) => h.content.t("sheet.candidate").to_string(),
        Sheet::Instance(id) => {
            let p = world.instances()[id as usize].prefab as usize;
            h.content
                .t(&world.content().prefabs[p].name_key)
                .to_string()
        }
    }
}

fn head(ui: &mut egui::Ui, eyebrow: &str, title: &str, sub: &str) {
    ui.label(
        RichText::new(eyebrow.to_uppercase())
            .size(10.0)
            .color(c32(theme::TEXT_MUTED))
            .strong(),
    );
    ui.label(RichText::new(title).size(24.0));
    if !sub.is_empty() {
        ui.label(
            RichText::new(sub)
                .size(12.0)
                .color(c32(theme::TEXT_SECONDARY)),
        );
    }
    theme::double_rule(ui);
}

fn kv(ui: &mut egui::Ui, k: &str, v: impl Into<RichText>) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(k)
                .size(12.0)
                .color(c32(theme::TEXT_SECONDARY)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(v.into());
        });
    });
}

fn attr_grid(ui: &mut egui::Ui, h: &Hud, attrs: &[i32; 6], extra: Option<&dyn Fn(Attr) -> i32>) {
    let c = &h.content;
    egui::Grid::new(ui.next_auto_id())
        .num_columns(6)
        .spacing([18.0, 2.0])
        .show(ui, |ui| {
            for a in Attr::ALL {
                ui.label(
                    RichText::new(c.t(&c.0.attributes[a.index()].short_key))
                        .size(10.0)
                        .color(c32(theme::TEXT_MUTED)),
                );
            }
            ui.end_row();
            for a in Attr::ALL {
                let mut t = theme::mono(attrs[a.index()].to_string()).size(16.0);
                if let Some(f) = extra {
                    let b = f(a);
                    if b > 0 {
                        t = theme::mono(format!("{}+{b}", attrs[a.index()])).size(16.0);
                    }
                }
                ui.label(t);
            }
            ui.end_row();
        });
}

fn character(ui: &mut egui::Ui, h: &mut Hud, id: u32) -> Action {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let Some(ch) = world.characters().get(id as usize).cloned() else {
        return Action::Close;
    };
    let now = world.minute();
    let mut act = Action::None;
    let name = ch.name(&c.0);
    let state = state_text(world, &c, &ch);
    if let Some(st) = &ch.staff {
        let role = c.t(&c.0.roles[st.role as usize].name_key).to_string();
        head(ui, c.t("sheet.staff"), &name, &format!("{role} · {state}"));
        attr_grid(ui, h, &ch.attrs, None);

        theme::section_label(ui, c.t("sheet.shift"));
        let mut start = st.shift.start_hour;
        let mut len = st.shift.length;
        ui.horizontal(|ui| {
            ui.label(RichText::new(c.t("sheet.shift_length")).size(12.0));
            for l in c.0.shift_lengths.iter() {
                if ui
                    .selectable_label(len == *l, theme::mono(format!("{l}h")))
                    .clicked()
                {
                    len = *l;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(c.t("sheet.shift_start")).size(12.0));
            if ui.button("−").clicked() {
                start = (start + 23) % 24;
            }
            ui.label(
                theme::mono(format!("{start:02}:00 – {:02}:00", (start + len) % 24)).size(15.0),
            );
            if ui.button("+").clicked() {
                start = (start + 1) % 24;
            }
        });
        // A small 24-hour strip: the shift in ink, night hours shaded.
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 14.0), egui::Sense::hover());
        let cell = rect.width() / 24.0;
        for hr in 0..24u8 {
            let r = egui::Rect::from_min_size(
                rect.min + egui::vec2(hr as f32 * cell, 0.0),
                egui::vec2(cell - 1.0, 14.0),
            );
            let on = ggr_sim::Shift {
                start_hour: start,
                length: len,
            }
            .covers(i64::from(hr));
            let night = !(6..22).contains(&hr);
            let fill = match (on, night) {
                (true, true) => c32(theme::ACCENT),
                (true, false) => c32(theme::INK),
                (false, true) => c32(theme::SUNK),
                (false, false) => c32(theme::PAGE),
            };
            ui.painter().rect_filled(r, 0.0, fill);
        }
        ui.label(
            RichText::new(c.t("sheet.shift_legend"))
                .size(10.0)
                .color(c32(theme::TEXT_FAINT)),
        );
        if start != st.shift.start_hour || len != st.shift.length {
            act = Action::Cmd(Command::SetShift {
                character: id,
                start_hour: start,
                length: len,
            });
        }

        theme::section_label(ui, c.t("sheet.happiness"));
        let target = world.happiness_target(id);
        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(st.happiness as f32 / 100.0)
                    .desired_width(240.0)
                    .text(theme::mono(format!("{} → {target}", st.happiness))),
            );
            ui.label(
                RichText::new(c.f(
                    "sheet.work_speed",
                    &[&world.work_speed_percent(id).to_string()],
                ))
                .size(12.0),
            );
        });
        for m in world.happiness_modifiers(id) {
            let col = if m.value < 0 {
                c32(theme::ACCENT)
            } else if m.key == "happiness.base" {
                c32(theme::INK)
            } else {
                c32(theme::GAIN)
            };
            kv(
                ui,
                c.t(m.key),
                theme::mono(format!("{:+}", m.value)).color(col),
            );
        }
        ui.label(
            RichText::new(c.t("sheet.happiness_help"))
                .size(10.5)
                .italics()
                .color(c32(theme::TEXT_MUTED)),
        );

        theme::section_label(ui, c.t("sheet.terms"));
        let wage = c.0.roles[st.role as usize].wage_per_hour * i64::from(st.shift.length);
        kv(
            ui,
            c.t("sheet.wage"),
            theme::mono(c.f("sheet.per_day", &[&wage.to_string()])),
        );
        kv(
            ui,
            c.t("sheet.wages_paid"),
            theme::mono(format!("{} g", st.wages_paid)),
        );
        if st.unpaid_since >= 0 {
            ui.label(RichText::new(c.t("happiness.unpaid")).color(c32(theme::ACCENT)));
        }
        // Workstation choice.
        let kind = c.0.roles[st.role as usize].station;
        let stations: Vec<u32> = world
            .instances()
            .iter()
            .filter(|i| i.is_ready() && world.kind_of(i.id) == kind)
            .map(|i| i.id)
            .collect();
        if stations.len() > 1 {
            ui.horizontal(|ui| {
                ui.label(RichText::new(c.t("sheet.station")).size(12.0));
                let current = st.workstation;
                if ui
                    .selectable_label(current.is_none(), c.t("sheet.station_any"))
                    .clicked()
                    && current.is_some()
                {
                    act = Action::Cmd(Command::AssignWorkstation {
                        character: id,
                        instance: None,
                    });
                }
                for (n, s) in stations.iter().enumerate() {
                    if ui
                        .selectable_label(current == Some(*s), theme::mono(format!("#{}", n + 1)))
                        .clicked()
                    {
                        act = Action::Cmd(Command::AssignWorkstation {
                            character: id,
                            instance: Some(*s),
                        });
                    }
                }
            });
        } else if stations.is_empty() {
            ui.label(
                RichText::new(c.t("sheet.no_station"))
                    .size(11.0)
                    .color(c32(theme::WARN)),
            );
        }
        if ch.on_roster() {
            ui.add_space(8.0);
            if ui
                .button(c.t("sheet.dismiss"))
                .on_hover_text(c.t("sheet.dismiss_tip"))
                .clicked()
            {
                act = Action::CmdThenClose(Command::Dismiss { character: id });
            }
        }
        return act;
    }

    let adv = ch.adv.clone().unwrap();
    let class = &c.0.classes[adv.class as usize];
    let rank = &c.0.ranks[ch.rank as usize];
    head(
        ui,
        c.t("sheet.adventurer"),
        &name,
        &format!(
            "{} · {} · {}",
            c.f("sheet.rank", &[&rank.letter]),
            c.t(&class.name_key),
            state
        ),
    );
    // Promotion offer, first: it is a decision with a deadline.
    if let Some(p) = world.promotions().iter().find(|p| p.character == id) {
        egui::Frame::new()
            .fill(c32(theme::PAGE))
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(c.f(
                        "sheet.promotion_offer",
                        &[&c.0.ranks[p.to_rank as usize].letter],
                    ))
                    .size(14.0)
                    .strong()
                    .color(c32(theme::GAIN)),
                );
                ui.label(
                    RichText::new(c.t("sheet.promotion_help"))
                        .size(11.0)
                        .color(c32(theme::TEXT_SECONDARY)),
                );
                kv(
                    ui,
                    c.t("candidate.fee"),
                    theme::mono(format!("{} g", p.fee)),
                );
                kv(
                    ui,
                    c.t("candidate.leaves_in"),
                    theme::mono(duration_text(&c, p.expires - now)),
                );
                let can = world.desk_can_sign();
                if !can {
                    ui.label(RichText::new(c.t("refusal.desk_unmanned")).color(c32(theme::ACCENT)));
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            can && world.guild().gold >= p.fee,
                            egui::Button::new(c.f("sheet.promote", &[&p.fee.to_string()])),
                        )
                        .clicked()
                    {
                        act = Action::Cmd(Command::AcceptPromotion { character: id });
                    }
                    if ui.button(c.t("sheet.not_now")).clicked() {
                        act = Action::Cmd(Command::DeclinePromotion { character: id });
                    }
                });
            });
    }
    theme::section_label(ui, c.t("sheet.attributes"));
    let breakdown = |a: Attr| world.check_breakdown(id, a).gear_capped;
    attr_grid(ui, h, &ch.attrs, Some(&breakdown));
    ui.label(
        RichText::new(c.f("sheet.cap", &[&rank.modifier_cap.to_string()]))
            .size(10.5)
            .color(c32(theme::TEXT_MUTED)),
    );

    theme::section_label(ui, c.t("sheet.condition"));
    if ch.state == CharState::Dead {
        ui.label(RichText::new(c.t("state.dead")).color(c32(theme::ACCENT)));
    } else if ch.is_injured(now) {
        kv(
            ui,
            c.t("sheet.injured_for"),
            theme::mono(duration_text(&c, ch.injured_until - now)).color(c32(theme::ACCENT)),
        );
    } else {
        kv(
            ui,
            c.t("sheet.health"),
            RichText::new(c.t("sheet.well")).color(c32(theme::GAIN)),
        );
    }
    let hunger_col = if ch.hunger < c.0.rules.hunger_starving_below {
        c32(theme::ACCENT)
    } else if ch.hunger < c.0.rules.hunger_hungry_below {
        c32(theme::WARN)
    } else {
        c32(theme::INK)
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(c.t("sheet.hunger"))
                .size(12.0)
                .color(c32(theme::TEXT_SECONDARY)),
        );
        ui.add(
            egui::ProgressBar::new(ch.hunger as f32 / 100.0)
                .desired_width(160.0)
                .fill(hunger_col),
        );
    });

    theme::section_label(ui, c.t("sheet.record"));
    kv(ui, c.t("sheet.experience"), theme::mono(adv.xp.to_string()));
    kv(
        ui,
        c.t("sheet.quests"),
        theme::mono(c.f(
            "sheet.quests_value",
            &[
                &adv.quests_succeeded.to_string(),
                &adv.quests_failed.to_string(),
            ],
        )),
    );
    kv(
        ui,
        c.t("sheet.earned"),
        theme::mono(format!("{} g", adv.gold_earned)),
    );
    if (ch.rank as usize) < c.0.demo_rank_cap {
        kv(
            ui,
            c.t("sheet.next_rank"),
            theme::mono(c.f(
                "sheet.next_rank_value",
                &[
                    &adv.quests_at_rank.to_string(),
                    &rank.promote_quests.to_string(),
                    &adv.xp.to_string(),
                    &rank.promote_xp.to_string(),
                ],
            )),
        );
    }
    if let Some(q) = ch.quest {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(c.t("sheet.away_on"))
                    .size(12.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            if ui
                .link(world.quest_title(&world.quests()[q as usize]))
                .clicked()
            {
                act = Action::Link(Sheet::Quest(q));
            }
        });
    } else if let Some(last) = world
        .quests()
        .iter()
        .rev()
        .find(|q| q.taker == Some(id) && q.outcome().is_some())
    {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(c.t("sheet.last_quest"))
                    .size(12.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            if ui.link(world.quest_title(last)).clicked() {
                act = Action::Link(Sheet::Quest(last.id));
            }
        });
    }

    // Skills, with aptitude by branch.
    theme::section_label(ui, c.t("sheet.skills"));
    egui::Grid::new("skills")
        .num_columns(3)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            for (i, s) in c.0.skills.iter().enumerate() {
                let lvl = world.skill_level(id, i);
                let xp = adv.skill_xp[i];
                let next = c.0.skill_levels.get(lvl as usize).copied();
                let prev = if lvl == 0 {
                    0
                } else {
                    c.0.skill_levels[lvl as usize - 1]
                };
                ui.label(RichText::new(c.t(&s.name_key)).size(12.0));
                ui.label(theme::mono(c.f("sheet.level", &[&lvl.to_string()])));
                let frac = next.map_or(1.0, |n| (xp - prev) as f32 / (n - prev).max(1) as f32);
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(130.0)
                        .desired_height(8.0),
                );
                ui.end_row();
            }
        });
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(c.t("sheet.aptitude"))
                .size(11.0)
                .color(c32(theme::TEXT_SECONDARY)),
        );
        for (b, br) in c.0.branches.iter().enumerate() {
            let a = adv.aptitude[b];
            let col = if a >= 110 {
                c32(theme::GAIN)
            } else if a <= 80 {
                c32(theme::ACCENT)
            } else {
                c32(theme::INK)
            };
            ui.label(
                RichText::new(format!("{} {}%", c.t(&br.name_key), a))
                    .size(11.0)
                    .color(col),
            );
        }
    })
    .response
    .on_hover_text(c.t("sheet.aptitude_tip"));

    // Aspiration and what's missing.
    if ch.alive() {
        theme::section_label(ui, c.t("sheet.aspiration"));
        let current = adv.aspiration;
        let label = current.map_or(c.t("sheet.aspire_none").to_string(), |k| {
            c.t(&c.0.classes[k as usize].name_key).to_string()
        });
        let mut choice = current;
        egui::ComboBox::from_id_salt("aspire")
            .selected_text(label)
            .width(240.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, c.t("sheet.aspire_none"));
                for (k, cl) in c.0.classes.iter().enumerate() {
                    if cl.tier == 0 || k == adv.class as usize {
                        continue;
                    }
                    let br = c.t(&c.0.branches[cl.branch].name_key);
                    ui.selectable_value(
                        &mut choice,
                        Some(k as u8),
                        format!("{} ({br}, {}%)", c.t(&cl.name_key), adv.aptitude[cl.branch]),
                    )
                    .on_hover_text(c.t(&cl.desc_key));
                }
            });
        if choice != current {
            act = Action::Cmd(Command::SetAspiration {
                character: id,
                class: choice,
            });
        }
        if let Some(k) = current {
            let cl = &c.0.classes[k as usize];
            ui.label(
                RichText::new(c.t(&cl.desc_key))
                    .size(11.0)
                    .italics()
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            ui.label(RichText::new(bonus_text(&c, &cl.bonus, false)).size(11.0));
            for p in world.class_progress(id, k as usize) {
                let label = match p.skill {
                    Some(s) => c.f(
                        "sheet.req_skill",
                        &[c.t(&c.0.skills[s].name_key), &p.needed.to_string()],
                    ),
                    None => c.f("sheet.req_rank", &[&c.0.ranks[p.needed as usize].letter]),
                };
                let short = p.needed - p.have;
                let (col, mark) = if short <= 0 {
                    (c32(theme::GAIN), "✔")
                } else if short <= 1 {
                    (c32(theme::WARN), "·")
                } else {
                    (c32(theme::ACCENT), "✗")
                };
                kv(
                    ui,
                    &label,
                    theme::mono(format!("{mark} {}/{}", p.have, p.needed)).color(col),
                );
            }
        }
    }

    // Equipment.
    theme::section_label(ui, c.t("sheet.equipment"));
    let away = ch.state == CharState::OffMap(OffMapReason::Quest);
    for slot in ItemSlot::ALL {
        let item = adv.equipment[slot.index()];
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(c.t(slot.key()))
                    .size(11.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(i) = item {
                    if ui
                        .add_enabled(!away && ch.alive(), egui::Button::new(c.t("sheet.unequip")))
                        .clicked()
                    {
                        act = Action::Cmd(Command::Unequip {
                            character: id,
                            slot,
                        });
                    }
                    let it = &c.0.items[i as usize];
                    ui.label(
                        RichText::new(format!(
                            "{}  ({})",
                            c.t(&it.name_key),
                            bonus_text(&c, &it.bonus, it.two_handed)
                        ))
                        .size(12.0),
                    );
                } else {
                    // Equip from the stash: only what fits this slot.
                    let options: Vec<usize> =
                        c.0.items
                            .iter()
                            .enumerate()
                            .filter(|(n, it)| it.slot == slot && world.guild().stash[*n] > 0)
                            .map(|(n, _)| n)
                            .collect();
                    if !options.is_empty() && !away && ch.alive() {
                        egui::ComboBox::from_id_salt(("equip", slot.index()))
                            .selected_text(c.t("sheet.equip"))
                            .width(170.0)
                            .show_ui(ui, |ui| {
                                for n in options {
                                    let it = &c.0.items[n];
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!(
                                                "{} ({})",
                                                c.t(&it.name_key),
                                                bonus_text(&c, &it.bonus, it.two_handed)
                                            ),
                                        )
                                        .clicked()
                                    {
                                        act = Action::Cmd(Command::Equip {
                                            character: id,
                                            item: n,
                                        });
                                    }
                                }
                            });
                    } else {
                        ui.label(RichText::new("—").color(c32(theme::TEXT_FAINT)));
                    }
                }
            });
        });
    }
    if ch.on_roster() && ch.quest.is_none() {
        ui.add_space(8.0);
        if ui
            .button(c.t("sheet.dismiss"))
            .on_hover_text(c.t("sheet.dismiss_tip"))
            .clicked()
        {
            act = Action::CmdThenClose(Command::Dismiss { character: id });
        }
    }
    act
}

fn candidate(ui: &mut egui::Ui, h: &mut Hud, id: u32) -> Action {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let Some(cand) = world.candidates().iter().find(|x| x.id == id).cloned() else {
        ui.label(RichText::new(c.t("refusal.candidate_gone")).italics());
        return Action::None;
    };
    let now = world.minute();
    let mut act = Action::None;
    let what = match cand.staff_role {
        Some(r) => c.t(&c.0.roles[r as usize].name_key).to_string(),
        None => c.f(
            "desk.adventurer_rank",
            &[&c.0.ranks[cand.rank as usize].letter],
        ),
    };
    head(ui, c.t("sheet.candidate"), &cand.name(&c.0), &what);
    theme::section_label(ui, c.t("sheet.attributes"));
    attr_grid(ui, h, &cand.attrs, None);
    if cand.staff_role.is_none() {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(c.t("sheet.aptitude"))
                    .size(11.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            for (b, br) in c.0.branches.iter().enumerate() {
                ui.label(
                    RichText::new(format!("{} {}%", c.t(&br.name_key), cand.aptitude[b]))
                        .size(11.0),
                );
            }
        });
    } else if let Some(r) = cand.staff_role {
        let role = &c.0.roles[r as usize];
        kv(
            ui,
            c.t("sheet.wage"),
            theme::mono(c.f("sheet.wage_rate", &[&role.wage_per_hour.to_string()])),
        );
        ui.label(
            RichText::new(c.t("candidate.staff_help"))
                .size(11.0)
                .italics()
                .color(c32(theme::TEXT_SECONDARY)),
        );
    }
    theme::section_label(ui, c.t("sheet.terms"));
    let gold = world.guild().gold;
    kv(
        ui,
        c.t("candidate.fee"),
        theme::mono(format!("{} g", cand.cost)).color(if cand.cost > gold {
            c32(theme::ACCENT)
        } else {
            c32(theme::INK)
        }),
    );
    kv(
        ui,
        c.t("candidate.leaves_in"),
        theme::mono(duration_text(&c, cand.expires - now)),
    );
    kv(
        ui,
        c.t("candidate.treasury_now"),
        theme::mono(format!("{gold} g")),
    );
    kv(
        ui,
        c.t("candidate.treasury_after"),
        theme::mono(format!("{} g", gold - cand.cost)),
    );
    if cand.volunteer {
        ui.label(
            RichText::new(c.t("candidate.volunteer"))
                .italics()
                .color(c32(theme::GAIN)),
        );
    }
    let blocker = if !world.desk_can_sign() {
        Some(c.t("refusal.desk_unmanned"))
    } else if cand.cost > gold {
        Some(c.t("refusal.gold"))
    } else {
        None
    };
    if let Some(b) = blocker {
        ui.label(RichText::new(b).color(c32(theme::ACCENT)));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                blocker.is_none(),
                egui::Button::new(c.f("candidate.accept", &[&cand.cost.to_string()])),
            )
            .clicked()
        {
            act = Action::CmdThenClose(Command::Hire { candidate: id });
        }
        if ui.button(c.t("candidate.decline")).clicked() {
            act = Action::CmdThenClose(Command::Decline { candidate: id });
        }
    });
    act
}

fn quest(ui: &mut egui::Ui, h: &mut Hud, id: u32) -> Action {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let Some(q) = world.quests().get(id as usize).cloned() else {
        return Action::Close;
    };
    let now = world.minute();
    let mut act = Action::None;
    let status = match q.status {
        QuestStatus::Posted => c.t("quest.status.posted").to_string(),
        QuestStatus::Taken => c.t("quest.status.taken").to_string(),
        QuestStatus::Underway => c.t("quest.status.underway").to_string(),
        QuestStatus::Resolved(o) => c
            .t(match o {
                Outcome::Succeeded => "outcome.succeeded",
                Outcome::Failed => "outcome.failed",
                Outcome::Died => "outcome.died",
                Outcome::Expired => "outcome.expired",
            })
            .to_string(),
    };
    head(
        ui,
        &c.f("quest.eyebrow", &[&c.0.ranks[q.rank as usize].letter]),
        &world.quest_title(&q),
        &status,
    );
    ui.label(
        RichText::new(world.quest_description(&q))
            .size(13.0)
            .italics(),
    );

    theme::section_label(ui, c.t("quest.requirements"));
    kv(
        ui,
        &c.f("quest.rank_at_least", &[&c.0.ranks[q.rank as usize].letter]),
        RichText::new(""),
    );
    kv(ui, c.t("quest.not_injured"), RichText::new(""));
    theme::section_label(ui, c.t("quest.rewards"));
    kv(
        ui,
        c.t("quest.gold"),
        theme::mono(format!("{} – {} g", q.gold.0, q.gold.1)).color(c32(theme::GAIN)),
    );
    kv(ui, c.t("quest.experience"), theme::mono(q.xp.to_string()));
    if !q.loot.is_empty() {
        let loot: Vec<String> = q
            .loot
            .iter()
            .map(|(i, p)| format!("{} {p}%", c.t(&c.0.items[*i as usize].name_key)))
            .collect();
        kv(
            ui,
            c.t("quest.loot"),
            RichText::new(loot.join(", ")).size(12.0),
        );
    }
    if let Some(t) = q.taker {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(c.t("quest.taken_by"))
                    .size(12.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            if ui.link(world.characters()[t as usize].name(&c.0)).clicked() {
                act = Action::Link(Sheet::Character(t));
            }
        });
    }
    match q.status {
        QuestStatus::Posted => kv(
            ui,
            c.t("quest.expires"),
            theme::mono(duration_text(&c, q.expires - now)),
        ),
        QuestStatus::Underway => kv(
            ui,
            c.t("quest.due_back"),
            theme::mono(duration_text(&c, q.due_back - now)),
        ),
        QuestStatus::Resolved(Outcome::Succeeded) => {
            kv(
                ui,
                c.t("quest.paid"),
                theme::mono(format!("{} g", q.gold_won)).color(c32(theme::GAIN)),
            );
            for i in &q.loot_won {
                kv(
                    ui,
                    c.t("quest.found"),
                    RichText::new(c.t(&c.0.items[*i as usize].name_key)),
                );
            }
        }
        _ => {}
    }

    theme::section_label(ui, c.t("quest.itinerary"));
    let reveal = q.status == QuestStatus::Underway || q.outcome().is_some();
    egui::Grid::new("legs")
        .num_columns(4)
        .spacing([12.0, 3.0])
        .show(ui, |ui| {
            for (i, leg) in q.legs.iter().enumerate() {
                let kind = c.t(match leg.kind {
                    LegKind::Travel => "leg.travel",
                    LegKind::Encounter => "leg.encounter",
                    LegKind::Objective => "leg.objective",
                    LegKind::Return => "leg.return",
                });
                ui.label(RichText::new(kind).size(12.0).strong());
                ui.label(
                    theme::mono(duration_text(&c, leg.minutes)).color(c32(theme::TEXT_SECONDARY)),
                );
                ui.label(
                    RichText::new(c.f(
                        "quest.check",
                        &[
                            c.t(&c.0.attributes[leg.attr.index()].name_key),
                            &leg.target.to_string(),
                        ],
                    ))
                    .size(12.0),
                );
                // The printable reason: roll + attribute + gear + food against the target.
                match q.report.get(i) {
                    Some(r) if reveal && r.resolved => {
                        let mut text = format!("{} + {} + {}", r.roll, r.attr_value, r.gear);
                        if r.food != 0 {
                            text.push_str(&format!(" {:+}", r.food));
                        }
                        text.push_str(&format!(" = {} ", r.total()));
                        let (mark, col) = if r.passed {
                            ("✔", c32(theme::GAIN))
                        } else {
                            ("✗", c32(theme::ACCENT))
                        };
                        let cons = match r.consequence {
                            Consequence::None => String::new(),
                            Consequence::Injury => format!(" · {}", c.t("quest.injury")),
                            Consequence::Death => format!(" · {}", c.t("quest.death")),
                        };
                        ui.label(theme::mono(format!("{text}{mark}{cons}")).color(col))
                            .on_hover_text(c.t("quest.report_tip"));
                    }
                    Some(_) if reveal => {
                        ui.label(
                            RichText::new(c.t("quest.ahead"))
                                .size(11.0)
                                .italics()
                                .color(c32(theme::TEXT_FAINT)),
                        );
                    }
                    _ => {
                        ui.label("");
                    }
                }
                ui.end_row();
            }
        });
    if q.is_posted() {
        ui.label(
            RichText::new(c.t("quest.board_help"))
                .size(11.0)
                .italics()
                .color(c32(theme::TEXT_MUTED)),
        );
    }
    act
}

fn instance(ui: &mut egui::Ui, h: &mut Hud, id: u32) -> Action {
    let c = h.content.clone();
    let world = h.session.world.as_ref().unwrap();
    let Some(inst) = world.instances().get(id as usize).cloned() else {
        return Action::Close;
    };
    let now = world.minute();
    let p = &c.0.prefabs[inst.prefab as usize];
    let mut act = Action::None;
    let status = match inst.status {
        InstanceStatus::Ready => c.t("building.ready").to_string(),
        InstanceStatus::UnderConstruction { done_at } => c.f(
            "building.constructing",
            &[&duration_text(&c, done_at - now)],
        ),
        InstanceStatus::Demolished => c.t("building.demolished").to_string(),
    };
    head(ui, c.t("sheet.building"), c.t(&p.name_key), &status);
    ui.label(RichText::new(c.t(&p.desc_key)).size(13.0).italics());
    if !p.skills.is_empty() {
        let names: Vec<&str> = p
            .skills
            .iter()
            .map(|s| c.t(&c.0.skills[*s].name_key))
            .collect();
        kv(ui, c.t("building.trains"), RichText::new(names.join(", ")));
    }
    if let Some(staff) = inst.slots.iter().find(|s| s.staff) {
        let who = staff.holder.filter(|_| staff.occupied);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(c.t("building.staffed_by"))
                    .size(12.0)
                    .color(c32(theme::TEXT_SECONDARY)),
            );
            match who {
                Some(w) => {
                    if ui.link(world.characters()[w as usize].name(&c.0)).clicked() {
                        act = Action::Link(Sheet::Character(w));
                    }
                }
                None => {
                    ui.label(RichText::new(c.t("building.unstaffed")).color(c32(theme::ACCENT)));
                }
            }
        });
    }
    let users: Vec<u32> = inst
        .slots
        .iter()
        .filter(|s| !s.staff)
        .filter_map(|s| s.holder)
        .collect();
    let customer_slots = inst.slots.iter().filter(|s| !s.staff).count();
    if customer_slots > 0 {
        kv(
            ui,
            c.t("building.in_use"),
            theme::mono(format!("{}/{}", users.len(), customer_slots)),
        );
        for u in users {
            if ui.link(world.characters()[u as usize].name(&c.0)).clicked() {
                act = Action::Link(Sheet::Character(u));
            }
        }
    }
    if matches!(p.kind, PrefabKind::Desk | PrefabKind::Board) {
        ui.label(
            RichText::new(c.t("building.essential"))
                .size(11.0)
                .italics()
                .color(c32(theme::TEXT_MUTED)),
        );
    }
    if inst.status != InstanceStatus::Demolished {
        let refund = match inst.status {
            InstanceStatus::UnderConstruction { .. } => inst.paid,
            _ => inst.paid * c.0.rules.demolish_refund_percent / 100,
        };
        ui.add_space(8.0);
        if ui
            .button(c.f("building.demolish", &[&refund.to_string()]))
            .on_hover_text(c.t("build.demolish_tip"))
            .clicked()
        {
            act = Action::CmdThenClose(Command::Demolish { instance: id });
        }
    }
    act
}
