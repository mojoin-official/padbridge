//! Binding editor: clickable keyboard, mouse and controller pictures plus the side panel.

use crate::theme;
use crate::{PadApp, Tab};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke};
use padbridge::bindings::UnboundPolicy;
use padbridge::keys::{self, Source};
use padbridge::layout::{self, Slot};
use padbridge::pad::Output;

struct SlotLook {
    fill: Color32,
    stroke: Stroke,
    text: Color32,
    sub: Option<String>,
}

/// Draws `slots` scaled to fit the available space. Returns (clicked, hovered) slot indices.
fn slot_canvas(
    ui: &mut egui::Ui,
    slots: &[Slot],
    size: (f32, f32),
    max_h: f32,
    background: impl FnOnce(&egui::Painter, Rect, f32),
    look: impl Fn(&Slot, bool) -> SlotLook,
) -> (Option<usize>, Option<usize>, egui::Response) {
    let avail = ui.available_width();
    let unit = (avail / size.0).min(max_h / size.1).max(8.0);
    let canvas = egui::vec2(size.0 * unit, size.1 * unit);
    // centre horizontally
    let pad = ((avail - canvas.x) / 2.0).max(0.0);
    ui.add_space(4.0);
    let (outer, resp) = ui.allocate_exact_size(egui::vec2(avail, canvas.y), egui::Sense::click());
    let origin = Pos2::new(outer.left() + pad, outer.top());
    let painter = ui.painter_at(outer);
    background(&painter, Rect::from_min_size(origin, canvas), unit);

    let gap = (unit * 0.07).max(2.0);
    let rects: Vec<Rect> = slots
        .iter()
        .map(|s| {
            Rect::from_min_size(
                Pos2::new(origin.x + s.x * unit, origin.y + s.y * unit),
                egui::vec2(s.w * unit, s.h * unit),
            )
            .shrink(gap / 2.0)
        })
        .collect();
    let hovered = resp
        .hover_pos()
        .and_then(|p| rects.iter().position(|r| r.contains(p)));
    let clicked = if resp.clicked() {
        resp.interact_pointer_pos()
            .and_then(|p| rects.iter().position(|r| r.contains(p)))
    } else {
        None
    };

    let label_size = (unit * 0.30).clamp(9.0, 17.0);
    let sub_size = (unit * 0.22).clamp(8.0, 13.0);
    for (i, (s, r)) in slots.iter().zip(&rects).enumerate() {
        let lk = look(s, hovered == Some(i));
        let base = (unit * 0.18).clamp(4.0, 12.0);
        // The two main mouse buttons follow the curve of the mouse body.
        let rounding = match s.name {
            "MouseLeft" => egui::Rounding {
                nw: 3.8 * unit,
                ne: base,
                sw: base,
                se: base,
            },
            "MouseRight" => egui::Rounding {
                nw: base,
                ne: 3.8 * unit,
                sw: base,
                se: base,
            },
            _ => egui::Rounding::same(base),
        };
        theme::glass_rect(&painter, *r, rounding, lk.fill, lk.stroke);
        let max_w = r.width() - 6.0;
        match &lk.sub {
            Some(sub) => {
                fit_text(
                    &painter,
                    Pos2::new(r.center().x, r.top() + r.height() * 0.34),
                    s.label,
                    label_size,
                    max_w,
                    lk.text,
                );
                fit_text(
                    &painter,
                    Pos2::new(r.center().x, r.bottom() - r.height() * 0.24),
                    sub,
                    sub_size,
                    max_w,
                    theme::p().accent,
                );
            }
            None => {
                fit_text(&painter, r.center(), s.label, label_size, max_w, lk.text);
            }
        }
    }
    if hovered.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (clicked, hovered, resp)
}

