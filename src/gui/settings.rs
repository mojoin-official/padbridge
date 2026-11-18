//! Settings page: hotkeys and general behaviour.

use crate::theme;
use crate::PadApp;
use eframe::egui;
use padbridge::bindings::UnboundPolicy;
use padbridge::config::Socd;

impl PadApp {
    pub fn settings_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::same(16.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_max_width(900.0);
                    self.hotkey_card(ui);
                    ui.add_space(12.0);
                    self.keyboard_card(ui);
                    ui.add_space(12.0);
                    self.advanced_card(ui);
                });
            });
    }

    fn hotkey_card(&mut self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Hotkeys");
            theme::help(
                ui,
                "These always work, even while everything is captured, and never reach the game. \
                 Write keys joined by '+', e.g. Ctrl+Alt+F10. Ctrl, Shift, Alt and Win mean \
                 either side.",
            );
            ui.add_space(6.0);
            egui::Grid::new("hotkeys")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    for (i, label) in [(0usize, "Pause / resume capture"), (1, "Stop mapping")] {
                        ui.label(label);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.hotkey_text[i])
                                .desired_width(200.0),
                        );
                        match &self.hotkey_err[i] {
                            Some(e) => {
                                ui.label(egui::RichText::new(e).color(theme::DANGER));
                            }
                            None => {
                                ui.label("");
                            }
                        }
                        ui.end_row();
                    }
                    ui.label("Start with capture on");
                    ui.checkbox(&mut self.cfg.general.start_enabled, "");
                    theme::help(
                        ui,
                        "Off: Start only arms padbridge; press the hotkey to capture.",
                    );
                    ui.end_row();
                    ui.label("Beep when capture changes");
                    ui.checkbox(&mut self.cfg.general.beep_on_toggle, "");
                    theme::help(ui, "Handy in fullscreen games.");
                    ui.end_row();
                });
        });
    }

    fn keyboard_card(&mut self, ui: &mut egui::Ui) {
        let g = &mut self.cfg.general;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Keyboard behaviour");
            ui.add_space(6.0);
            egui::Grid::new("kb")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Unmapped keys while capturing");
                    egui::ComboBox::from_id_salt("unbound")
                        .width(220.0)
                        .selected_text(match g.unbound {
                            UnboundPolicy::Pass => "Pass to Windows",
                            UnboundPolicy::Block => "Block",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut g.unbound,
                                UnboundPolicy::Pass,
                                "Pass to Windows",
                            );
                            ui.selectable_value(&mut g.unbound, UnboundPolicy::Block, "Block");
                        });
                    theme::help(ui, "Block = the game sees only the controller.");
                    ui.end_row();

                    ui.label("Opposite directions held");
                    egui::ComboBox::from_id_salt("socd")
                        .width(220.0)
                        .selected_text(match g.socd {
                            Socd::Last => "Last pressed wins",
                            Socd::Neutral => "Cancel out",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut g.socd, Socd::Last, "Last pressed wins");
                            ui.selectable_value(&mut g.socd, Socd::Neutral, "Cancel out");
                        });
                    theme::help(ui, "e.g. A and D held together.");
                    ui.end_row();

                    ui.label("Round diagonals");
                    ui.checkbox(&mut g.normalize_diagonals, "");
                    theme::help(ui, "W+D gives a full-length diagonal like a real stick.");
                    ui.end_row();

                    ui.label("Stick ramp time");
                    ui.add(
                        egui::Slider::new(&mut g.stick_ramp_ms, 0.0..=500.0)
                            .suffix(" ms")
                            .max_decimals(0),
                    );
                    theme::help(
                        ui,
                        "Keyboard sticks ease to full instead of jumping. 0 = instant.",
                    );
                    ui.end_row();

                    ui.label("Walk scale");
                    ui.add(egui::Slider::new(&mut g.walk_scale, 0.05..=1.0).max_decimals(2));
                    theme::help(ui, "Left-stick length while the Walk input is held.");
                    ui.end_row();

                    ui.label("Trigger strength");
                    ui.add(egui::Slider::new(&mut g.trigger_value, 1..=255));
                    theme::help(ui, "How far LT / RT are pulled when their key is held.");
                    ui.end_row();

                    ui.label("Wheel press length");
                    ui.add(
                        egui::Slider::new(&mut g.wheel_pulse_ms, 5.0..=500.0)
                            .suffix(" ms")
                            .max_decimals(0),
                    );
                    theme::help(ui, "Each wheel notch becomes one press this long.");
                    ui.end_row();
                });
        });
    }

    fn advanced_card(&mut self, ui: &mut egui::Ui) {
        let g = &mut self.cfg.general;
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Advanced");
            ui.add_space(6.0);
            egui::Grid::new("adv")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Update rate");
                    ui.add(egui::Slider::new(&mut g.output_hz, 60..=1000).suffix(" Hz"));
                    theme::help(ui, "Higher = lower latency, more CPU. 500 is plenty.");
                    ui.end_row();

                    ui.label("Ignore other programs' input");
                    ui.checkbox(&mut g.ignore_injected, "");
                    theme::help(ui, "Skip keys sent by on-screen keyboards or other tools.");
                    ui.end_row();
                });
        });
        ui.add_space(12.0);
        if let Some(store) = &self.store {
            theme::help(
                ui,
                &format!("Profiles are saved in {}", store.dir().display()),
            );
        }
    }
}
