//! Look and feel: two original "glass" themes over a soft gradient backdrop.
//!
//! * **Aurora** (dark): frosted panels over an indigo-to-violet gradient with teal, blue and pink
//!   glows.
//! * **Frost** (light): airy white glass over a pale blue-grey gradient with blue glows.
//!
//! egui has no backdrop blur, so the glass effect is built from translucent fills, thin bright
//! borders, soft shadows and large, blurred-looking radial glows painted behind everything.

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Theme {
    Aurora,
    Frost,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Aurora, Theme::Frost];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Aurora => "Aurora (dark glass)",
            Theme::Frost => "Frost (light glass)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Theme::Aurora => "aurora",
            Theme::Frost => "frost",
        }
    }

    pub fn from_key(s: &str) -> Option<Theme> {
        match s.trim().to_ascii_lowercase().as_str() {
            "aurora" | "dark" => Some(Theme::Aurora),
            "frost" | "light" => Some(Theme::Frost),
            _ => None,
        }
    }
}

pub struct Palette {
    pub dark: bool,
    pub bg_top: Color32,
    pub bg_bottom: Color32,
    /// (x fraction, y fraction, radius fraction of the window diagonal, colour)
    pub glows: [(f32, f32, f32, Color32); 3],
    /// Side panels.
    pub panel: Color32,
    /// Top and bottom bars.
    pub bar: Color32,
    pub card: Color32,
    pub card_hi: Color32,
    pub line: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub accent_hi: Color32,
    /// Text drawn on an accent fill.
    pub on_accent: Color32,
    pub select: Color32,
    pub danger: Color32,
    pub danger_dim: Color32,
    pub ok_dim: Color32,
    /// Mouse / controller body.
    pub body: Color32,
    /// Recessed areas: graphs, text fields, notes.
    pub inset: Color32,
    /// Menus and dialogs (nearly opaque so text stays readable).
    pub popup: Color32,
    pub shadow: Color32,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    // premultiplied, as egui stores colours
    Color32::from_rgba_premultiplied(
        (r as u16 * a as u16 / 255) as u8,
        (g as u16 * a as u16 / 255) as u8,
        (b as u16 * a as u16 / 255) as u8,
        a,
    )
}

const fn white(a: u8) -> Color32 {
    rgba(255, 255, 255, a)
}

static AURORA: Palette = Palette {
    dark: true,
    bg_top: Color32::from_rgb(34, 36, 82),
    bg_bottom: Color32::from_rgb(74, 58, 124),
    glows: [
        (0.12, 0.10, 0.55, rgba(64, 214, 200, 120)),
        (0.92, 0.35, 0.50, rgba(90, 140, 255, 110)),
        (0.45, 1.05, 0.60, rgba(236, 110, 190, 90)),
    ],
    panel: white(16),
    bar: white(12),
    card: white(22),
    card_hi: white(38),
    line: white(52),
    text: Color32::from_rgb(242, 244, 255),
    text_dim: Color32::from_rgb(184, 190, 224),
    accent: Color32::from_rgb(110, 238, 214),
    accent_dim: rgba(110, 238, 214, 70),
    accent_hi: rgba(110, 238, 214, 120),
    on_accent: Color32::from_rgb(20, 24, 56),
    select: Color32::from_rgb(255, 206, 128),
    danger: Color32::from_rgb(255, 130, 150),
    danger_dim: rgba(255, 110, 140, 90),
    ok_dim: rgba(110, 238, 214, 46),
    body: white(18),
    inset: rgba(12, 14, 40, 110),
    popup: rgba(44, 44, 96, 248),
    shadow: rgba(8, 6, 30, 90),
};

