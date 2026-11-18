//! Mouse-to-stick page: axis bindings, calibration, response graph and live stick preview.

use crate::theme;
use crate::PadApp;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use padbridge::config::{AxisBinding, MouseAxis};
use padbridge::mouse::StickStage;
use padbridge::pad::{AxisTarget, Report};

const DEFAULT_CPS: f64 = 8000.0;

impl PadApp {
    pub fn stick_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("stick-visuals")
            .exact_width(470.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    theme::title(ui, "Response");
                    theme::help(
                        ui,
                        "Green: how far padbridge pushes the stick. Grey: the camera speed the \
                         game ends up with, which matches the mouse 1:1 up to full speed.",
                    );
                    ui.add_space(6.0);
                    self.response_graph(ui);
                    ui.add_space(16.0);
                    theme::title(ui, "Live stick output");
                    self.live_preview(ui);
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(2, |cols| {
                        self.axis_card(&mut cols[0], MouseAxis::X);
                        self.axis_card(&mut cols[1], MouseAxis::Y);
                    });
                    ui.add_space(12.0);
                    self.motion_card(ui);
                });
            });
    }

    fn axis_card(&mut self, ui: &mut egui::Ui, axis: MouseAxis) {
        let idx = usize::from(axis == MouseAxis::Y);
        let (title, default_target) = match axis {
            MouseAxis::X => ("Mouse left / right", AxisTarget::RightX),
            MouseAxis::Y => ("Mouse up / down", AxisTarget::RightY),
        };
        let other_target = self
            .cfg
            .axes
            .iter()
            .find(|a| a.source != axis)
            .map(|a| a.target);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let pos = self.cfg.axes.iter().position(|a| a.source == axis);
            ui.horizontal(|ui| {
                theme::title(ui, title);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = pos.is_some();
                    if ui.checkbox(&mut on, "Enabled").changed() {
                        if on {
                            let target = if other_target == Some(default_target) {
                                AxisTarget::ALL
                                    .into_iter()
                                    .find(|t| Some(*t) != other_target)
                                    .unwrap_or(default_target)
                            } else {
                                default_target
                            };
                            self.cfg.axes.push(AxisBinding {
                                source: axis,
                                target,
                                sensitivity: 1.0,
                                full_deflection_cps: DEFAULT_CPS,
                                invert: false,
                            });
                        } else if let Some(p) = pos {
                            self.cfg.axes.remove(p);
                        }
                    }
                });
            });
            let Some(pos) = self.cfg.axes.iter().position(|a| a.source == axis) else {
                theme::help(
                    ui,
                    "Not mapped: this mouse movement does nothing on the controller.",
                );
                return;
            };
            let calib = &mut self.calib[idx];
            let a = &mut self.cfg.axes[pos];
            egui::Grid::new(("axis-grid", idx))
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Drives");
                    egui::ComboBox::from_id_salt(("axis-target", idx))
                        .selected_text(a.target.name())
                        .show_ui(ui, |ui| {
                            for t in AxisTarget::ALL {
                                let taken = other_target == Some(t);
                                let label = if taken {
                                    format!("{} (used by the other axis)", t.name())
                                } else {
                                    t.name().to_string()
                                };
                                if ui
                                    .add_enabled(
                                        !taken,
                                        egui::SelectableLabel::new(a.target == t, label),
                                    )
                                    .clicked()
                                {
                                    a.target = t;
                                }
                            }
                        });
                    ui.end_row();

                    ui.label("Sensitivity");
                    ui.add(
                        egui::Slider::new(&mut a.sensitivity, 0.05..=10.0)
                            .logarithmic(true)
                            .max_decimals(2),
                    );
                    ui.end_row();

                    ui.label("Invert");
                    ui.checkbox(&mut a.invert, "");
                    ui.end_row();

                    ui.label("Full stick at").on_hover_text(
                        "The mouse speed that equals holding the stick all the way over. \
                             Use the calibration below to get it right for your game.",
                    );
                    ui.add(
                        egui::DragValue::new(&mut a.full_deflection_cps)
                            .range(50.0..=1_000_000.0)
                            .speed(25.0)
                            .max_decimals(0)
                            .suffix(" counts/s"),
                    );
                    ui.end_row();
                });
            ui.add_space(6.0);
            egui::CollapsingHeader::new("Calibrate from a 360° turn")
                .id_salt(("calib", idx))
                .show(ui, |ui| {
                    theme::help(
                        ui,
                        "1. In the game, hold the stick fully over and time a full turn. \
                         2. Enter how many mouse counts a full turn takes at your normal mouse \
                         sensitivity (DPI \u{00D7} cm per 360 \u{00F7} 2.54).",
                    );
                    egui::Grid::new(("calib-grid", idx))
                        .num_columns(2)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Mouse counts per 360°");
                            ui.add(
                                egui::DragValue::new(&mut calib[0])
                                    .range(1.0..=10_000_000.0)
                                    .speed(50.0)
                                    .max_decimals(0),
                            );
                            ui.end_row();
                            ui.label("Seconds per 360° (stick)");
                            ui.add(
                                egui::DragValue::new(&mut calib[1])
                                    .range(0.05..=600.0)
                                    .speed(0.01)
                                    .max_decimals(2)
                                    .suffix(" s"),
                            );
                            ui.end_row();
                        });
                    let cps = calib[0] / calib[1];
                    let valid = (50.0..=1_000_000.0).contains(&cps);
                    ui.horizontal(|ui| {
                        ui.label(format!("= {cps:.0} counts/s"));
                        if ui
                            .add_enabled(valid, egui::Button::new("Use this"))
                            .clicked()
                        {
                            a.full_deflection_cps = cps.round();
                        }
                    });
                });
        });
    }

    fn motion_card(&mut self, ui: &mut egui::Ui) {
        // (name, smoothing ms, catch-up ms, description)
        const PRESETS: [(&str, f64, f64, &str); 3] = [
            (
                "Snappy",
                6.0,
                0.0,
                "Lowest lag. Very fast flicks are cut at the stick's top speed.",
            ),
            (
                "Balanced",
                10.0,
                50.0,
                "Default. Short catch-up after fast flicks.",
            ),
            (
                "Exact distance",
                16.0,
                300.0,
                "Every count is played out, so fast flicks land exactly but keep turning briefly.",
            ),
        ];
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Motion feel (both axes)");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Preset");
                for (name, smooth, catch, desc) in PRESETS {
                    let m = &self.cfg.mouse;
                    let active = m.smoothing_ms == smooth && m.max_backlog_ms == catch;
                    let text = egui::RichText::new(name).color(if active {
                        theme::BG
                    } else {
                        theme::TEXT
                    });
                    let b = egui::Button::new(text).fill(if active {
                        theme::ACCENT
                    } else {
                        theme::CARD_HI
                    });
                    if ui.add(b).on_hover_text(desc).clicked() {
                        self.cfg.mouse.smoothing_ms = smooth;
                        self.cfg.mouse.max_backlog_ms = catch;
                    }
                }
            });
            ui.add_space(6.0);

            // One speed control for both axes, keeping their ratio.
            let current = self.cfg.axes.first().map(|a| a.sensitivity);
            egui::Grid::new("motion-grid")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Overall speed");
                    match current {
                        Some(old) => {
                            let mut v = old;
                            let r = ui.add(
                                egui::Slider::new(&mut v, 0.05..=10.0)
                                    .logarithmic(true)
                                    .max_decimals(2),
                            );
                            if r.changed() && old > 0.0 {
                                let k = v / old;
                                for a in &mut self.cfg.axes {
                                    a.sensitivity = (a.sensitivity * k).clamp(0.01, 100.0);
                                }
                            }
                            theme::help(ui, "Scales both mouse axes together.");
                        }
                        None => {
                            ui.label("-");
                            theme::help(ui, "No mouse axis is enabled above.");
                        }
                    }
                    ui.end_row();

                    let m = &mut self.cfg.mouse;
                    ui.label("Smoothing");
                    ui.add(
                        egui::Slider::new(&mut m.smoothing_ms, 0.0..=50.0)
                            .suffix(" ms")
                            .max_decimals(0),
                    );
                    theme::help(ui, "About one game frame: 16 at 60 fps, 7 at 144 fps.");
                    ui.end_row();

                    ui.label("Catch-up after flicks");
                    ui.add(
                        egui::Slider::new(&mut m.max_backlog_ms, 0.0..=1000.0)
                            .suffix(" ms")
                            .max_decimals(0),
                    );
                    theme::help(
                        ui,
                        "If the camera keeps turning after you stop, lower this.",
                    );
                    ui.end_row();

                    ui.label("Game deadzone");
                    ui.add(egui::Slider::new(&mut m.deadzone, 0.0..=0.9).max_decimals(3));
                    theme::help(
                        ui,
                        "Raise until slow movement just starts to move the camera.",
                    );
                    ui.end_row();

                    ui.label("Game response curve");
                    ui.add(
                        egui::Slider::new(&mut m.curve_exponent, 0.2..=4.0)
                            .logarithmic(true)
                            .max_decimals(2),
                    );
                    theme::help(ui, "1 = linear. Raise if small movements feel too slow.");
                    ui.end_row();

                    ui.label("Freeze the cursor");
                    ui.checkbox(&mut m.swallow_motion, "");
                    theme::help(
                        ui,
                        "While capturing, the mouse moves the stick, not the cursor.",
                    );
                    ui.end_row();
                });
        });
        ui.add_space(12.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Feels slow or floaty?");
            for line in [
                "1. Camera keeps moving after your hand stops: lower 'Catch-up after flicks', \
                 or pick Snappy.",
                "2. Everything is too slow: raise 'Overall speed'. Watch the live preview: normal \
                 movements should push the green dot well out from the centre.",
                "3. Fast turns hit a ceiling: that is the game's top stick speed. In the game, set \
                 controller look sensitivity to maximum and turn off look acceleration / aim \
                 slowdown; padbridge cannot turn faster than the stick allows.",
                "4. Small movements do nothing, then jump: raise 'Game deadzone'. Small movements \
                 crawl: raise 'Game response curve'.",
                "5. Apply changes while running with 'Apply changes' at the top.",
            ] {
                theme::help(ui, line);
            }
        });
    }

    fn response_graph(&self, ui: &mut egui::Ui) {
        let m = &self.cfg.mouse;
        let stage = StickStage::new(1, 0.001, 0.0, m.deadzone, m.curve_exponent);
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 230.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 8.0, theme::BG);
        let plot = Rect::from_min_max(
            Pos2::new(rect.left() + 40.0, rect.top() + 14.0),
            Pos2::new(rect.right() - 14.0, rect.bottom() - 34.0),
        );
        let x_max = 1.25;
        let to_screen = |x: f64, y: f64| {
            Pos2::new(
                plot.left() + (x / x_max) as f32 * plot.width(),
                plot.bottom() - y as f32 * plot.height(),
            )
        };
        // deadzone band
        if m.deadzone > 0.0 {
            p.rect_filled(
                Rect::from_min_max(to_screen(0.0, m.deadzone), to_screen(x_max, 0.0)),
                0.0,
                Color32::from_rgba_unmultiplied(236, 96, 96, 28),
            );
            p.text(
                to_screen(x_max, m.deadzone) + egui::vec2(-4.0, 3.0),
                Align2::RIGHT_TOP,
                "game deadzone",
                FontId::proportional(11.0),
                theme::DANGER,
            );
        }
        // grid
        let grid = Stroke::new(1.0_f32, Color32::from_rgb(40, 45, 56));
        for i in 0..=4 {
            let y = i as f64 / 4.0;
            p.line_segment([to_screen(0.0, y), to_screen(x_max, y)], grid);
            p.text(
                to_screen(0.0, y) - egui::vec2(6.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{}%", i * 25),
                FontId::proportional(11.0),
                theme::TEXT_DIM,
            );
        }
        let cps = self
            .cfg
            .axes
            .iter()
            .find(|a| a.source == MouseAxis::X)
            .or_else(|| self.cfg.axes.first())
            .map(|a| a.full_deflection_cps / a.sensitivity);
        for i in 0..=5 {
            let x = i as f64 * 0.25;
            p.line_segment([to_screen(x, 0.0), to_screen(x, 1.0)], grid);
            let label = match cps {
                Some(c) => format!("{:.0}", c * x),
                None => format!("{}%", i * 25),
            };
            p.text(
                to_screen(x, 0.0) + egui::vec2(0.0, 4.0),
                Align2::CENTER_TOP,
                label,
                FontId::proportional(11.0),
                theme::TEXT_DIM,
            );
        }
        p.text(
            Pos2::new(plot.center().x, rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            if cps.is_some() {
                "mouse speed (counts/s)"
            } else {
                "mouse speed (% of full stick)"
            },
            FontId::proportional(11.5),
            theme::TEXT_DIM,
        );
        // camera speed the game produces: 1:1 up to full, then capped (excess is queued)
        let game: Vec<Pos2> = [(0.0, 0.0), (1.0, 1.0), (x_max, 1.0)]
            .iter()
            .map(|&(x, y)| to_screen(x, y))
            .collect();
        p.add(egui::Shape::line(
            game,
            Stroke::new(2.0_f32, theme::TEXT_DIM),
        ));
        // stick value padbridge sends
        let mut sent = vec![to_screen(0.0, 0.0)];
        for i in 1..=250 {
            let x = i as f64 / 200.0;
            let raw = stage.respond((x.min(1.0), 0.0)).0;
            sent.push(to_screen(x, raw));
        }
        p.add(egui::Shape::line(sent, Stroke::new(2.5_f32, theme::ACCENT)));
    }

    fn live_preview(&self, ui: &mut egui::Ui) {
        let report = self.live_report();
        if report.is_none() {
            theme::help(
                ui,
                "Press Start (tick Test mode if the driver isn't installed) and move the mouse to \
                 see the sticks here.",
            );
        }
        let r = report.unwrap_or_default();
        let width = ui.available_width();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 200.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let half = rect.width() / 2.0;
        let radius = (half - 30.0).min(78.0);
        let sticks = [
            (
                "Left stick",
                r.thumb_lx,
                r.thumb_ly,
                crate::stick::uses(self, true),
            ),
            (
                "Right stick",
                r.thumb_rx,
                r.thumb_ry,
                crate::stick::uses(self, false),
            ),
        ];
        for (i, (name, x, y, mouse_driven)) in sticks.into_iter().enumerate() {
            let c = Pos2::new(
                rect.left() + half * (i as f32 + 0.5),
                rect.top() + radius + 8.0,
            );
            p.circle_filled(c, radius, theme::CARD);
            p.circle_stroke(c, radius, Stroke::new(1.5_f32, theme::LINE));
            if mouse_driven && self.cfg.mouse.deadzone > 0.0 {
                p.circle_stroke(
                    c,
                    radius * self.cfg.mouse.deadzone as f32,
                    Stroke::new(1.0_f32, theme::DANGER),
                );
            }
            p.line_segment(
                [c - egui::vec2(radius, 0.0), c + egui::vec2(radius, 0.0)],
                Stroke::new(1.0_f32, theme::LINE),
            );
            p.line_segment(
                [c - egui::vec2(0.0, radius), c + egui::vec2(0.0, radius)],
                Stroke::new(1.0_f32, theme::LINE),
            );
            let fx = f32::from(x) / 32767.0;
            let fy = f32::from(y) / 32767.0;
            let dot = c + egui::vec2(fx * radius, -fy * radius);
            let color = if report.is_some() {
                theme::ACCENT
            } else {
                theme::TEXT_DIM
            };
            p.line_segment([c, dot], Stroke::new(2.0_f32, color.gamma_multiply(0.5)));
            p.circle_filled(dot, 7.0, color);
            p.text(
                Pos2::new(c.x, c.y + radius + 8.0),
                Align2::CENTER_TOP,
                format!("{name}   {x:>6}, {y:>6}"),
                FontId::monospace(12.0),
                theme::TEXT_DIM,
            );
        }
        if let Some(rep) = report {
            buttons_line(ui, &rep);
        }
    }
}

