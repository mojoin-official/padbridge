//! Colours and widget styling for the app (an original dark theme).

use eframe::egui::{self, Color32, Stroke};

pub const BG: Color32 = Color32::from_rgb(17, 19, 24);
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 34);
pub const CARD: Color32 = Color32::from_rgb(32, 36, 45);
pub const CARD_HI: Color32 = Color32::from_rgb(44, 50, 62);
pub const LINE: Color32 = Color32::from_rgb(58, 64, 78);
pub const TEXT: Color32 = Color32::from_rgb(222, 226, 234);
pub const TEXT_DIM: Color32 = Color32::from_rgb(140, 148, 164);
/// Primary accent: mapped things, the live dot, primary buttons.
pub const ACCENT: Color32 = Color32::from_rgb(78, 214, 168);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(28, 74, 62);
pub const ACCENT_HI: Color32 = Color32::from_rgb(36, 96, 80);
/// Selection outline.
pub const SELECT: Color32 = Color32::from_rgb(255, 190, 70);
pub const DANGER: Color32 = Color32::from_rgb(236, 96, 96);
pub const DANGER_DIM: Color32 = Color32::from_rgb(92, 36, 40);
pub const OK_DIM: Color32 = Color32::from_rgb(26, 70, 56);
pub const BODY: Color32 = Color32::from_rgb(38, 42, 52);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = CARD;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = ACCENT_HI;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.window_stroke = Stroke::new(1.0_f32, LINE);

    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.weak_bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.inactive.bg_fill = CARD_HI;
    v.widgets.inactive.weak_bg_fill = CARD_HI;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);

    v.widgets.hovered.bg_fill = Color32::from_rgb(56, 63, 78);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(56, 63, 78);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, TEXT);

    v.widgets.active.bg_fill = ACCENT_HI;
    v.widgets.active.weak_bg_fill = ACCENT_HI;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, TEXT);

    v.widgets.open.bg_fill = CARD_HI;
    v.widgets.open.weak_bg_fill = CARD_HI;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.slider_width = 180.0;
    style.spacing.interact_size.y = 24.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    ctx.set_style(style);
}

/// A rounded card frame used for grouped settings.
pub fn card() -> egui::Frame {
    egui::Frame::default()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, LINE))
        .rounding(8.0)
        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
}

/// Section title inside a card.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().size(15.0).color(TEXT));
}

/// Dim help text.
pub fn help(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.5).color(TEXT_DIM));
}
