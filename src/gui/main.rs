//! padbridge window app: edit bindings visually, tune mouse-to-stick, and run the mapping.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod settings;
mod stick;
mod theme;
mod views;

use eframe::egui;
use padbridge::app::Backend;
use padbridge::app::{RunOptions, Runtime};
use padbridge::bindings::Combo;
use padbridge::config::Config;
use padbridge::driver::DriverError;
use padbridge::keys::Source;
use padbridge::pad::Output;
use padbridge::profiles::ProfileStore;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tab {
    Keyboard,
    Mouse,
    Controller,
    Stick,
    Settings,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::Keyboard,
        Tab::Mouse,
        Tab::Controller,
        Tab::Stick,
        Tab::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Tab::Keyboard => "Keyboard",
            Tab::Mouse => "Mouse buttons",
            Tab::Controller => "Controller",
            Tab::Stick => "Mouse to stick",
            Tab::Settings => "Settings",
        }
    }

    fn from_env(s: &str) -> Option<Tab> {
        match s.to_ascii_lowercase().as_str() {
            "keyboard" => Some(Tab::Keyboard),
            "mouse" => Some(Tab::Mouse),
            "controller" => Some(Tab::Controller),
            "stick" => Some(Tab::Stick),
            "settings" => Some(Tab::Settings),
            _ => None,
        }
    }
}

#[derive(PartialEq, Eq)]
enum Dialog {
    None,
    NewProfile,
    ConfirmDelete,
    ConfirmSwitch(String),
}

pub struct PadApp {
    store: Option<ProfileStore>,
    profiles: Vec<String>,
    profile: String,
    pub cfg: Config,
    saved_text: String,
    /// Toggle and exit hotkey text as typed (may be temporarily invalid).
    pub hotkey_text: [String; 2],
    pub hotkey_err: [Option<String>; 2],
    /// Which hotkey field is waiting for a key combination to be pressed.
    pub recording: Option<usize>,
    pub tab: Tab,
    pub selected_input: Option<Source>,
    pub selected_output: Option<Output>,
    /// Input picked in the controller tab's "add input" list.
    pub add_input: Option<Source>,
    runtime: Option<Runtime>,
    /// Profile text the running mapping was started with.
    running_text: String,
    pub test_mode: bool,
    log: VecDeque<String>,
    status: Option<(String, bool)>,
    dialog: Dialog,
    new_name: String,
    /// Calibration helper inputs per mouse axis: counts per 360, seconds per 360.
    pub calib: [[f64; 2]; 2],
    /// Whether the Interception driver can be used (checked at start and on request).
    pub driver_status: Result<(), DriverError>,
    pub driver_checked: Instant,
    /// "Pick the game window" countdown.
    pub pick_deadline: Option<Instant>,
}

