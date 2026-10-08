//! The visual language, from V2's DESIGN.md: "a ledger, not parchment". Cream surfaces, ruled
//! rows, one ink, one accent, tabular numerals, no ornament. Never pure black or white.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

pub const INK: [u8; 3] = [0x22, 0x20, 0x1B];
pub const ACCENT: [u8; 3] = [0x8C, 0x2F, 0x26];
pub const PAGE: [u8; 3] = [0xF2, 0xF0, 0xEA];
pub const SURFACE: [u8; 3] = [0xFA, 0xF9, 0xF5];
pub const SUNK: [u8; 3] = [0xEA, 0xE7, 0xDD];
pub const RULE: [u8; 3] = [0xD5, 0xD1, 0xC4];
pub const RULE_ROW: [u8; 3] = [0xE0, 0xDC, 0xD0];
pub const RULE_MID: [u8; 3] = [0xC6, 0xC2, 0xB4];
pub const TEXT_SECONDARY: [u8; 3] = [0x6E, 0x6A, 0x5F];
pub const TEXT_MUTED: [u8; 3] = [0x8C, 0x88, 0x7C];
pub const TEXT_FAINT: [u8; 3] = [0xA2, 0x9E, 0x92];
pub const GAIN: [u8; 3] = [0x1E, 0x43, 0x35];
pub const WARN: [u8; 3] = [0x9A, 0x5A, 0x12];

/// Branch tints for adventurer capsules: muted, readable on the cream floor.
pub const BRANCH: [[u8; 3]; 4] = [
    [0x5B, 0x6E, 0x8C], // fighter: slate blue
    [0x4F, 0x7A, 0x52], // rogue: moss
    [0x7A, 0x55, 0x8C], // mage: plum
    [0xB0, 0x8A, 0x3C], // holy: ochre
];

pub fn c32(c: [u8; 3]) -> egui::Color32 {
    egui::Color32::from_rgb(c[0], c[1], c[2])
}

pub fn bevy_color(c: [u8; 3]) -> Color {
    Color::srgb_u8(c[0], c[1], c[2])
}

pub fn scrim() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0x22, 0x20, 0x1B, 107)
}

#[derive(Default)]
pub struct Applied(u32);

/// Installs the ledger style into egui once (and again after a UI-scale change).
pub fn apply_once(
    mut contexts: EguiContexts,
    mut applied: Local<Applied>,
    settings: Res<crate::persist::Settings>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    let stamp = (settings.ui_scale * 100.0) as u32 + 1;
    if applied.0 == stamp {
        return;
    }
    applied.0 = stamp;
    ctx.set_zoom_factor(settings.ui_scale);
    let mut v = egui::Visuals::light();
    v.override_text_color = Some(c32(INK));
    v.panel_fill = c32(SURFACE);
    v.window_fill = c32(SURFACE);
    v.extreme_bg_color = c32(PAGE);
    v.faint_bg_color = c32(SUNK);
    v.window_stroke = egui::Stroke::new(1.0, c32(RULE_MID));
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_corner_radius = egui::CornerRadius::ZERO;
    v.menu_corner_radius = egui::CornerRadius::ZERO;
    v.selection.bg_fill = c32(SUNK);
    v.selection.stroke = egui::Stroke::new(2.0, c32(ACCENT));
    v.hyperlink_color = c32(ACCENT);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = egui::CornerRadius::ZERO;
        w.fg_stroke = egui::Stroke::new(1.0, c32(INK));
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, c32(RULE));
    v.widgets.noninteractive.bg_fill = c32(SURFACE);
    // Rails, check boxes and tracks: a sunk fill so they read against the surface. Buttons
    // use the weak fill and stay cream.
    v.widgets.inactive.bg_fill = c32(RULE);
    v.widgets.inactive.weak_bg_fill = c32(SURFACE);
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, c32(RULE_MID));
    v.widgets.hovered.bg_fill = c32(SUNK);
    v.widgets.hovered.weak_bg_fill = c32(SUNK);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, c32(INK));
    // Pressed is a sunk fill with the accent edge; strong text reads this colour, so it stays ink.
    v.widgets.active.bg_fill = c32(RULE);
    v.widgets.active.weak_bg_fill = c32(RULE);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, c32(INK));
    v.widgets.active.bg_stroke = egui::Stroke::new(2.0, c32(ACCENT));
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.slider_width = 200.0;
        use egui::{FontFamily, FontId, TextStyle};
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
            (
                TextStyle::Button,
                FontId::new(12.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(10.5, FontFamily::Proportional),
            ),
        ]
        .into();
    });
}

/// A small upper-case section label (DESIGN.md: 9-10px, tracked, muted).
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.0)
            .color(c32(TEXT_MUTED))
            .strong(),
    );
}

/// The double rule under panel and sheet heads: 1px mid rule, 2px gap, 2px ink.
pub fn double_rule(ui: &mut egui::Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 5.0), egui::Sense::hover());
    let p = ui.painter();
    p.hline(
        rect.x_range(),
        rect.top() + 0.5,
        egui::Stroke::new(1.0, c32(RULE_MID)),
    );
    p.hline(
        rect.x_range(),
        rect.top() + 4.0,
        egui::Stroke::new(2.0, c32(INK)),
    );
}

pub fn row_rule(ui: &mut egui::Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1.0, c32(RULE_ROW)),
    );
}

pub fn mono(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).monospace()
}

pub fn heading(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text.into()).size(13.0).strong()
}

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(c32(SURFACE))
        .inner_margin(egui::Margin::same(12))
        .stroke(egui::Stroke::new(1.0, c32(RULE)))
}
