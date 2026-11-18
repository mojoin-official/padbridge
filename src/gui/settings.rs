//! Settings page: hotkeys and general behaviour.

use crate::theme;
use crate::PadApp;
use eframe::egui;
use padbridge::bindings::UnboundPolicy;
use padbridge::config::{InputMethod, Socd};
use padbridge::driver::DriverError;
use std::time::{Duration, Instant};

const PICK_SECONDS: u64 = 5;

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
                    self.input_card(ui);
                    ui.add_space(12.0);
                    self.hotkey_card(ui);
                    ui.add_space(12.0);
                    self.keyboard_card(ui);
                    ui.add_space(12.0);
                    self.advanced_card(ui);
                });
            });
    }

    /// Finishes the "pick the game window" countdown.
    pub fn poll_pick(&mut self, ctx: &egui::Context) {
        let Some(deadline) = self.pick_deadline else {
            return;
        };
        let now = Instant::now();
        if now < deadline {
            ctx.request_repaint_after(Duration::from_millis(200));
            return;
        }
        self.pick_deadline = None;
        match padbridge::app::foreground_program() {
            Some(exe) if exe.to_lowercase().starts_with("padbridge") => {
                self.error(
                    "padbridge was still in front. Click Pick again, then switch to the game.",
                );
            }
            Some(exe) => {
                self.cfg.general.game_exe = exe.clone();
                self.info(format!("Game set to {exe}. Remember to Save."));
            }
            None => self.error("Could not read which program was in front."),
        }
    }

    fn input_card(&mut self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Game and input");
            theme::help(
                ui,
                "To make the game see only the controller: install the Interception driver, set \
                 your game below, and (optionally) block unmapped keys further down.",
            );
            ui.add_space(6.0);
            egui::Grid::new("input-grid")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    ui.label("Only capture while this is in front");
                    ui.add_sized(
                        [220.0, 26.0],
                        egui::TextEdit::singleline(&mut self.cfg.general.game_exe)
                            .hint_text("e.g. game.exe (empty = always)"),
                    );
                    ui.horizontal(|ui| {
                        match self.pick_deadline {
                            Some(d) => {
                                let left =
                                    d.saturating_duration_since(Instant::now()).as_secs() + 1;
                                ui.label(
                                    egui::RichText::new(format!("Switch to the game… {left}"))
                                        .color(theme::SELECT),
                                );
                            }
                            None => {
                                if ui
                                    .button("Pick game window")
                                    .on_hover_text(format!(
                                        "Click, then switch to the game within {PICK_SECONDS} \
                                         seconds; padbridge reads which program is in front."
                                    ))
                                    .clicked()
                                {
                                    self.pick_deadline =
                                        Some(Instant::now() + Duration::from_secs(PICK_SECONDS));
                                }
                            }
                        }
                        if !self.cfg.general.game_exe.is_empty() && ui.button("Clear").clicked() {
                            self.cfg.general.game_exe.clear();
                        }
                    });
                    ui.end_row();

                    ui.label("Input method");
                    let im = &mut self.cfg.general.input_method;
                    egui::ComboBox::from_id_salt("input-method")
                        .width(220.0)
                        .selected_text(match im {
                            InputMethod::Auto => "Driver if installed",
                            InputMethod::Hooks => "Windows hooks only",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(im, InputMethod::Auto, "Driver if installed");
                            ui.selectable_value(im, InputMethod::Hooks, "Windows hooks only");
                        });
                    theme::help(
                        ui,
                        "The driver hides captured input from the game completely.",
                    );
                    ui.end_row();

                    ui.label("Interception driver");
                    let (text, color) = match self.driver_status {
                        Ok(()) => ("Installed and ready", theme::ACCENT),
                        Err(DriverError::NotInstalled) => ("Not installed", theme::SELECT),
                        Err(DriverError::DllMissing) => ("interception.dll missing", theme::SELECT),
                        Err(DriverError::Unsupported) => ("Windows only", theme::TEXT_DIM),
                    };
                    ui.label(egui::RichText::new(text).color(color).strong());
                    if ui
                        .add_enabled(self.runtime.is_none(), egui::Button::new("Check again"))
                        .clicked()
                    {
                        self.driver_status = padbridge::driver::status();
                    }
                    ui.end_row();
                });
            if let Err(e) = self.driver_status {
                if e != DriverError::Unsupported {
                    ui.add_space(4.0);
                    theme::help(
                        ui,
                        "To install it: download Interception.zip from \
                         github.com/oblitum/Interception/releases, open a Command Prompt as \
                         administrator in its 'command line installer' folder, run \
                         install-interception.exe /install, then restart the PC. Keep \
                         interception.dll next to padbridge.exe (it is included in the download).",
                    );
                    theme::help(ui, &format!("Status detail: {e}."));
                }
            }
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
                        ui.add_sized(
                            [220.0, 26.0],
                            egui::TextEdit::singleline(&mut self.hotkey_text[i]),
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