/// Draws `text` centred on `center`, shrinking the font so it fits within `max_w`.
fn fit_text(
    painter: &egui::Painter,
    center: Pos2,
    text: &str,
    size: f32,
    max_w: f32,
    color: Color32,
) {
    let mut galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(size), color);
    let w = galley.size().x;
    if w > max_w && max_w > 0.0 {
        let smaller = (size * max_w / w).max(7.0);
        galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(smaller), color);
    }
    let pos = center - galley.size() / 2.0;
    painter.galley(pos, galley, color);
}

/// Short summary of a source's outputs that fits under a key.
fn outputs_summary(outs: &[Output]) -> Option<String> {
    match outs {
        [] => None,
        [a] => Some(layout::output_short(*a).to_string()),
        [a, b] => Some(format!(
            "{} {}",
            layout::output_short(*a),
            layout::output_short(*b)
        )),
        [a, rest @ ..] => Some(format!("{} +{}", layout::output_short(*a), rest.len())),
    }
}

fn input_kind(src: Source) -> &'static str {
    match src {
        Source::Key(_) => "Keyboard key",
        Source::Mouse(_) => "Mouse button",
        Source::Wheel(_) => "Mouse wheel (each notch is a short press)",
    }
}

fn draw_mouse_body(p: &egui::Painter, r: Rect, unit: f32) {
    let body = Rect::from_min_max(
        Pos2::new(r.left() + 0.6 * unit, r.top()),
        Pos2::new(r.right() - 0.6 * unit, r.bottom() - 0.2 * unit),
    );
    p.rect(
        body,
        4.2 * unit,
        theme::p().body,
        Stroke::new(2.0_f32, theme::p().line),
    );
    // palm shading
    let palm = Rect::from_min_max(
        Pos2::new(body.left() + 1.4 * unit, r.top() + 7.6 * unit),
        Pos2::new(body.right() - 1.4 * unit, body.bottom() - 1.2 * unit),
    );
    p.rect_filled(palm, 3.0 * unit, theme::p().inset);
    p.text(
        Pos2::new(r.center().x, r.top() + 11.6 * unit),
        Align2::CENTER_CENTER,
        "Movement: see 'Mouse to stick'",
        FontId::proportional((unit * 0.32).clamp(10.0, 15.0)),
        theme::p().text_dim,
    );
}