impl PadApp {
    fn new(cc: &eframe::CreationContext<'_>) -> PadApp {
        theme::apply(&cc.egui_ctx);
        let mut app = PadApp {
            store: None,
            profiles: Vec::new(),
            profile: String::new(),
            cfg: Config::builtin(),
            saved_text: String::new(),
            hotkey_text: [String::new(), String::new()],
            hotkey_err: [None, None],
            recording: None,
            tab: Tab::Keyboard,
            selected_input: None,
            selected_output: None,
            add_input: None,
            runtime: None,
            running_text: String::new(),
            test_mode: false,
            log: VecDeque::new(),
            status: None,
            dialog: Dialog::None,
            new_name: String::new(),
            calib: [[16000.0, 2.0], [16000.0, 2.0]],
            driver_status: padbridge::driver::status(),
            driver_checked: Instant::now(),
            pick_deadline: None,
        };
        match ProfileStore::open(ProfileStore::default_dir()) {
            Ok(store) => {
                let first = store
                    .last_used()
                    .or_else(|| store.list().into_iter().next())
                    .unwrap_or_else(|| "Default".to_string());
                app.profiles = store.list();
                app.store = Some(store);
                app.load_profile(&first);
            }
            Err(e) => {
                app.set_loaded(String::from("(unsaved)"), Config::builtin());
                app.error(format!("{e}. Changes cannot be saved."));
            }
        }
        // Appearance: PADBRIDGE_THEME (for screenshots), else the saved choice, else Aurora.
        let saved_theme = app
            .store
            .as_ref()
            .and_then(|s| std::fs::read_to_string(s.dir().join(".theme")).ok());
        let chosen = std::env::var("PADBRIDGE_THEME")
            .ok()
            .or(saved_theme)
            .and_then(|t| theme::Theme::from_key(&t))
            .unwrap_or(theme::Theme::Aurora);
        if let Some(v) = app
            .store
            .as_ref()
            .and_then(|s| std::fs::read_to_string(s.dir().join(".opacity")).ok())
            .and_then(|t| t.trim().parse::<f32>().ok())
        {
            theme::set_opacity(v);
        }
        if let Some(v) = std::env::var("PADBRIDGE_OPACITY")
            .ok()
            .and_then(|t| t.trim().parse::<f32>().ok())
        {
            theme::set_opacity(v);
        }
        theme::set(&cc.egui_ctx, chosen);
        // Used for screenshots and for jumping straight to a page.
        if let Some(t) = std::env::var("PADBRIDGE_TAB")
            .ok()
            .and_then(|t| Tab::from_env(&t))
        {
            app.tab = t;
        }
        if let Ok(s) = std::env::var("PADBRIDGE_SELECT") {
            app.selected_input = Source::parse(&s).ok();
        }
        if let Ok(s) = std::env::var("PADBRIDGE_SELECT_OUTPUT") {
            app.selected_output = Output::parse(&s);
        }
        app
    }