static FROST: Palette = Palette {
    dark: false,
    bg_top: Color32::from_rgb(238, 242, 250),
    bg_bottom: Color32::from_rgb(214, 224, 241),
    glows: [
        (0.10, 0.12, 0.50, rgba(120, 200, 255, 120)),
        (0.95, 0.30, 0.45, rgba(110, 130, 255, 100)),
        (0.50, 1.05, 0.60, rgba(180, 160, 255, 90)),
    ],
    panel: white(110),
    bar: white(130),
    card: white(165),
    card_hi: white(235),
    line: rgba(255, 255, 255, 235),
    text: Color32::from_rgb(28, 38, 60),
    text_dim: Color32::from_rgb(96, 110, 140),
    accent: Color32::from_rgb(52, 120, 246),
    accent_dim: rgba(52, 120, 246, 60),
    accent_hi: rgba(52, 120, 246, 110),
    on_accent: Color32::WHITE,
    select: Color32::from_rgb(236, 140, 20),
    danger: Color32::from_rgb(220, 38, 80),
    danger_dim: rgba(220, 38, 80, 50),
    ok_dim: rgba(52, 120, 246, 40),
    body: white(150),
    inset: rgba(220, 228, 242, 200),
    popup: rgba(250, 252, 255, 252),
    shadow: rgba(60, 80, 130, 45),
};

static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The active palette.
pub fn p() -> &'static Palette {
    match CURRENT.load(Ordering::Relaxed) {
        1 => &FROST,
        _ => &AURORA,
    }
}

pub fn current() -> Theme {
    if p().dark {
        Theme::Aurora
    } else {
        Theme::Frost
    }
}

/// Switches theme and restyles the widgets.
pub fn set(ctx: &egui::Context, t: Theme) {
    CURRENT.store(
        match t {
            Theme::Aurora => 0,
            Theme::Frost => 1,
        },
        Ordering::Relaxed,
    );
    apply(ctx);
}

pub fn apply(ctx: &egui::Context) {
    let c = p();
    let mut v = if c.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = c.popup;
    v.extreme_bg_color = c.inset;
    v.faint_bg_color = c.card;
    v.code_bg_color = c.inset;
    v.override_text_color = Some(c.text);
    v.hyperlink_color = c.accent;
    v.selection.bg_fill = c.accent_hi;
    v.selection.stroke = Stroke::new(1.0_f32, c.accent);
    v.window_stroke = Stroke::new(1.0_f32, c.line);
    v.window_rounding = egui::Rounding::same(16.0);
    v.menu_rounding = egui::Rounding::same(12.0);
    v.window_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 10.0),
        blur: 30.0,
        spread: 0.0,
        color: c.shadow,
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: egui::vec2(0.0, 6.0),
        blur: 18.0,
        spread: 0.0,
        color: c.shadow,
    };

    let round = egui::Rounding::same(10.0);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = c.card;
    w.noninteractive.weak_bg_fill = c.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.line);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, c.text);
    w.noninteractive.rounding = round;

    w.inactive.bg_fill = c.card_hi;
    w.inactive.weak_bg_fill = c.card_hi;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, c.line);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, c.text);
    w.inactive.rounding = round;

    let hover = if c.dark { white(60) } else { white(250) };
    w.hovered.bg_fill = hover;
    w.hovered.weak_bg_fill = hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, c.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, c.text);
    w.hovered.rounding = round;

    w.active.bg_fill = c.accent_hi;
    w.active.weak_bg_fill = c.accent_hi;
    w.active.bg_stroke = Stroke::new(1.0_f32, c.accent);
    w.active.fg_stroke = Stroke::new(1.5_f32, c.text);
    w.active.rounding = round;

    w.open.bg_fill = c.card_hi;
    w.open.weak_bg_fill = c.card_hi;
    w.open.rounding = round;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 7.0);
    style.spacing.button_padding = egui::vec2(14.0, 6.0);
    style.spacing.slider_width = 180.0;
    style.spacing.interact_size.y = 26.0;
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

