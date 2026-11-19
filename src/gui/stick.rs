//! Mouse-to-stick page: axis bindings, calibration, response graph and live stick preview.

use crate::theme;
use crate::PadApp;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke};
use padbridge::config::{AxisBinding, MouseAxis};
use padbridge::mouse::StickStage;
use padbridge::pad::{AxisTarget, Report};

const DEFAULT_CPS: f64 = 8000.0;

impl PadApp {
    pub fn stick_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("stick-visuals")
            .exact_width(470.0)
            .resizable(false)
            .frame(theme::side())
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    theme::title(ui, "Response");
                    theme::help(
                        ui,
                        "Coloured line: how far padbridge pushes the stick. Grey line: the camera speed the \
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
            .frame(theme::clear(16.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(2, |cols| {
                        self.axis_card(&mut cols[0], MouseAxis::X);
                        self.axis_card(&mut cols[1], MouseAxis::Y);
                    });
                    ui.add_space(12.0);
                    self.calibrate_card(ui);
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

    fn calibrate_card(&mut self, ui: &mut egui::Ui) {
        use padbridge::app::StickTest;
        use padbridge::mouse::{RAMP_MAX, RAMP_WAIT_S};
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Calibrate in your game");
            theme::help(
                ui,
                "Most games ignore small stick movements (a deadzone). If padbridge doesn't \
                 know yours, small mouse movements do nothing and then jump. This finds it:",
            );
            let hotkey = self.cfg.general.toggle_hotkey.to_string();
            theme::help(
                ui,
                &format!(
                    "1. Start mapping (not Test mode) and click Find deadzone. 2. Switch to the \
                     game within {RAMP_WAIT_S:.0} s and leave the mouse still. 3. padbridge \
                     slowly turns the camera stick; press {hotkey} the moment the camera starts \
                     moving, then {hotkey} again to resume."
                ),
            );
            ui.add_space(6.0);
            let left = crate::stick::uses(self, true) && !crate::stick::uses(self, false);
            let mut use_value: Option<f64> = None;
            match &self.runtime {
                None => {
                    ui.add_enabled(false, egui::Button::new("Find deadzone"));
                    theme::help(ui, "Press Start first.");
                }
                Some(rt) => match rt.stick_test() {
                    StickTest::Idle => {
                        if ui.button("Find deadzone").clicked() {
                            rt.start_stick_test(left);
                        }
                    }
                    StickTest::Running { started, value, .. } => {
                        let waited = started.elapsed().as_secs_f64();
                        ui.horizontal(|ui| {
                            if waited < RAMP_WAIT_S {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Switch to the game now… {:.0}",
                                        (RAMP_WAIT_S - waited).ceil()
                                    ))
                                    .color(theme::p().select)
                                    .strong(),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Pushing the stick: {:.0}% of {:.0}%. Press {hotkey} \
                                         when the camera starts moving.",
                                        value * 100.0,
                                        RAMP_MAX * 100.0
                                    ))
                                    .color(theme::p().accent)
                                    .strong(),
                                );
                            }
                            if ui.button("Cancel").clicked() {
                                rt.clear_stick_test();
                            }
                        });
                        ui.ctx()
                            .request_repaint_after(std::time::Duration::from_millis(100));
                    }
                    StickTest::Found(v) => {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Found: the camera starts moving at {:.0}% stick.",
                                    v * 100.0
                                ))
                                .color(theme::p().accent)
                                .strong(),
                            );
                            if ui.button("Use it").clicked() {
                                use_value = Some(v);
                                rt.clear_stick_test();
                            }
                            if ui.button("Try again").clicked() {
                                rt.start_stick_test(left);
                            }
                        });
                    }
                    StickTest::NotFound => {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    "No press seen. Was the game in front and mapping running \
                                     (not Test mode)?",
                                )
                                .color(theme::p().select),
                            );
                            if ui.button("Try again").clicked() {
                                rt.start_stick_test(left);
                            }
                        });
                    }
                },
            }
            if let Some(v) = use_value {
                self.cfg.mouse.deadzone = (v * 1000.0).round() / 1000.0;
                self.info(format!(
                    "Game deadzone set to {:.3}. Click 'Apply changes' at the top to use it, and \
                     Save to keep it.",
                    self.cfg.mouse.deadzone
                ));
            }
        });
    }

    fn motion_card(&mut self, ui: &mut egui::Ui) {
        // (name, catch-up ms, description). Smoothing follows the game frame rate instead.
        const PRESETS: [(&str, f64, &str); 3] = [
            (
                "Precise",
                0.0,
                "Default. The camera stops the instant your hand stops; very fast flicks are \
                 capped at the stick's top speed.",
            ),
            (
                "Balanced",
                50.0,
                "Short catch-up after very fast flicks (the camera may move on briefly).",
            ),
            (
                "Exact distance",
                300.0,
                "Every count is played out, so fast flicks land exactly but keep turning briefly.",
            ),
        ];
        const FRAME_RATES: [u32; 7] = [30, 60, 120, 144, 165, 240, 360];
        let live_fps = self.runtime.as_ref().and_then(|r| r.frame_rate());
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Motion feel (both axes)");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Preset");
                for (name, catch, desc) in PRESETS {
                    let m = &self.cfg.mouse;
                    let active = m.max_backlog_ms == catch;
                    let text = egui::RichText::new(name).color(if active {
                        theme::p().on_accent
                    } else {
                        theme::p().text
                    });
                    let b = egui::Button::new(text).fill(if active {
                        theme::p().accent
                    } else {
                        theme::p().card_hi
                    });
                    if ui.add(b).on_hover_text(desc).clicked() {
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
                    ui.label("Game frame rate");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut m.adaptive_frame_rate, "Detect automatically")
                            .on_hover_text(
                                "Measures the game's frame rate while mapping runs and spreads \
                                 each mouse movement over exactly one frame. Exact when \
                                 padbridge runs as administrator; otherwise uses the monitor \
                                 refresh rate.",
                            );
                        if !m.adaptive_frame_rate {
                            let current = FRAME_RATES
                                .iter()
                                .copied()
                                .find(|f| (1000.0 / f64::from(*f)).round() == m.smoothing_ms);
                            egui::ComboBox::from_id_salt("frame-rate")
                                .width(110.0)
                                .selected_text(match current {
                                    Some(f) => format!("{f} fps"),
                                    None => "Custom".to_string(),
                                })
                                .show_ui(ui, |ui| {
                                    for f in FRAME_RATES {
                                        if ui
                                            .selectable_label(
                                                current == Some(f),
                                                format!("{f} fps"),
                                            )
                                            .clicked()
                                        {
                                            m.smoothing_ms = (1000.0 / f64::from(f)).round();
                                        }
                                    }
                                });
                            ui.add(
                                egui::Slider::new(&mut m.smoothing_ms, 0.0..=50.0)
                                    .suffix(" ms")
                                    .max_decimals(0),
                            );
                        }
                    });
                    if m.adaptive_frame_rate {
                        match live_fps {
                            Some((f, src)) => {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Following {f:.0} fps ({}): {:.1} ms",
                                        src.describe(),
                                        padbridge::fps::smoothing_for_fps(f)
                                    ))
                                    .size(12.5)
                                    .color(theme::p().accent),
                                );
                            }
                            None => theme::help(
                                ui,
                                "Detected while mapping runs. Run padbridge as administrator for \
                                 an exact reading.",
                            ),
                        }
                    } else {
                        theme::help(ui, "Match your game's fps: nothing missed, no added lag.");
                    }
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
                "1. Small movements do nothing, then jump (no precision): use 'Find deadzone' \
                 above. This matters more than anything else.",
                "2. Camera keeps moving after your hand stops: pick Precise, and turn off look \
                 acceleration / aim smoothing in the game's controller settings.",
                "3. Everything is too slow: raise 'Overall speed', or calibrate each axis from a \
                 360° turn.",
                "4. Fast turns hit a ceiling: that is the game's top stick speed. Set the game's \
                 controller look sensitivity to maximum; padbridge cannot turn faster than the \
                 stick allows.",
                "5. Small movements crawl compared with big ones: raise 'Game response curve' \
                 (many games use about 2).",
                "6. Apply changes while running with 'Apply changes' at the top.",
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
        p.rect_filled(rect, 12.0, theme::p().inset);
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
                theme::p().danger_dim.gamma_multiply(0.45),
            );
            p.text(
                to_screen(x_max, m.deadzone) + egui::vec2(-4.0, 3.0),
                Align2::RIGHT_TOP,
                "game deadzone",
                FontId::proportional(11.0),
                theme::p().danger,
            );
        }
        // grid
        let grid = Stroke::new(1.0_f32, theme::p().line.gamma_multiply(0.45));
        for i in 0..=4 {
            let y = i as f64 / 4.0;
            p.line_segment([to_screen(0.0, y), to_screen(x_max, y)], grid);
            p.text(
                to_screen(0.0, y) - egui::vec2(6.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{}%", i * 25),
                FontId::proportional(11.0),
                theme::p().text_dim,
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
                theme::p().text_dim,
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
            theme::p().text_dim,
        );
        // camera speed the game produces: 1:1 up to full, then capped (excess is queued)
        let game: Vec<Pos2> = [(0.0, 0.0), (1.0, 1.0), (x_max, 1.0)]
            .iter()
            .map(|&(x, y)| to_screen(x, y))
            .collect();
        p.add(egui::Shape::line(
            game,
            Stroke::new(2.0_f32, theme::p().text_dim),
        ));
        // stick value padbridge sends
        let mut sent = vec![to_screen(0.0, 0.0)];
        for i in 1..=250 {
            let x = i as f64 / 200.0;
            let raw = stage.respond((x.min(1.0), 0.0)).0;
            sent.push(to_screen(x, raw));
        }
        p.add(egui::Shape::line(
            sent,
            Stroke::new(2.5_f32, theme::p().accent),
        ));
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
        if let Some(rt) = &self.runtime {
            let n = rt.mouse_events();
            let capturing = rt.enabled();
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "Input: {}   Mouse movement received: {n}",
                        rt.backend().describe()
                    ))
                    .size(12.5)
                    .color(theme::p().text_dim),
                );
            });
            if capturing && n == 0 {
                theme::help(
                    ui,
                    "Move the mouse now. If this number stays at 0, mouse movement is not reaching \
                     padbridge: tell Claude which input method is shown above.",
                );
            } else if !capturing {
                theme::help(ui, "Capture is paused, so the sticks stay centred.");
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
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
            p.circle_filled(c, radius, theme::p().card);
            p.circle_stroke(c, radius, Stroke::new(1.5_f32, theme::p().line));
            if mouse_driven && self.cfg.mouse.deadzone > 0.0 {
                p.circle_stroke(
                    c,
                    radius * self.cfg.mouse.deadzone as f32,
                    Stroke::new(1.0_f32, theme::p().danger),
                );
            }
            p.line_segment(
                [c - egui::vec2(radius, 0.0), c + egui::vec2(radius, 0.0)],
                Stroke::new(1.0_f32, theme::p().line),
            );
            p.line_segment(
                [c - egui::vec2(0.0, radius), c + egui::vec2(0.0, radius)],
                Stroke::new(1.0_f32, theme::p().line),
            );
            let fx = f32::from(x) / 32767.0;
            let fy = f32::from(y) / 32767.0;
            let dot = c + egui::vec2(fx * radius, -fy * radius);
            let color = if report.is_some() {
                theme::p().accent
            } else {
                theme::p().text_dim
            };
            p.line_segment([c, dot], Stroke::new(2.0_f32, color.gamma_multiply(0.5)));
            p.circle_filled(dot, 7.0, color);
            p.text(
                Pos2::new(c.x, c.y + radius + 8.0),
                Align2::CENTER_TOP,
                format!("{name}   {x:>6}, {y:>6}"),
                FontId::monospace(12.0),
                theme::p().text_dim,
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
        ui.label(egui::RichText::new("Buttons:").color(theme::p().text_dim));
        let mut any = false;
        for (bit, n) in names {
            if r.buttons & bit != 0 {
                any = true;
                ui.label(egui::RichText::new(n).color(theme::p().accent).strong());
            }
        }
        if r.left_trigger > 0 {
            any = true;
            ui.label(
                egui::RichText::new(format!("LT {}", r.left_trigger))
                    .color(theme::p().accent)
                    .strong(),
            );
        }
        if r.right_trigger > 0 {
            any = true;
            ui.label(
                egui::RichText::new(format!("RT {}", r.right_trigger))
                    .color(theme::p().accent)
                    .strong(),
            );
        }
        if !any {
            ui.label(egui::RichText::new("none").color(theme::p().text_dim));
        }
    });
}