    pub fn info(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), false));
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), true));
    }

    fn set_loaded(&mut self, name: String, cfg: Config) {
        self.hotkey_text = [
            cfg.general.toggle_hotkey.to_string(),
            cfg.general.exit_hotkey.to_string(),
        ];
        self.hotkey_err = [None, None];
        self.saved_text = cfg.to_toml();
        self.cfg = cfg;
        self.profile = name;
        self.selected_input = None;
        self.selected_output = None;
    }

    fn load_profile(&mut self, name: &str) {
        let Some(store) = &self.store else { return };
        match store.load(name) {
            Ok(mut cfg) => {
                store.set_last_used(name);
                // Old profiles used Ctrl+Alt+F10/F12, which mute Windows on keyboards where F10
                // is the mute key. Switch them over and save, so the fix sticks.
                let migrated = cfg.migrate_hotkeys() && store.save(name, &cfg).is_ok();
                self.set_loaded(name.to_string(), cfg);
                if migrated {
                    self.info(format!(
                        "Loaded '{name}'. Hotkeys changed to {} (pause/resume) and {} (stop): \
                         the old F10/F12 ones muted Windows on many keyboards.",
                        padbridge::config::DEFAULT_TOGGLE_HOTKEY,
                        padbridge::config::DEFAULT_EXIT_HOTKEY
                    ));
                } else {
                    self.info(format!("Loaded profile '{name}'"));
                }
            }
            Err(e) => self.error(e),
        }
    }

    fn save_profile(&mut self) {
        if let Err(e) = self.validate() {
            self.error(format!("Not saved: {e}"));
            return;
        }
        let Some(store) = &self.store else {
            self.error("The profiles folder is not available, so nothing can be saved");
            return;
        };
        match store.save(&self.profile, &self.cfg) {
            Ok(()) => {
                store.set_last_used(&self.profile);
                self.saved_text = self.cfg.to_toml();
                let msg = format!("Saved '{}'", self.profile);
                self.info(msg);
            }
            Err(e) => self.error(e),
        }
    }

    fn dirty(&self) -> bool {
        self.cfg.to_toml() != self.saved_text
    }

    /// Applies the hotkey text fields and checks the whole configuration.
    pub fn validate(&mut self) -> Result<(), String> {
        for i in 0..2 {
            match Combo::parse(&self.hotkey_text[i]) {
                Ok(c) => {
                    self.hotkey_err[i] = None;
                    if i == 0 {
                        self.cfg.general.toggle_hotkey = c;
                    } else {
                        self.cfg.general.exit_hotkey = c;
                    }
                }
                Err(e) => self.hotkey_err[i] = Some(e),
            }
        }
        if let Some(e) = self.hotkey_err.iter().flatten().next() {
            return Err(format!("hotkey: {e}"));
        }
        Config::parse(&self.cfg.to_toml())
            .map(|_| ())
            .map_err(|e| e.message)
    }

    fn running(&self) -> bool {
        self.runtime.is_some()
    }

    fn start(&mut self) {
        if let Err(e) = self.validate() {
            self.error(format!("Cannot start: {e}"));
            return;
        }
        let opts = RunOptions {
            dry_run: self.test_mode,
            sniff: false,
            print_reports: false,
        };
        match Runtime::start(&self.cfg, opts) {
            Ok(rt) => {
                self.running_text = self.cfg.to_toml();
                self.runtime = Some(rt);
                self.info(if self.test_mode {
                    "Test mode running: no controller is created, watch the live stick preview"
                } else {
                    "Mapping started"
                });
            }
            Err(e) => {
                let hint = if !self.test_mode && e.contains("ViGEmBus") {
                    " Tick 'Test mode' to try it without the driver."
                } else {
                    ""
                };
                self.error(format!("Could not start: {e}{hint}"));
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut rt) = self.runtime.take() {
            rt.stop();
            for l in rt.drain_log() {
                self.push_log(l);
            }
            self.info("Mapping stopped; keyboard and mouse are back to normal");
        }
    }

    fn push_log(&mut self, line: String) {
        if self.log.len() >= 300 {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    /// Pulls log lines and notices if mapping ended on its own (exit hotkey or an error).
    fn poll_runtime(&mut self) {
        let Some(rt) = &self.runtime else { return };
        let lines = rt.drain_log();
        let ended = !rt.is_running();
        for l in lines {
            self.push_log(l);
        }
        if ended {
            if let Some(mut rt) = self.runtime.take() {
                rt.stop();
                for l in rt.drain_log() {
                    self.push_log(l);
                }
            }
            self.info("Mapping ended (exit hotkey). Press Start to run it again.");
        }
    }

    pub fn live_report(&self) -> Option<padbridge::pad::Report> {
        self.runtime.as_ref().map(Runtime::last_report)
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(theme::bar())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("padbridge")
                            .size(22.0)
                            .strong()
                            .color(theme::p().accent),
                    );
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("Profile").color(theme::p().text_dim));
                    let current = self.profile.clone();
                    let mut pick: Option<String> = None;
                    egui::ComboBox::from_id_salt("profile")
                        .width(180.0)
                        .selected_text(current.clone())
                        .show_ui(ui, |ui| {
                            for p in &self.profiles {
                                if ui.selectable_label(*p == current, p).clicked() && *p != current
                                {
                                    pick = Some(p.clone());
                                }
                            }
                        });
                    if let Some(p) = pick {
                        if self.dirty() {
                            self.dialog = Dialog::ConfirmSwitch(p);
                        } else {
                            self.load_profile(&p);
                        }
                    }
                    let can_save = self.store.is_some();
                    let save_label = if self.dirty() { "Save *" } else { "Save" };
                    let save_btn = if self.dirty() {
                        egui::Button::new(
                            egui::RichText::new(save_label)
                                .color(theme::p().on_accent)
                                .strong(),
                        )
                        .fill(theme::p().accent)
                    } else {
                        egui::Button::new(save_label)
                    };
                    if ui.add_enabled(can_save, save_btn).clicked() {
                        self.save_profile();
                    }
                    if ui
                        .add_enabled(can_save, egui::Button::new("New…"))
                        .clicked()
                    {
                        self.new_name.clear();
                        self.dialog = Dialog::NewProfile;
                    }
                    if ui
                        .add_enabled(
                            can_save && self.profiles.len() > 1,
                            egui::Button::new("Delete"),
                        )
                        .clicked()
                    {
                        self.dialog = Dialog::ConfirmDelete;
                    }
                    if ui
                        .add_enabled(self.dirty(), egui::Button::new("Revert"))
                        .on_hover_text("Discard unsaved changes")
                        .clicked()
                    {
                        let p = self.profile.clone();
                        self.load_profile(&p);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.run_controls(ui);
                    });
                });
            });
    }

    fn run_controls(&mut self, ui: &mut egui::Ui) {
        if self.running() {
            let stop = egui::Button::new(
                egui::RichText::new("Stop")
                    .color(theme::p().text)
                    .strong()
                    .size(15.0),
            )
            .fill(theme::p().danger_dim)
            .min_size(egui::vec2(96.0, 32.0));
            if ui.add(stop).clicked() {
                self.stop();
                return;
            }
            let changed = self.cfg.to_toml() != self.running_text;
            if changed
                && ui
                    .add(egui::Button::new("Apply changes").min_size(egui::vec2(0.0, 32.0)))
                    .on_hover_text("Restart mapping with the edited settings")
                    .clicked()
            {
                self.stop();
                self.start();
                return;
            }
            let (on, text) = match &self.runtime {
                Some(rt) if rt.enabled() => (true, "Capturing"),
                _ if !self.cfg.general.game_exe.trim().is_empty() => (false, "Waiting for game"),
                _ => (false, "Paused"),
            };
            status_chip(ui, on, text);
            if !on && ui.button("Resume capture").clicked() {
                if let Some(rt) = &self.runtime {
                    rt.set_enabled(true);
                }
            }
        } else {
            let valid = self.validate().is_ok();
            let start = egui::Button::new(
                egui::RichText::new("Start")
                    .color(theme::p().on_accent)
                    .strong()
                    .size(15.0),
            )
            .fill(theme::p().accent)
            .min_size(egui::vec2(96.0, 32.0));
            if ui.add_enabled(valid, start).clicked() {
                self.start();
            }
            ui.checkbox(&mut self.test_mode, "Test mode").on_hover_text(
                "Run everything except the virtual controller (no driver needed). \
                     Use the live preview on 'Mouse to stick' to check it works.",
            );
            status_chip(ui, false, "Stopped");
        }
    }

    fn tab_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs")
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::TRANSPARENT)
                    .inner_margin(egui::Margin {
                        left: 16.0,
                        right: 16.0,
                        top: 10.0,
                        bottom: 10.0,
                    }),
            )
            .show(ctx, |ui| {
                // A single pill-shaped segmented control.
                egui::Frame::default()
                    .fill(theme::p().bar)
                    .stroke(egui::Stroke::new(1.0_f32, theme::p().line))
                    .rounding(22.0)
                    .inner_margin(egui::Margin::same(4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for t in Tab::ALL {
                                let selected = self.tab == t;
                                let text =
                                    egui::RichText::new(t.label())
                                        .size(14.5)
                                        .color(if selected {
                                            theme::p().on_accent
                                        } else {
                                            theme::p().text
                                        });
                                let btn = egui::Button::new(text)
                                    .fill(if selected {
                                        theme::p().accent
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    })
                                    .stroke(egui::Stroke::NONE)
                                    .rounding(18.0)
                                    .min_size(egui::vec2(0.0, 32.0));
                                if ui.add(btn).clicked() {
                                    self.tab = t;
                                }
                            }
                        });
                    });
                if let Some(rt) = &self.runtime {
                    let hotkey = self.cfg.general.toggle_hotkey.to_string();
                    let exit = self.cfg.general.exit_hotkey.to_string();
                    let game = self.cfg.general.game_exe.trim().to_string();
                    let (fill, mut text) = if rt.enabled() {
                        (
                            theme::p().ok_dim,
                            if game.is_empty() {
                                format!(
                                    "Capturing: bound keys and the mouse go to the controller, so \
                                     you may not be able to click here. Press {hotkey} to pause \
                                     and get them back, {exit} to stop."
                                )
                            } else {
                                format!(
                                    "Capturing while {game} is in front. Switch away from the \
                                     game (Alt+Tab) to use your keyboard and mouse normally; \
                                     {hotkey} pauses, {exit} stops."
                                )
                            },
                        )
                    } else if !game.is_empty() {
                        (
                            theme::p().card,
                            format!(
                                "Waiting: capture turns on automatically when {game} is in front. \
                                 Until then your keyboard and mouse work normally."
                            ),
                        )
                    } else {
                        (
                            theme::p().card,
                            format!(
                                "Paused: keyboard and mouse work normally. Press {hotkey} (or \
                                 'Resume capture') to send them to the controller again."
                            ),
                        )
                    };
                    let hooks_warning = rt.backend() == Backend::Hooks;
                    text.push_str(&format!("   Input: {}.", rt.backend().describe()));
                    ui.add_space(6.0);
                    egui::Frame::default()
                        .fill(fill)
                        .rounding(6.0)
                        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(egui::RichText::new(text).color(theme::p().text));
                            if hooks_warning {
                                ui.label(
                                    egui::RichText::new(
                                        "The game can still see your real mouse and keyboard, so \
                                         it may keep switching to keyboard prompts. Install the \
                                         Interception driver (Settings) to stop that.",
                                    )
                                    .color(theme::p().select),
                                );
                            }
                        });
                }
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        let validation = self.validate().err();
        egui::TopBottomPanel::bottom("bottom")
            .frame(theme::bar())
            .show(ctx, |ui| {
                if let Some(e) = &validation {
                    ui.label(
                        egui::RichText::new(format!("Problem: {e}"))
                            .color(theme::p().danger)
                            .strong(),
                    );
                } else if let Some((msg, is_err)) = &self.status {
                    ui.label(egui::RichText::new(msg).color(if *is_err {
                        theme::p().danger
                    } else {
                        theme::p().text_dim
                    }));
                }
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!("Log ({})", self.log.len()))
                        .color(theme::p().text_dim),
                )
                .id_salt("log")
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .stick_to_bottom(true)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for l in &self.log {
                                ui.label(egui::RichText::new(l).monospace().size(12.0));
                            }
                        });
                });
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        match &self.dialog {
            Dialog::None => {}
            Dialog::NewProfile => {
                let mut close = false;
                let mut create = false;
                egui::Window::new("New profile")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        theme::help(ui, "Starts as a copy of the current settings.");
                        let r = ui.text_edit_singleline(&mut self.new_name);
                        r.request_focus();
                        let name = self.new_name.trim().to_string();
                        let exists = self.store.as_ref().is_some_and(|s| s.exists(&name));
                        let valid = ProfileStore::valid_name(&name) && !exists;
                        if !name.is_empty() && !valid {
                            ui.label(
                                egui::RichText::new(if exists {
                                    "A profile with that name exists"
                                } else {
                                    "Use letters, digits, spaces, - or _"
                                })
                                .color(theme::p().danger),
                            );
                        }
                        ui.horizontal(|ui| {
                            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.add_enabled(valid, egui::Button::new("Create")).clicked()
                                || (enter && valid)
                            {
                                create = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close = true;
                            }
                        });
                    });
                if create {
                    let name = self.new_name.trim().to_string();
                    let result = match &self.store {
                        Some(s) => s.save(&name, &self.cfg),
                        None => Err("no profiles folder".to_string()),
                    };
                    match result {
                        Ok(()) => {
                            if let Some(s) = &self.store {
                                self.profiles = s.list();
                                s.set_last_used(&name);
                            }
                            self.profile = name.clone();
                            self.saved_text = self.cfg.to_toml();
                            self.info(format!("Created profile '{name}'"));
                        }
                        Err(e) => self.error(e),
                    }
                    close = true;
                }
                if close {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::ConfirmDelete => {
                let mut close = false;
                let mut delete = false;
                egui::Window::new("Delete profile")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!("Delete '{}'? This cannot be undone.", self.profile));
                        ui.horizontal(|ui| {
                            if ui
                                .add(egui::Button::new("Delete").fill(theme::p().danger_dim))
                                .clicked()
                            {
                                delete = true;
                            }
                            if ui.button("Cancel").clicked() {
                                close = true;
                            }
                        });
                    });
                if delete {
                    let name = self.profile.clone();
                    let result = match &self.store {
                        Some(s) => s.delete(&name),
                        None => Err("no profiles folder".to_string()),
                    };
                    match result {
                        Ok(()) => {
                            if let Some(s) = &self.store {
                                self.profiles = s.list();
                            }
                            let next = self.profiles.first().cloned().unwrap_or_default();
                            self.load_profile(&next);
                            self.info(format!("Deleted '{name}'"));
                        }
                        Err(e) => self.error(e),
                    }
                    close = true;
                }
                if close {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::ConfirmSwitch(target) => {
                let target = target.clone();
                let mut choice: Option<u8> = None;
                egui::Window::new("Unsaved changes")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "'{}' has unsaved changes. Save them before opening '{target}'?",
                            self.profile
                        ));
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                choice = Some(0);
                            }
                            if ui.button("Discard").clicked() {
                                choice = Some(1);
                            }
                            if ui.button("Cancel").clicked() {
                                choice = Some(2);
                            }
                        });
                    });
                match choice {
                    Some(0) => {
                        self.save_profile();
                        if !self.dirty() {
                            self.load_profile(&target);
                        }
                        self.dialog = Dialog::None;
                    }
                    Some(1) => {
                        self.load_profile(&target);
                        self.dialog = Dialog::None;
                    }
                    Some(2) => self.dialog = Dialog::None,
                    _ => {}
                }
            }
        }
    }
}