/// Paints the gradient and soft glows behind every panel. Call first in each frame.
pub fn paint_backdrop(ctx: &egui::Context) {
    let c = p();
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    let mut m = egui::Mesh::default();
    m.colored_vertex(screen.left_top(), c.bg_top);
    m.colored_vertex(screen.right_top(), c.bg_top);
    m.colored_vertex(screen.right_bottom(), c.bg_bottom);
    m.colored_vertex(screen.left_bottom(), c.bg_bottom);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(m));

    let diag = screen.size().length();
    for (fx, fy, fr, color) in c.glows {
        let center = Pos2::new(
            screen.left() + fx * screen.width(),
            screen.top() + fy * screen.height(),
        );
        glow(&painter, center, fr * diag, color);
    }
}

/// A radial glow: `color` at the centre fading smoothly to transparent at `radius`.
fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    const SEGMENTS: u32 = 64;
    const RINGS: [(f32, f32); 3] = [(0.0, 1.0), (0.45, 0.55), (1.0, 0.0)];
    let mut m = egui::Mesh::default();
    // centre
    m.colored_vertex(center, color);
    // rings of decreasing strength give a soft, blur-like falloff
    for &(r, k) in &RINGS[1..] {
        for i in 0..SEGMENTS {
            let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let pos = center + egui::vec2(a.cos(), a.sin()) * radius * r;
            m.colored_vertex(pos, color.gamma_multiply(k));
        }
    }
    for i in 0..SEGMENTS {
        let j = (i + 1) % SEGMENTS;
        // centre fan to the first ring
        m.add_triangle(0, 1 + i, 1 + j);
        // quads between the first and second ring
        let (a0, a1) = (1 + i, 1 + j);
        let (b0, b1) = (1 + SEGMENTS + i, 1 + SEGMENTS + j);
        m.add_triangle(a0, b0, b1);
        m.add_triangle(a0, b1, a1);
    }
    painter.add(egui::Shape::mesh(m));
}

/// A frosted glass card for grouped content.
pub fn card() -> egui::Frame {
    let c = p();
    egui::Frame::default()
        .fill(c.card)
        .stroke(Stroke::new(1.0_f32, c.line))
        .rounding(16.0)
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 8.0),
            blur: 24.0,
            spread: 0.0,
            color: c.shadow,
        })
        .inner_margin(egui::Margin::symmetric(18.0, 14.0))
}

/// The translucent strip used for the top and bottom bars.
pub fn bar() -> egui::Frame {
    egui::Frame::default()
        .fill(p().bar)
        .inner_margin(egui::Margin::symmetric(18.0, 10.0))
}

/// A clear frame for the main area (the backdrop shows through).
pub fn clear(margin: f32) -> egui::Frame {
    egui::Frame::default()
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin::same(margin))
}

/// A glass side panel.
pub fn side() -> egui::Frame {
    let c = p();
    egui::Frame::default()
        .fill(c.panel)
        .stroke(Stroke::new(1.0_f32, c.line))
        .inner_margin(egui::Margin::same(18.0))
}

/// Section title inside a card.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(15.5)
            .color(p().text),
    );
}

/// Dim help text.
pub fn help(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.5).color(p().text_dim));
}

/// A rectangle painted as glass (used for keys and buttons on the pictures).
pub fn glass_rect(
    painter: &egui::Painter,
    r: Rect,
    rounding: egui::Rounding,
    fill: Color32,
    stroke: Stroke,
) {
    painter.rect(r, rounding, fill, stroke);
    let rounding = rounding.nw.max(rounding.ne);
    // a faint highlight along the top edge sells the glass
    let hl = if p().dark { white(28) } else { white(200) };
    let top = Rect::from_min_max(
        r.left_top() + egui::vec2(rounding * 0.6, 1.5),
        Pos2::new(r.right() - rounding * 0.6, r.top() + 2.5),
    );
    if top.width() > 2.0 {
        painter.rect_filled(top, 1.0, hl);
    }
}