/// Whether a mouse axis drives the left (`true`) or right stick.
pub fn uses(app: &PadApp, left: bool) -> bool {
    app.cfg
        .axes
        .iter()
        .any(|a| matches!(a.target, AxisTarget::LeftX | AxisTarget::LeftY) == left)
}

fn buttons_line(ui: &mut egui::Ui, r: &Report) {
    use padbridge::pad::*;
    let names = [
        (BTN_A, "A"),
        (BTN_B, "B"),
        (BTN_X, "X"),
        (BTN_Y, "Y"),
        (BTN_LB, "LB"),
        (BTN_RB, "RB"),
        (BTN_BACK, "Back"),
        (BTN_START, "Start"),
        (BTN_GUIDE, "Guide"),
        (BTN_LEFT_THUMB, "L3"),
        (BTN_RIGHT_THUMB, "R3"),
        (BTN_DPAD_UP, "D Up"),
        (BTN_DPAD_DOWN, "D Dn"),
        (BTN_DPAD_LEFT, "D Lt"),
        (BTN_DPAD_RIGHT, "D Rt"),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Buttons:").color(theme::TEXT_DIM));
        let mut any = false;
        for (bit, n) in names {
            if r.buttons & bit != 0 {
                any = true;
                ui.label(egui::RichText::new(n).color(theme::ACCENT).strong());
            }
        }
        if r.left_trigger > 0 {
            any = true;
            ui.label(
                egui::RichText::new(format!("LT {}", r.left_trigger))
                    .color(theme::ACCENT)
                    .strong(),
            );
        }
        if r.right_trigger > 0 {
            any = true;
            ui.label(
                egui::RichText::new(format!("RT {}", r.right_trigger))
                    .color(theme::ACCENT)
                    .strong(),
            );
        }
        if !any {
            ui.label(egui::RichText::new("none").color(theme::TEXT_DIM));
        }
    });
}