fn status_chip(ui: &mut egui::Ui, on: bool, text: &str) {
    let color = if on {
        theme::p().accent
    } else {
        theme::p().text_dim
    };
    egui::Frame::default()
        .fill(theme::p().card)
        .rounding(12.0)
        .inner_margin(egui::Margin::symmetric(10.0, 4.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, color);
                ui.label(egui::RichText::new(text).color(color).strong());
            });
        });
}

impl eframe::App for PadApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Fully transparent: the backdrop painted each frame decides what shows.
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::paint_backdrop(ctx);
        self.poll_runtime();
        self.poll_pick(ctx);
        if self.running() {
            // Live preview and log.
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        self.top_bar(ctx);
        self.tab_bar(ctx);
        self.bottom_bar(ctx);
        match self.tab {
            Tab::Keyboard | Tab::Mouse => self.input_tab(ctx),
            Tab::Controller => self.controller_tab(ctx),
            Tab::Stick => self.stick_tab(ctx),
            Tab::Settings => self.settings_tab(ctx),
        }
        self.dialogs(ctx);
    }
}

impl Drop for PadApp {
    fn drop(&mut self) {
        // Closing the window always releases the keyboard and mouse and unplugs the controller.
        if let Some(mut rt) = self.runtime.take() {
            rt.stop();
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("padbridge")
            .with_inner_size([1380.0, 860.0])
            .with_min_inner_size([1100.0, 700.0])
            // Lets the Clear theme show the desktop behind the window. The other themes paint an
            // opaque backdrop, so they look exactly as before.
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "padbridge",
        options,
        Box::new(|cc| Ok(Box::new(PadApp::new(cc)))),
    )
}