fn draw_pad_body(p: &egui::Painter, r: Rect, unit: f32) {
    let u = |x: f32, y: f32| Pos2::new(r.left() + x * unit, r.top() + y * unit);
    // The body is the union of a rounded rectangle and two grip circles. Drawn as one shape
    // (not three overlapping ones) so translucent glass shows no darker seams.
    let inside = |x: f32, y: f32| {
        let in_circle = |cx: f32, cy: f32, r: f32| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
        let in_rrect = {
            let (x0, y0, x1, y1, rr) = (2.0, 2.2, 18.0, 9.6, 3.0);
            let qx = (x0 + rr - x).max(x - (x1 - rr)).max(0.0);
            let qy = (y0 + rr - y).max(y - (y1 - rr)).max(0.0);
            x >= x0 && x <= x1 && y >= y0 && y <= y1 && qx * qx + qy * qy <= rr * rr
        };
        in_rrect || in_circle(4.4, 7.6, 4.1) || in_circle(15.6, 7.6, 4.1)
    };
    let (cx, cy) = (10.0, 6.0);
    const STEPS: usize = 360;
    let mut outline = Vec::with_capacity(STEPS);
    for i in 0..STEPS {
        let a = i as f32 / STEPS as f32 * std::f32::consts::TAU;
        let (dx, dy) = (a.cos(), a.sin());
        // coarse scan for the outermost inside point, then refine the edge by bisection
        let mut d = 0.0;
        let mut last = 0.0;
        while d < 12.0 {
            if inside(cx + dx * d, cy + dy * d) {
                last = d;
            }
            d += 0.05;
        }
        let (mut lo, mut hi) = (last, last + 0.05);
        for _ in 0..12 {
            let mid = (lo + hi) / 2.0;
            if inside(cx + dx * mid, cy + dy * mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        outline.push(u(cx + dx * lo, cy + dy * lo));
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(u(cx, cy), theme::p().body);
    for pt in &outline {
        mesh.colored_vertex(*pt, theme::p().body);
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(egui::Shape::mesh(mesh));
    p.add(egui::Shape::closed_line(
        outline,
        Stroke::new(1.5_f32, theme::p().line),
    ));
    // stick wells
    p.circle_filled(u(4.0, 4.5), 2.0 * unit, theme::p().inset);
    p.circle_filled(u(12.55, 7.62), 1.75 * unit, theme::p().inset);
    p.circle_filled(u(16.0, 4.55), 2.0 * unit, theme::p().inset);
    p.circle_filled(u(7.45, 7.62), 1.75 * unit, theme::p().inset);
    let small = FontId::proportional((unit * 0.32).clamp(9.0, 13.0));
    p.text(
        u(4.0, 6.85),
        Align2::CENTER_TOP,
        "Left stick",
        small.clone(),
        theme::p().text_dim,
    );
    p.text(
        u(16.0, 6.85),
        Align2::CENTER_TOP,
        "Face",
        small.clone(),
        theme::p().text_dim,
    );
    p.text(
        u(7.45, 9.5),
        Align2::CENTER_TOP,
        "D-pad",
        small.clone(),
        theme::p().text_dim,
    );
    p.text(
        u(12.55, 9.5),
        Align2::CENTER_TOP,
        "Right stick",
        small,
        theme::p().text_dim,
    );
}

impl PadApp {
    pub fn input_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .exact_width(360.0)
            .resizable(false)
            .frame(theme::side())
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.input_inspector(ui));
            });

        egui::CentralPanel::default()
            .frame(theme::clear(16.0))
            .show(ctx, |ui| {
                let keyboard = self.tab == Tab::Keyboard;
                ui.label(
                    egui::RichText::new(if keyboard {
                        "Click a key to choose what it does on the controller."
                    } else {
                        "Click a mouse button or wheel direction to choose what it does."
                    })
                    .color(theme::p().text_dim),
                );
                let (slots, size, max_h): (&[Slot], (f32, f32), f32) = if keyboard {
                    (
                        layout::KEYBOARD,
                        layout::KEYBOARD_SIZE,
                        ui.available_height() - 40.0,
                    )
                } else {
                    (
                        layout::MOUSE,
                        layout::MOUSE_SIZE,
                        (ui.available_height() - 40.0).min(560.0),
                    )
                };
                let selected = self.selected_input;
                let bindings = &self.cfg.bindings;
                let (clicked, hovered, resp) = slot_canvas(
                    ui,
                    slots,
                    size,
                    max_h,
                    |p, r, unit| {
                        if !keyboard {
                            draw_mouse_body(p, r, unit);
                        }
                    },
                    |s, hover| {
                        let src = layout::slot_source(s);
                        let outs = bindings.outputs(src);
                        let bound = !outs.is_empty();
                        let fill = match (bound, hover) {
                            (true, true) => theme::p().accent_hi,
                            (true, false) => theme::p().accent_dim,
                            (false, true) => theme::p().card_hi,
                            (false, false) => theme::p().card,
                        };
                        let stroke = if selected == Some(src) {
                            Stroke::new(2.5_f32, theme::p().select)
                        } else if bound {
                            Stroke::new(1.0_f32, theme::p().accent_hi)
                        } else {
                            Stroke::new(1.0_f32, theme::p().line)
                        };
                        SlotLook {
                            fill,
                            stroke,
                            text: theme::p().text,
                            sub: outputs_summary(outs),
                        }
                    },
                );
                if let Some(i) = hovered {
                    let src = layout::slot_source(&slots[i]);
                    let outs = self.cfg.bindings.outputs(src);
                    let text = if outs.is_empty() {
                        format!("{src}: not mapped")
                    } else {
                        let names: Vec<&str> = outs.iter().map(|o| o.name()).collect();
                        format!("{src} drives {}", names.join(", "))
                    };
                    resp.on_hover_text_at_pointer(text);
                }
                if let Some(i) = clicked {
                    self.selected_input = Some(layout::slot_source(&slots[i]));
                }
                ui.add_space(8.0);
                legend(ui);
            });
    }

    fn input_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(src) = self.selected_input else {
            theme::title(ui, "Nothing selected");
            ui.add_space(4.0);
            theme::help(
                ui,
                "Pick a key or button on the left. Mapped ones are green and show what they do.",
            );
            ui.add_space(12.0);
            theme::help(
                ui,
                "A key can drive several controller outputs, and several keys can drive the same \
                 output.",
            );
            return;
        };
        ui.label(egui::RichText::new(src.to_string()).size(26.0).strong());
        theme::help(ui, input_kind(src));
        ui.add_space(10.0);

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Mapped to");
            let outs: Vec<Output> = self.cfg.bindings.outputs(src).to_vec();
            if outs.is_empty() {
                let what = match self.cfg.general.unbound {
                    UnboundPolicy::Pass => "Nothing. It keeps working as a normal key in Windows.",
                    UnboundPolicy::Block => {
                        "Nothing. It is blocked while capturing (see Settings, unmapped keys)."
                    }
                };
                theme::help(ui, what);
            }
            for o in outs {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(o.name())
                            .color(theme::p().accent)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            self.cfg.bindings.remove(src, o);
                        }
                    });
                });
            }
        });

        ui.add_space(10.0);
        theme::title(ui, "Map to");
        theme::help(ui, "Click to add or remove. Highlighted means it's mapped.");
        ui.add_space(4.0);
        for (group, outs) in layout::OUTPUT_GROUPS {
            ui.label(
                egui::RichText::new(*group)
                    .size(12.5)
                    .color(theme::p().text_dim),
            );
            ui.horizontal_wrapped(|ui| {
                for &o in *outs {
                    let on = self.cfg.bindings.outputs(src).contains(&o);
                    let text = egui::RichText::new(o.name()).size(13.0).color(if on {
                        theme::p().on_accent
                    } else {
                        theme::p().text
                    });
                    let b = egui::Button::new(text).fill(if on {
                        theme::p().accent
                    } else {
                        theme::p().card_hi
                    });
                    let resp = ui.add(b);
                    let resp = if o == Output::Walk {
                        resp.on_hover_text(
                            "Not a controller button: while held, keyboard-driven left-stick \
                             movement is scaled down (Settings, walk scale).",
                        )
                    } else {
                        resp
                    };
                    if resp.clicked() {
                        self.cfg.bindings.toggle(src, o);
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(8.0);
        if ui
            .add_enabled(
                self.cfg.bindings.contains(src),
                egui::Button::new("Clear all mappings for this input"),
            )
            .clicked()
        {
            self.cfg.bindings.clear_source(src);
        }
    }

    pub fn controller_tab(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("pad-inspector")
            .exact_width(360.0)
            .resizable(false)
            .frame(theme::side())
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.output_inspector(ui));
            });

        egui::CentralPanel::default()
            .frame(theme::clear(16.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Click a controller button to see and change which keys drive it.",
                    )
                    .color(theme::p().text_dim),
                );
                let selected = self.selected_output;
                let bindings = &self.cfg.bindings;
                let max_h = (ui.available_height() - 40.0).min(620.0);
                let (clicked, hovered, resp) = slot_canvas(
                    ui,
                    layout::PAD,
                    layout::PAD_SIZE,
                    max_h,
                    draw_pad_body,
                    |s, hover| {
                        let out = layout::slot_output(s);
                        let srcs = bindings.sources_for(out);
                        let bound = !srcs.is_empty();
                        let fill = match (bound, hover) {
                            (true, true) => theme::p().accent_hi,
                            (true, false) => theme::p().accent_dim,
                            (false, true) => theme::p().card_hi,
                            (false, false) => theme::p().card,
                        };
                        let stroke = if selected == Some(out) {
                            Stroke::new(2.5_f32, theme::p().select)
                        } else {
                            Stroke::new(1.0_f32, theme::p().line)
                        };
                        let sub = match srcs.as_slice() {
                            [] => None,
                            [a] => Some(a.to_string()),
                            [a, rest @ ..] => Some(format!("{a} +{}", rest.len())),
                        };
                        SlotLook {
                            fill,
                            stroke,
                            text: theme::p().text,
                            sub,
                        }
                    },
                );
                if let Some(i) = hovered {
                    let out = layout::slot_output(&layout::PAD[i]);
                    let srcs = self.cfg.bindings.sources_for(out);
                    let text = if srcs.is_empty() {
                        format!("{}: no input", out.name())
                    } else {
                        let names: Vec<String> = srcs.iter().map(|s| s.to_string()).collect();
                        format!("{} is driven by {}", out.name(), names.join(", "))
                    };
                    resp.on_hover_text_at_pointer(text);
                }
                if let Some(i) = clicked {
                    self.selected_output = Some(layout::slot_output(&layout::PAD[i]));
                }
                ui.add_space(8.0);
                legend(ui);
            });
    }

    fn output_inspector(&mut self, ui: &mut egui::Ui) {
        let Some(out) = self.selected_output else {
            theme::title(ui, "Nothing selected");
            ui.add_space(4.0);
            theme::help(ui, "Pick a controller button on the left.");
            return;
        };
        ui.label(egui::RichText::new(out.name()).size(26.0).strong());
        theme::help(
            ui,
            if out == Output::Walk {
                "Modifier: while held, keyboard-driven left-stick movement is scaled down."
            } else {
                "Controller output"
            },
        );
        ui.add_space(10.0);
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Driven by");
            let srcs = self.cfg.bindings.sources_for(out);
            if srcs.is_empty() {
                theme::help(ui, "Nothing yet.");
            }
            for s in srcs {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(s.to_string())
                            .color(theme::p().accent)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Remove").clicked() {
                            self.cfg.bindings.remove(s, out);
                        }
                    });
                });
            }
        });
        ui.add_space(10.0);
        theme::title(ui, "Add an input");
        ui.horizontal(|ui| {
            let current = self
                .add_input
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Choose…".to_string());
            egui::ComboBox::from_id_salt("add-input")
                .width(200.0)
                .height(360.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for name in keys::all_names() {
                        if let Ok(src) = Source::parse(name) {
                            ui.selectable_value(&mut self.add_input, Some(src), name);
                        }
                    }
                });
            if ui
                .add_enabled(self.add_input.is_some(), egui::Button::new("Add"))
                .clicked()
            {
                if let Some(src) = self.add_input.take() {
                    self.cfg.bindings.add(src, out);
                }
            }
        });
        ui.add_space(6.0);
        theme::help(
            ui,
            "Tip: the Keyboard and Mouse buttons pages let you click the key itself instead.",
        );
    }
}

fn legend(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        swatch(ui, theme::p().accent_dim, theme::p().accent_hi);
        ui.label(egui::RichText::new("mapped").color(theme::p().text_dim));
        ui.add_space(10.0);
        swatch(ui, theme::p().card, theme::p().line);
        ui.label(egui::RichText::new("not mapped").color(theme::p().text_dim));
        ui.add_space(10.0);
        swatch(ui, theme::p().card, theme::p().select);
        ui.label(egui::RichText::new("selected").color(theme::p().text_dim));
    });
}

fn swatch(ui: &mut egui::Ui, fill: Color32, stroke: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
    ui.painter()
        .rect(r, 3.0, fill, Stroke::new(1.5_f32, stroke));
}
