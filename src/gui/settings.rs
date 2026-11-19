//! Settings page: hotkeys and general behaviour.

use crate::theme;
use crate::PadApp;
use eframe::egui;
use padbridge::bindings::UnboundPolicy;
use padbridge::config::{InputMethod, Socd};
use padbridge::driver::DriverError;
use std::time::{Duration, Instant};

const PICK_SECONDS: u64 = 5;

/// padbridge's name for a key pressed in the app window (letters, digits, F-keys, navigation and
/// punctuation keys); `None` for keys it cannot name.
fn egui_key_name(key: egui::Key) -> Option<String> {
    let dbg = format!("{key:?}");
    let name = if let Some(d) = dbg.strip_prefix("Num") {
        d.to_string()
    } else if let Some(a) = dbg.strip_prefix("Arrow") {
        a.to_string()
    } else {
        dbg
    };
    padbridge::keys::Source::parse(&name)
        .ok()
        .map(|s| s.to_string())
}

impl PadApp {
    pub fn settings_tab(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(theme::clear(16.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.set_max_width(900.0);
                    self.appearance_card(ui);
                    ui.add_space(12.0);
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

    fn appearance_card(&mut self, ui: &mut egui::Ui) {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Appearance");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for t in theme::Theme::ALL {
                    let on = theme::current() == t;
                    let text = egui::RichText::new(t.label()).color(if on {
                        theme::p().on_accent
                    } else {
                        theme::p().text
                    });
                    let b = egui::Button::new(text)
                        .fill(if on {
                            theme::p().accent
                        } else {
                            theme::p().card_hi
                        })
                        .rounding(18.0)
                        .min_size(egui::vec2(0.0, 32.0));
                    if ui.add(b).clicked() && !on {
                        theme::set(ui.ctx(), t);
                        if let Some(store) = &self.store {
                            let _ = std::fs::write(store.dir().join(".theme"), t.key());
                        }
                    }
                }
            });
            if theme::current() == theme::Theme::Clear {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Background opacity");
                    let mut pct = theme::opacity() * 100.0;
                    let r = ui.add(
                        egui::Slider::new(&mut pct, 0.0..=100.0)
                            .suffix(" %")
                            .max_decimals(0),
                    );
                    if r.changed() {
                        theme::set_opacity(pct / 100.0);
                    }
                    if r.drag_stopped() || (r.changed() && !r.dragged()) {
                        if let Some(store) = &self.store {
                            let _ = std::fs::write(
                                store.dir().join(".opacity"),
                                format!("{:.2}", theme::opacity()),
                            );
                        }
                    }
                });
                theme::help(
                    ui,
                    "0 % = fully clear (your desktop or game shows through), 100 % = solid. Panels \
                     and cards keep their own tint so text stays readable.",
                );
            }
        });
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
                                        .color(theme::p().select),
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
                    let (text, color) = match &self.driver_status {
                        Ok(()) => ("Installed and ready", theme::p().accent),
                        Err(DriverError::Unsupported) => ("Windows only", theme::p().text_dim),
                        Err(e) => (e.short(), theme::p().select),
                    };
                    ui.label(egui::RichText::new(text).color(color).strong());
                    ui.horizontal(|ui| {
                        if ui.button("Check again").clicked() {
                            self.driver_status = padbridge::driver::status();
                            self.driver_checked = Instant::now();
                        }
                        let ago = self.driver_checked.elapsed().as_secs();
                        let when = if ago < 2 {
                            "checked just now".to_string()
                        } else if ago < 120 {
                            format!("checked {ago} s ago")
                        } else {
                            format!("checked {} min ago", ago / 60)
                        };
                        ui.label(
                            egui::RichText::new(when)
                                .size(12.5)
                                .color(theme::p().text_dim),
                        );
                    });
                    ui.end_row();
                });
            if let Some(rt) = &self.runtime {
                ui.add_space(4.0);
                theme::help(
                    ui,
                    &format!("Mapping is running with: {}.", rt.backend().describe()),
                );
            }
            if let Err(e) = &self.driver_status {
                if *e != DriverError::Unsupported {
                    ui.add_space(4.0);
                    egui::Frame::default()
                        .fill(theme::p().inset)
                        .rounding(6.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new(e.advice()).color(theme::p().text));
                        });
                }
            }
            ui.add_space(10.0);
            self.driver_tools(ui);
        });
    }

    /// Collects a finished driver diagnosis.
    pub fn poll_diag(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.diag_rx else { return };
        match rx.try_recv() {
            Ok(facts) => {
                self.driver_status = facts.status.clone();
                self.driver_checked = Instant::now();
                self.diag = Some(facts);
                self.diag_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.diag_rx = None;
                self.error("The driver check stopped unexpectedly.");
            }
        }
    }

    fn driver_tools(&mut self, ui: &mut egui::Ui) {
        theme::title(ui, "Driver diagnosis");
        ui.horizontal(|ui| {
            if self.diag_rx.is_some() {
                ui.add(egui::Spinner::new());
                ui.label("Checking files, services, filters and Windows security…");
            } else if ui
                .button("Diagnose driver")
                .on_hover_text(
                    "Checks every step the Interception driver needs and says what to do",
                )
                .clicked()
            {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(padbridge::diag::gather());
                });
                self.diag_rx = Some(rx);
            }
        });
        if let Some(facts) = &self.diag {
            let v = padbridge::diag::conclude(facts);
            let report = padbridge::diag::report(facts);
            egui::Frame::default()
                .fill(theme::p().inset)
                .rounding(10.0)
                .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        egui::RichText::new(&v.title)
                            .strong()
                            .size(15.0)
                            .color(if v.ok {
                                theme::p().accent
                            } else {
                                theme::p().select
                            }),
                    );
                    ui.label(egui::RichText::new(&v.advice).color(theme::p().text));
                    ui.add_space(4.0);
                    egui::CollapsingHeader::new("Details")
                        .id_salt("diag-details")
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(&report).monospace().size(12.0));
                        });
                    if ui
                        .button("Copy report")
                        .on_hover_text("Paste it to Claude if you need help")
                        .clicked()
                    {
                        ui.output_mut(|o| o.copied_text = report.clone());
                        self.info("Driver report copied to the clipboard.");
                    }
                });
        }

        ui.add_space(8.0);
        theme::title(ui, "Install or remove the driver");
        theme::help(
            ui,
            "Uses install-interception.exe from Interception.zip (github.com/oblitum/Interception \
             /releases, folder 'command line installer'). Windows asks for administrator \
             permission; restart the PC afterwards.",
        );
        ui.horizontal(|ui| {
            ui.add_sized(
                [360.0, 26.0],
                egui::TextEdit::singleline(&mut self.installer_path)
                    .hint_text("path to install-interception.exe"),
            );
            if ui
                .button("Find")
                .on_hover_text("Search padbridge's folder and Downloads")
                .clicked()
            {
                match padbridge::diag::find_installer() {
                    Some(p) => self.installer_path = p.display().to_string(),
                    None => self.error(
                        "install-interception.exe was not found next to padbridge or in \
                         Downloads. Download and extract Interception.zip first.",
                    ),
                }
            }
        });
        ui.horizontal(|ui| {
            let path = std::path::PathBuf::from(self.installer_path.trim());
            let ready = path.is_file();
            for (label, uninstall) in [("Install driver", false), ("Uninstall driver", true)] {
                if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
                    match padbridge::diag::run_installer(&path, uninstall) {
                        Ok(()) => self.info(format!(
                            "{label}: approve the Windows prompt, read the result in the window \
                             that opens, then restart the PC."
                        )),
                        Err(e) => self.error(e),
                    }
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
                 Click Record and press the combination, or type keys joined by '+'. Avoid F-keys \
                 that are also media keys on your keyboard (F10 is mute on many), or using the \
                 hotkey will mute Windows too.",
            );
            ui.add_space(6.0);
            egui::Grid::new("hotkeys")
                .num_columns(3)
                .spacing([14.0, 10.0])
                .show(ui, |ui| {
                    for (i, label) in [(0usize, "Pause / resume capture"), (1, "Stop mapping")] {
                        ui.label(label);
                        ui.horizontal(|ui| {
                            if self.recording == Some(i) {
                                ui.add_sized(
                                    [220.0, 26.0],
                                    egui::Label::new(
                                        egui::RichText::new("Press the keys… (Esc cancels)")
                                            .color(theme::p().select),
                                    ),
                                );
                                let pressed = ui.input(|inp| {
                                    inp.events.iter().find_map(|e| match e {
                                        egui::Event::Key {
                                            key,
                                            pressed: true,
                                            repeat: false,
                                            modifiers,
                                            ..
                                        } => Some((*key, *modifiers)),
                                        _ => None,
                                    })
                                });
                                if let Some((key, m)) = pressed {
                                    if key == egui::Key::Escape && !m.any() {
                                        self.recording = None;
                                    } else if let Some(name) = egui_key_name(key) {
                                        let mut parts = Vec::new();
                                        if m.ctrl {
                                            parts.push("Ctrl".to_string());
                                        }
                                        if m.shift {
                                            parts.push("Shift".to_string());
                                        }
                                        if m.alt {
                                            parts.push("Alt".to_string());
                                        }
                                        parts.push(name);
                                        self.hotkey_text[i] = parts.join("+");
                                        self.recording = None;
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    self.recording = None;
                                }
                            } else {
                                ui.add_sized(
                                    [220.0, 26.0],
                                    egui::TextEdit::singleline(&mut self.hotkey_text[i]),
                                );
                                if ui
                                    .button("Record")
                                    .on_hover_text("Click, then press the key combination")
                                    .clicked()
                                {
                                    self.recording = Some(i);
                                }
                            }
                        });
                        match &self.hotkey_err[i] {
                            Some(e) => {
                                ui.label(egui::RichText::new(e).color(theme::p().danger));
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
                    ui.label("Tone when capture changes");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.cfg.general.beep_on_toggle, "");
                        if ui.small_button("Test sound").clicked() {
                            padbridge::sound::play_toggle(true);
                        }
                    });
                    theme::help(ui, "A short quiet tone that plays alongside game audio.");
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
