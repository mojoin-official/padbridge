//! Configuration: a strict TOML subset parser plus the typed schema.
//!
//! The subset: `[section]` headers (dots allowed), `key = value` with strings, integers, floats,
//! booleans and (possibly multi-line) arrays of those, `#` comments. Unknown sections or keys are
//! errors (with a "did you mean" hint), so a typo can never silently disable a binding.

use crate::bindings::{Bindings, Combo, UnboundPolicy};
use crate::keys::{self, Source};
use crate::pad::{AxisTarget, Output};
use crate::util::suggest;
use std::fmt;

/// The shipped default configuration (also written by `--write-default`).
pub const DEFAULT_CONFIG: &str = include_str!("../bindings.toml");

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigError {
    /// 1-based line number, or 0 when not tied to a line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

fn err_at<T>(line: usize, message: impl Into<String>) -> Result<T, ConfigError> {
    Err(ConfigError {
        line,
        message: message.into(),
    })
}

// ---------------------------------------------------------------------------------------------
// Document model and parser
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(Vec<Value>),
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Lower-cased key used for matching.
    pub key: String,
    /// Key exactly as typed, for messages.
    pub raw: String,
    pub value: Value,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub name: String,
    pub line: usize,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Default)]
pub struct Document {
    pub sections: Vec<Section>,
}

struct Parser {
    c: Vec<char>,
    i: usize,
    line: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.c.get(self.i).copied()?;
        self.i += 1;
        if ch == '\n' {
            self.line += 1;
        }
        Some(ch)
    }

    fn err<T>(&self, msg: impl Into<String>) -> Result<T, ConfigError> {
        err_at(self.line, msg)
    }

    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\r')) {
            self.i += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while let Some(ch) = self.peek() {
                if ch == '\n' {
                    break;
                }
                self.i += 1;
            }
        }
    }

    fn skip_blank(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                }
                Some('#') => self.skip_comment(),
                _ => break,
            }
        }
    }

    fn end_of_line(&mut self) -> Result<(), ConfigError> {
        self.skip_spaces();
        self.skip_comment();
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.bump();
                Ok(())
            }
            Some(ch) => self.err(format!("unexpected '{ch}' (one entry per line)")),
        }
    }

    fn value(&mut self) -> Result<Value, ConfigError> {
        match self.peek() {
            Some('"') => self.string().map(Value::Str),
            Some('[') => self.array(),
            Some(_) => self.scalar(),
            None => self.err("missing value after '='"),
        }
    }

    fn string(&mut self) -> Result<String, ConfigError> {
        let start = self.line;
        self.bump(); // opening quote
        let mut s = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return err_at(start, "unterminated string"),
                Some('"') => return Ok(s),
                Some('\\') => match self.bump() {
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    _ => return err_at(start, "unsupported escape (use \\\" \\\\ \\n \\t)"),
                },
                Some(ch) => s.push(ch),
            }
        }
    }

    fn array(&mut self) -> Result<Value, ConfigError> {
        self.bump(); // '['
        let mut items = Vec::new();
        loop {
            self.skip_blank();
            match self.peek() {
                Some(']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                None => return self.err("unterminated array (missing ']')"),
                Some('[') => return self.err("nested arrays are not supported"),
                Some(',') => return self.err("unexpected ',' in array"),
                _ => {}
            }
            items.push(self.value()?);
            self.skip_blank();
            match self.peek() {
                Some(',') => {
                    self.bump();
                }
                Some(']') => {}
                _ => return self.err("expected ',' or ']' in array"),
            }
        }
    }

    fn scalar(&mut self) -> Result<Value, ConfigError> {
        let start = self.i;
        while let Some(ch) = self.peek() {
            if ch.is_whitespace() || matches!(ch, ',' | ']' | '#') {
                break;
            }
            self.i += 1;
        }
        let tok: String = self.c[start..self.i].iter().collect();
        match tok.as_str() {
            "" => self.err("missing value"),
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => match parse_number(&tok) {
                Ok(v) => Ok(v),
                Err(m) => self.err(m),
            },
        }
    }
}

fn parse_number(tok: &str) -> Result<Value, String> {
    let clean: String = tok.chars().filter(|c| *c != '_').collect();
    let first = clean
        .chars()
        .next()
        .ok_or_else(|| "missing value".to_string())?;
    if !(first.is_ascii_digit() || matches!(first, '+' | '-' | '.')) {
        return Err(format!(
            "invalid value '{tok}' (text must be written in double quotes)"
        ));
    }
    if let Ok(i) = clean.parse::<i64>() {
        return Ok(Value::Int(i));
    }
    match clean.parse::<f64>() {
        Ok(f) if f.is_finite() => Ok(Value::Float(f)),
        _ => Err(format!("invalid number '{tok}'")),
    }
}

pub fn parse_document(text: &str) -> Result<Document, ConfigError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser {
        c: text.chars().collect(),
        i: 0,
        line: 1,
    };
    let mut doc = Document::default();
    loop {
        p.skip_blank();
        let Some(ch) = p.peek() else { break };
        let line = p.line;
        if ch == '[' {
            p.bump();
            let mut name = String::new();
            loop {
                match p.bump() {
                    Some(']') => break,
                    Some('\n') | None => return err_at(line, "section header is missing ']'"),
                    Some(c) => name.push(c),
                }
            }
            let name = name.trim().to_ascii_lowercase();
            if name.is_empty()
                || !name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
            {
                return err_at(line, format!("invalid section name '[{name}]'"));
            }
            if doc.sections.iter().any(|s| s.name == name) {
                return err_at(line, format!("section [{name}] appears twice"));
            }
            doc.sections.push(Section {
                name,
                line,
                entries: Vec::new(),
            });
            p.end_of_line()?;
        } else {
            if doc.sections.is_empty() {
                return err_at(line, "entries must appear inside a [section]");
            }
            let mut raw = String::new();
            while let Some(c) = p.peek() {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    raw.push(c);
                    p.i += 1;
                } else {
                    break;
                }
            }
            if raw.is_empty() {
                return p.err(format!("unexpected character '{ch}'"));
            }
            p.skip_spaces();
            if p.peek() != Some('=') {
                return p.err(format!("expected '=' after '{raw}'"));
            }
            p.bump();
            p.skip_spaces();
            let value = p.value()?;
            p.end_of_line()?;
            let key = raw.to_ascii_lowercase();
            let sec = doc.sections.last_mut().expect("checked non-empty above");
            if sec.entries.iter().any(|e| e.key == key) {
                return err_at(line, format!("'{raw}' is set twice in [{}]", sec.name));
            }
            sec.entries.push(Entry {
                key,
                raw,
                value,
                line,
            });
        }
    }
    Ok(doc)
}

// ---------------------------------------------------------------------------------------------
// Typed schema
// ---------------------------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Socd {
    /// Opposing directions: the one pressed most recently wins.
    Last,
    /// Opposing directions cancel out.
    Neutral,
}

/// How keyboard and mouse input is read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMethod {
    /// The Interception driver when it is installed (games never see captured input), otherwise
    /// low-level hooks.
    Auto,
    /// Always low-level hooks (games that read the mouse directly still see it).
    Hooks,
}

/// Default hotkeys. Letters on purpose: on many keyboards F10/F12 double as mute/volume keys, so
/// the original Ctrl+Alt+F10 / Ctrl+Alt+F12 muted Windows when used.
pub const DEFAULT_TOGGLE_HOTKEY: &str = "Ctrl+Alt+P";
pub const DEFAULT_EXIT_HOTKEY: &str = "Ctrl+Alt+Q";
/// The hotkeys earlier versions used; profiles still carrying both are switched to the new ones.
pub const OLD_TOGGLE_HOTKEY: &str = "Ctrl+Alt+F10";
pub const OLD_EXIT_HOTKEY: &str = "Ctrl+Alt+F12";

impl Config {
    /// Replaces the old F10/F12 default hotkeys with the new defaults. Returns true if it did.
    pub fn migrate_hotkeys(&mut self) -> bool {
        let g = &mut self.general;
        if g.toggle_hotkey.to_string() == OLD_TOGGLE_HOTKEY
            && g.exit_hotkey.to_string() == OLD_EXIT_HOTKEY
        {
            g.toggle_hotkey = Combo::parse(DEFAULT_TOGGLE_HOTKEY).expect("valid default");
            g.exit_hotkey = Combo::parse(DEFAULT_EXIT_HOTKEY).expect("valid default");
            true
        } else {
            false
        }
    }
}

/// Longest accepted program name for `game_exe`.
pub const GAME_EXE_MAX: usize = 260;

#[derive(Clone, Debug)]
pub struct General {
    pub start_enabled: bool,
    pub toggle_hotkey: Combo,
    pub exit_hotkey: Combo,
    pub output_hz: u32,
    pub ignore_injected: bool,
    pub unbound: UnboundPolicy,
    pub socd: Socd,
    pub normalize_diagonals: bool,
    pub stick_ramp_ms: f64,
    pub walk_scale: f64,
    pub trigger_value: u8,
    pub wheel_pulse_ms: f64,
    pub beep_on_toggle: bool,
    pub input_method: InputMethod,
    /// When set, capture is on only while this program's window is in front (file name such as
    /// `game.exe`, compared case-insensitively). Empty: always.
    pub game_exe: String,
}

#[derive(Clone, Debug)]
pub struct MouseCfg {
    pub smoothing_ms: f64,
    pub deadzone: f64,
    pub curve_exponent: f64,
    pub max_backlog_ms: f64,
    pub swallow_motion: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MouseAxis {
    X,
    Y,
}

#[derive(Clone, Debug)]
pub struct AxisBinding {
    pub source: MouseAxis,
    pub target: AxisTarget,
    pub sensitivity: f64,
    /// Mouse speed (counts per second) that should equal a fully deflected stick.
    pub full_deflection_cps: f64,
    pub invert: bool,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub general: General,
    pub mouse: MouseCfg,
    pub axes: Vec<AxisBinding>,
    pub bindings: Bindings,
}

struct Reader<'a> {
    name: &'static str,
    section: Option<&'a Section>,
    used: Vec<bool>,
}

impl<'a> Reader<'a> {
    fn new(doc: &'a Document, name: &'static str) -> Self {
        let section = doc.sections.iter().find(|s| s.name == name);
        Reader {
            name,
            section,
            used: vec![false; section.map_or(0, |s| s.entries.len())],
        }
    }

    fn take(&mut self, key: &str) -> Option<&'a Entry> {
        let sec = self.section?;
        let i = sec.entries.iter().position(|e| e.key == key)?;
        self.used[i] = true;
        Some(&sec.entries[i])
    }

    fn boolean(&mut self, key: &str, default: bool) -> Result<bool, ConfigError> {
        match self.take(key) {
            None => Ok(default),
            Some(e) => match e.value {
                Value::Bool(b) => Ok(b),
                _ => err_at(e.line, format!("'{}' must be true or false", e.raw)),
            },
        }
    }

    fn number(&mut self, key: &str, default: f64, min: f64, max: f64) -> Result<f64, ConfigError> {
        self.number_opt(key, min, max).map(|v| v.unwrap_or(default))
    }

    fn number_opt(&mut self, key: &str, min: f64, max: f64) -> Result<Option<f64>, ConfigError> {
        let Some(e) = self.take(key) else {
            return Ok(None);
        };
        let v = match e.value {
            Value::Int(i) => i as f64,
            Value::Float(f) => f,
            _ => return err_at(e.line, format!("'{}' must be a number", e.raw)),
        };
        if !(min..=max).contains(&v) {
            return err_at(
                e.line,
                format!("'{}' must be between {min} and {max} (got {v})", e.raw),
            );
        }
        Ok(Some(v))
    }

    fn integer(&mut self, key: &str, default: i64, min: i64, max: i64) -> Result<i64, ConfigError> {
        match self.take(key) {
            None => Ok(default),
            Some(e) => match e.value {
                Value::Int(i) if (min..=max).contains(&i) => Ok(i),
                Value::Int(i) => err_at(
                    e.line,
                    format!("'{}' must be between {min} and {max} (got {i})", e.raw),
                ),
                _ => err_at(e.line, format!("'{}' must be a whole number", e.raw)),
            },
        }
    }

    fn string(&mut self, key: &str) -> Result<Option<(String, usize)>, ConfigError> {
        match self.take(key) {
            None => Ok(None),
            Some(e) => match &e.value {
                Value::Str(s) => Ok(Some((s.clone(), e.line))),
                _ => err_at(e.line, format!("'{}' must be text in double quotes", e.raw)),
            },
        }
    }

    fn choice(
        &mut self,
        key: &str,
        options: &[&str],
        default: usize,
    ) -> Result<usize, ConfigError> {
        match self.string(key)? {
            None => Ok(default),
            Some((s, line)) => {
                let n = crate::util::norm(&s);
                options
                    .iter()
                    .position(|o| *o == n)
                    .ok_or_else(|| ConfigError {
                        line,
                        message: format!(
                            "'{key}' must be one of: {} (got '{s}')",
                            options
                                .iter()
                                .map(|o| format!("\"{o}\""))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    })
            }
        }
    }

    /// Fails on any key that was never read, with a suggestion.
    fn finish(self, valid: &[&str]) -> Result<(), ConfigError> {
        let Some(sec) = self.section else {
            return Ok(());
        };
        for (e, used) in sec.entries.iter().zip(&self.used) {
            if !used {
                let hint = suggest(&e.key, valid.iter().copied())
                    .map(|s| format!(" Did you mean '{s}'?"))
                    .unwrap_or_default();
                return err_at(
                    e.line,
                    format!(
                        "unknown key '{}' in [{}].{hint} Valid keys: {}",
                        e.raw,
                        self.name,
                        valid.join(", ")
                    ),
                );
            }
        }
        Ok(())
    }
}

fn parse_hotkey(r: &mut Reader<'_>, key: &str, default: &str) -> Result<Combo, ConfigError> {
    match r.string(key)? {
        None => Ok(Combo::parse(default).expect("built-in hotkey is valid")),
        Some((text, line)) => Combo::parse(&text).map_err(|m| ConfigError {
            line,
            message: format!("'{key}': {m}"),
        }),
    }
}

fn parse_general(doc: &Document) -> Result<General, ConfigError> {
    const KEYS: &[&str] = &[
        "start_enabled",
        "toggle_hotkey",
        "exit_hotkey",
        "output_hz",
        "ignore_injected",
        "unbound",
        "socd",
        "normalize_diagonals",
        "stick_ramp_ms",
        "walk_scale",
        "trigger_value",
        "wheel_pulse_ms",
        "beep_on_toggle",
        "input_method",
        "game_exe",
    ];
    let mut r = Reader::new(doc, "general");
    let toggle_line = r.section.and_then(|s| {
        s.entries
            .iter()
            .find(|e| e.key == "exit_hotkey")
            .map(|e| e.line)
    });
    let g = General {
        start_enabled: r.boolean("start_enabled", true)?,
        toggle_hotkey: parse_hotkey(&mut r, "toggle_hotkey", DEFAULT_TOGGLE_HOTKEY)?,
        exit_hotkey: parse_hotkey(&mut r, "exit_hotkey", DEFAULT_EXIT_HOTKEY)?,
        output_hz: r.integer("output_hz", 500, 60, 1000)? as u32,
        ignore_injected: r.boolean("ignore_injected", true)?,
        unbound: match r.choice("unbound", &["pass", "block"], 0)? {
            0 => UnboundPolicy::Pass,
            _ => UnboundPolicy::Block,
        },
        socd: match r.choice("socd", &["last", "neutral"], 0)? {
            0 => Socd::Last,
            _ => Socd::Neutral,
        },
        normalize_diagonals: r.boolean("normalize_diagonals", true)?,
        stick_ramp_ms: r.number("stick_ramp_ms", 0.0, 0.0, 500.0)?,
        walk_scale: r.number("walk_scale", 0.5, 0.05, 1.0)?,
        trigger_value: r.integer("trigger_value", 255, 1, 255)? as u8,
        wheel_pulse_ms: r.number("wheel_pulse_ms", 40.0, 5.0, 500.0)?,
        beep_on_toggle: r.boolean("beep_on_toggle", false)?,
        input_method: match r.choice("input_method", &["auto", "hooks"], 0)? {
            0 => InputMethod::Auto,
            _ => InputMethod::Hooks,
        },
        game_exe: match r.string("game_exe")? {
            None => String::new(),
            Some((text, line)) => {
                let t = text.trim().to_string();
                if t.chars().count() > GAME_EXE_MAX || t.contains(['"', '\n']) {
                    return err_at(
                        line,
                        "'game_exe' must be a program file name such as game.exe",
                    );
                }
                t
            }
        },
    };
    r.finish(KEYS)?;
    if g.toggle_hotkey.overlaps(&g.exit_hotkey) {
        return err_at(
            toggle_line.unwrap_or(0),
            "toggle_hotkey and exit_hotkey must be different",
        );
    }
    Ok(g)
}

fn parse_mouse(doc: &Document) -> Result<MouseCfg, ConfigError> {
    const KEYS: &[&str] = &[
        "smoothing_ms",
        "deadzone",
        "curve_exponent",
        "max_backlog_ms",
        "swallow_motion",
    ];
    let mut r = Reader::new(doc, "mouse");
    let m = MouseCfg {
        smoothing_ms: r.number("smoothing_ms", 8.0, 0.0, 100.0)?,
        deadzone: r.number("deadzone", 0.0, 0.0, 0.95)?,
        curve_exponent: r.number("curve_exponent", 1.0, 0.1, 10.0)?,
        max_backlog_ms: r.number("max_backlog_ms", 0.0, 0.0, 5000.0)?,
        swallow_motion: r.boolean("swallow_motion", true)?,
    };
    r.finish(KEYS)?;
    Ok(m)
}

fn parse_axis(
    doc: &Document,
    name: &'static str,
    source: MouseAxis,
) -> Result<Option<AxisBinding>, ConfigError> {
    const KEYS: &[&str] = &[
        "target",
        "sensitivity",
        "invert",
        "full_deflection_cps",
        "counts_per_360",
        "stick_seconds_per_360",
    ];
    let Some(section) = doc.sections.iter().find(|s| s.name == name) else {
        return Ok(None);
    };
    let mut r = Reader::new(doc, name);
    let target = match r.string("target")? {
        None => {
            return err_at(
                section.line,
                format!(
                    "[{name}] needs a 'target' (one of: {})",
                    AxisTarget::ALL.map(AxisTarget::name).join(", ")
                ),
            )
        }
        Some((text, line)) => AxisTarget::parse(&text).ok_or_else(|| ConfigError {
            line,
            message: format!(
                "unknown axis '{text}' (valid: {})",
                AxisTarget::ALL.map(AxisTarget::name).join(", ")
            ),
        })?,
    };
    let sensitivity = r.number("sensitivity", 1.0, 0.01, 100.0)?;
    let invert = r.boolean("invert", false)?;

    let line_of = |key: &str| {
        section
            .entries
            .iter()
            .find(|e| e.key == key)
            .map_or(section.line, |e| e.line)
    };
    let direct = r.number_opt("full_deflection_cps", 50.0, 1_000_000.0)?;
    let counts = r.number_opt("counts_per_360", 1.0, 10_000_000.0)?;
    let seconds = r.number_opt("stick_seconds_per_360", 0.05, 600.0)?;
    let full_deflection_cps = match (direct, counts, seconds) {
        (None, None, None) => 8000.0,
        (Some(v), None, None) => v,
        (None, Some(c), Some(s)) => c / s,
        (Some(_), _, _) if counts.is_some() || seconds.is_some() => {
            return err_at(
                line_of("full_deflection_cps"),
                "give either full_deflection_cps, or counts_per_360 together with \
                 stick_seconds_per_360, not both",
            )
        }
        _ => {
            return err_at(
                line_of("counts_per_360"),
                "counts_per_360 and stick_seconds_per_360 must be given together",
            )
        }
    };
    if !(50.0..=1_000_000.0).contains(&full_deflection_cps) {
        return err_at(
            line_of("counts_per_360"),
            format!(
                "calibration works out to {full_deflection_cps:.1} counts/s of full deflection, \
                 outside the supported 50..1000000"
            ),
        );
    }
    r.finish(KEYS)?;
    Ok(Some(AxisBinding {
        source,
        target,
        sensitivity,
        full_deflection_cps,
        invert,
    }))
}

fn parse_bindings(doc: &Document) -> Result<Bindings, ConfigError> {
    let mut b = Bindings::new();
    let Some(sec) = doc.sections.iter().find(|s| s.name == "bindings") else {
        return Ok(b);
    };
    let outputs = Output::all_names_with_aliases();
    let sources = keys::all_names_with_aliases();
    for e in &sec.entries {
        let Some(out) = Output::parse(&e.raw) else {
            let hint = suggest(&e.raw, outputs.iter().copied())
                .map(|s| format!(" Did you mean '{s}'?"))
                .unwrap_or_default();
            return err_at(
                e.line,
                format!(
                    "unknown controller output '{}'.{hint} (see --list-names)",
                    e.raw
                ),
            );
        };
        let names: Vec<&Value> = match &e.value {
            Value::Array(items) => items.iter().collect(),
            single => vec![single],
        };
        for v in names {
            let Value::Str(s) = v else {
                return err_at(
                    e.line,
                    format!("'{}' must list input names in double quotes", e.raw),
                );
            };
            match Source::parse(s) {
                Ok(src) => b.add(src, out),
                Err(m) => {
                    let hint = suggest(s, sources.iter().copied())
                        .map(|x| format!(" Did you mean '{x}'?"))
                        .unwrap_or_default();
                    return err_at(e.line, format!("{m}.{hint} (see --list-names)"));
                }
            }
        }
    }
    Ok(b)
}

impl Config {
    /// Parses and fully validates a configuration.
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        const SECTIONS: &[&str] = &[
            "general",
            "mouse",
            "axis.mouse_x",
            "axis.mouse_y",
            "bindings",
        ];
        let doc = parse_document(text)?;
        for s in &doc.sections {
            if !SECTIONS.contains(&s.name.as_str()) {
                let hint = suggest(&s.name, SECTIONS.iter().copied())
                    .map(|x| format!(" Did you mean [{x}]?"))
                    .unwrap_or_default();
                return err_at(
                    s.line,
                    format!(
                        "unknown section [{}].{hint} Valid sections: {}",
                        s.name,
                        SECTIONS.join(", ")
                    ),
                );
            }
        }
        let general = parse_general(&doc)?;
        let mouse = parse_mouse(&doc)?;
        let mut axes = Vec::new();
        if let Some(a) = parse_axis(&doc, "axis.mouse_x", MouseAxis::X)? {
            axes.push(a);
        }
        if let Some(a) = parse_axis(&doc, "axis.mouse_y", MouseAxis::Y)? {
            if axes.iter().any(|x| x.target == a.target) {
                let line = doc
                    .sections
                    .iter()
                    .find(|s| s.name == "axis.mouse_y")
                    .map_or(0, |s| s.line);
                return err_at(
                    line,
                    format!(
                        "both mouse axes are bound to {}; give each its own target",
                        a.target.name()
                    ),
                );
            }
            axes.push(a);
        }
        let bindings = parse_bindings(&doc)?;
        Ok(Config {
            general,
            mouse,
            axes,
            bindings,
        })
    }

    /// The built-in default configuration. Panics only if the shipped file is invalid, which a
    /// unit test guards against.
    pub fn builtin() -> Config {
        Config::parse(DEFAULT_CONFIG).expect("built-in bindings.toml is valid")
    }

    /// Writes the configuration back out in the same format `parse` reads. Every value is
    /// written explicitly, so the file is self-documenting and survives future default changes.
    pub fn to_toml(&self) -> String {
        fn q(s: &str) -> String {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        let g = &self.general;
        let m = &self.mouse;
        let mut s = String::new();
        s.push_str("# padbridge profile (written by padbridge; comments are not kept)\n\n");
        s.push_str("[general]\n");
        s.push_str(&format!("start_enabled = {}\n", g.start_enabled));
        s.push_str(&format!(
            "toggle_hotkey = {}\n",
            q(&g.toggle_hotkey.to_string())
        ));
        s.push_str(&format!(
            "exit_hotkey = {}\n",
            q(&g.exit_hotkey.to_string())
        ));
        s.push_str(&format!("output_hz = {}\n", g.output_hz));
        s.push_str(&format!("ignore_injected = {}\n", g.ignore_injected));
        s.push_str(&format!(
            "unbound = {}\n",
            q(match g.unbound {
                UnboundPolicy::Pass => "pass",
                UnboundPolicy::Block => "block",
            })
        ));
        s.push_str(&format!(
            "socd = {}\n",
            q(match g.socd {
                Socd::Last => "last",
                Socd::Neutral => "neutral",
            })
        ));
        s.push_str(&format!(
            "normalize_diagonals = {}\n",
            g.normalize_diagonals
        ));
        s.push_str(&format!("stick_ramp_ms = {}\n", g.stick_ramp_ms));
        s.push_str(&format!("walk_scale = {}\n", g.walk_scale));
        s.push_str(&format!("trigger_value = {}\n", g.trigger_value));
        s.push_str(&format!("wheel_pulse_ms = {}\n", g.wheel_pulse_ms));
        s.push_str(&format!("beep_on_toggle = {}\n", g.beep_on_toggle));
        s.push_str(&format!(
            "input_method = {}\n",
            q(match g.input_method {
                InputMethod::Auto => "auto",
                InputMethod::Hooks => "hooks",
            })
        ));
        s.push_str(&format!("game_exe = {}\n", q(&g.game_exe)));

        s.push_str("\n[mouse]\n");
        s.push_str(&format!("smoothing_ms = {}\n", m.smoothing_ms));
        s.push_str(&format!("deadzone = {}\n", m.deadzone));
        s.push_str(&format!("curve_exponent = {}\n", m.curve_exponent));
        s.push_str(&format!("max_backlog_ms = {}\n", m.max_backlog_ms));
        s.push_str(&format!("swallow_motion = {}\n", m.swallow_motion));

        for (axis, section) in [
            (MouseAxis::X, "axis.mouse_x"),
            (MouseAxis::Y, "axis.mouse_y"),
        ] {
            if let Some(a) = self.axes.iter().find(|a| a.source == axis) {
                s.push_str(&format!("\n[{section}]\n"));
                s.push_str(&format!("target = {}\n", q(a.target.name())));
                s.push_str(&format!("sensitivity = {}\n", a.sensitivity));
                s.push_str(&format!("invert = {}\n", a.invert));
                s.push_str(&format!(
                    "full_deflection_cps = {}\n",
                    a.full_deflection_cps
                ));
            }
        }

        s.push_str("\n[bindings]\n");
        for out in Output::ALL {
            let srcs = self.bindings.sources_for(out);
            if srcs.is_empty() {
                continue;
            }
            let names: Vec<String> = srcs.iter().map(|x| q(&x.to_string())).collect();
            s.push_str(&format!("{} = [{}]\n", out.name(), names.join(", ")));
        }
        s
    }

    /// Human-readable summary for `--check`.
    pub fn summary(&self) -> String {
        let mut s = String::new();
        let g = &self.general;
        s.push_str(&format!(
            "toggle: {}   exit: {}   output rate: {} Hz   unbound input: {:?}\n",
            g.toggle_hotkey, g.exit_hotkey, g.output_hz, g.unbound
        ));
        if self.axes.is_empty() {
            s.push_str("mouse motion: not bound\n");
        }
        for a in &self.axes {
            s.push_str(&format!(
                "mouse {:?} -> {}   full deflection at {:.0} counts/s, sensitivity {}{}\n",
                a.source,
                a.target.name(),
                a.full_deflection_cps,
                a.sensitivity,
                if a.invert { ", inverted" } else { "" }
            ));
        }
        s.push_str(&format!("{} bound inputs:\n", self.bindings.source_count()));
        for (src, out) in self.bindings.pairs_sorted() {
            s.push_str(&format!("  {src:<16} -> {}\n", out.name()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> ConfigError {
        Config::parse(text).expect_err("expected a config error")
    }

    #[test]
    fn shipped_default_parses_and_is_sane() {
        let c = Config::builtin();
        assert_eq!(c.axes.len(), 2);
        assert_eq!(c.axes[0].target, AxisTarget::RightX);
        assert_eq!(c.axes[1].target, AxisTarget::RightY);
        assert!(c.bindings.source_count() > 20);
        assert!(c.bindings.contains(Source::parse("W").unwrap()));
        assert_eq!(c.general.output_hz, 500);
        // every documented default in the file equals the code default
        let empty = Config::parse("").unwrap();
        assert_eq!(empty.general.output_hz, c.general.output_hz);
        assert_eq!(empty.mouse.smoothing_ms, c.mouse.smoothing_ms);
        assert_eq!(empty.general.socd, c.general.socd);
    }

    #[test]
    fn document_parser_handles_comments_arrays_and_escapes() {
        let doc = parse_document(
            "# top\n[a]\nx = 1 # trailing\ny = [\n  \"p\", # c\n  \"q\",\n]\nz=\"a\\\"b\"\nw = -2.5e1\nv = true\n",
        )
        .unwrap();
        let s = &doc.sections[0];
        assert_eq!(s.entries[0].value, Value::Int(1));
        assert_eq!(
            s.entries[1].value,
            Value::Array(vec![Value::Str("p".into()), Value::Str("q".into())])
        );
        assert_eq!(s.entries[2].value, Value::Str("a\"b".into()));
        assert_eq!(s.entries[3].value, Value::Float(-25.0));
        assert_eq!(s.entries[4].value, Value::Bool(true));
    }

    #[test]
    fn hash_inside_a_string_is_not_a_comment() {
        let doc = parse_document("[a]\nk = \"a#b\"\n").unwrap();
        assert_eq!(doc.sections[0].entries[0].value, Value::Str("a#b".into()));
    }

    #[test]
    fn bom_and_crlf_are_accepted() {
        let doc = parse_document("\u{feff}[a]\r\nk = 1\r\n").unwrap();
        assert_eq!(doc.sections[0].entries[0].value, Value::Int(1));
    }

    #[test]
    fn syntax_errors_report_the_line() {
        assert_eq!(err("[general\nx=1").line, 1);
        assert_eq!(err("[general]\n\nstart_enabled = maybe\n").line, 3);
        assert_eq!(err("[general]\nstart_enabled = \"x\n").line, 2);
        assert!(err("[bindings]\nA = [\"W\"\n").line > 0);
        assert!(err("x = 1\n").message.contains("inside a [section]"));
        assert!(err("[a]\n[a]\n").message.contains("twice"));
        assert!(err("[general]\nsocd = \"last\"\nsocd = \"neutral\"\n")
            .message
            .contains("twice"));
        assert!(err("[general]\nx = nan\n").message.contains("invalid"));
        assert!(err("[general]\nx = 1 2\n")
            .message
            .contains("one entry per line"));
    }

    #[test]
    fn unknown_names_get_suggestions() {
        let e = err("[genral]\n");
        assert!(e.message.contains("[general]"), "{e}");
        let e = err("[general]\noutput_hzz = 500\n");
        assert!(e.message.contains("output_hz"), "{e}");
        let e = err("[bindings]\nLeftStickUpp = \"W\"\n");
        assert!(e.message.contains("LeftStickUp"), "{e}");
        let e = err("[bindings]\nA = \"Spce\"\n");
        assert!(e.message.contains("Space"), "{e}");
    }

    #[test]
    fn range_and_type_checks() {
        assert!(err("[general]\noutput_hz = 10\n")
            .message
            .contains("between 60 and 1000"));
        assert!(err("[general]\noutput_hz = 500.5\n")
            .message
            .contains("whole number"));
        assert!(err("[general]\nunbound = \"maybe\"\n")
            .message
            .contains("\"pass\""));
        assert!(err("[mouse]\ndeadzone = 2\n").message.contains("between"));
        assert!(err("[general]\nstart_enabled = 1\n")
            .message
            .contains("true or false"));
        assert!(err("[bindings]\nA = 5\n").message.contains("double quotes"));
    }

    #[test]
    fn hotkeys_validated() {
        assert!(err("[general]\ntoggle_hotkey = \"Ctrl+Bogus\"\n")
            .message
            .contains("toggle_hotkey"));
        assert!(
            err("[general]\ntoggle_hotkey = \"F9\"\nexit_hotkey = \"F9\"\n")
                .message
                .contains("different")
        );
    }

    #[test]
    fn axis_section_rules() {
        assert!(err("[axis.mouse_x]\n").message.contains("needs a 'target'"));
        assert!(err("[axis.mouse_x]\ntarget = \"Banana\"\n")
            .message
            .contains("unknown axis"));
        assert!(err(
            "[axis.mouse_x]\ntarget = \"RightStickX\"\n[axis.mouse_y]\ntarget = \"RightStickX\"\n"
        )
        .message
        .contains("both mouse axes"));
        // calibration forms
        let c = Config::parse(
            "[axis.mouse_x]\ntarget = \"RightStickX\"\ncounts_per_360 = 16000\nstick_seconds_per_360 = 2\n",
        )
        .unwrap();
        assert!((c.axes[0].full_deflection_cps - 8000.0).abs() < 1e-9);
        assert!(
            err("[axis.mouse_x]\ntarget = \"RightStickX\"\ncounts_per_360 = 16000\n")
                .message
                .contains("together")
        );
        assert!(err(
            "[axis.mouse_x]\ntarget = \"RightStickX\"\nfull_deflection_cps = 5000\ncounts_per_360 = 16000\nstick_seconds_per_360 = 2\n"
        )
        .message
        .contains("not both"));
        // either mouse axis may target any stick axis, including swapped
        let c = Config::parse(
            "[axis.mouse_x]\ntarget = \"LeftStickY\"\n[axis.mouse_y]\ntarget = \"RightStickX\"\n",
        )
        .unwrap();
        assert_eq!(c.axes[0].target, AxisTarget::LeftY);
        assert_eq!(c.axes[1].target, AxisTarget::RightX);
    }

    #[test]
    fn mouse_motion_can_be_left_unbound() {
        let c = Config::parse("[bindings]\nA = \"Space\"\n").unwrap();
        assert!(c.axes.is_empty());
    }

    #[test]
    fn one_source_may_drive_several_outputs_and_vice_versa() {
        let c = Config::parse("[bindings]\nA = [\"Space\", \"Mouse4\"]\nLB = \"Space\"\n").unwrap();
        let space = Source::parse("Space").unwrap();
        assert_eq!(c.bindings.outputs(space), &[Output::A, Output::Lb]);
        assert!(c.bindings.contains(Source::parse("Mouse4").unwrap()));
    }

    #[test]
    fn summary_lists_bindings() {
        let s = Config::builtin().summary();
        assert!(s.contains("RightStickX"));
        assert!(s.contains("LeftStickUp"));
    }

    #[test]
    fn to_toml_round_trips_exactly() {
        let a = Config::builtin();
        let text = a.to_toml();
        let b = Config::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(b.to_toml(), text, "writing is idempotent");
        assert_eq!(b.bindings, a.bindings);
        assert_eq!(b.axes.len(), a.axes.len());
        assert_eq!(b.general.toggle_hotkey, a.general.toggle_hotkey);
        assert_eq!(b.mouse.smoothing_ms, a.mouse.smoothing_ms);
    }

    #[test]
    fn to_toml_keeps_unusual_values() {
        let mut c = Config::parse(
            "[general]\nunbound = \"block\"\nsocd = \"neutral\"\nwalk_scale = 0.35\n\
             [axis.mouse_y]\ntarget = \"LeftStickX\"\ninvert = true\nsensitivity = 2.5\n\
             full_deflection_cps = 1234.5\n\
             [bindings]\nA = [\"sc:0xE0FF\", \"WheelLeft\"]\n",
        )
        .unwrap();
        c.mouse.deadzone = 0.123456789;
        c.general.input_method = InputMethod::Hooks;
        c.general.game_exe = "My Game.exe".to_string();
        let b = Config::parse(&c.to_toml()).unwrap();
        assert_eq!(b.general.unbound, UnboundPolicy::Block);
        assert_eq!(b.general.socd, Socd::Neutral);
        assert_eq!(b.general.walk_scale, 0.35);
        assert_eq!(b.mouse.deadzone, 0.123456789, "floats survive exactly");
        assert_eq!(b.general.input_method, InputMethod::Hooks);
        assert_eq!(b.general.game_exe, "My Game.exe");
        assert_eq!(b.axes.len(), 1);
        assert_eq!(b.axes[0].target, AxisTarget::LeftX);
        assert!(b.axes[0].invert);
        assert_eq!(b.axes[0].full_deflection_cps, 1234.5);
        assert_eq!(b.bindings, c.bindings);
    }

    #[test]
    fn old_default_hotkeys_are_migrated_and_custom_ones_kept() {
        let mut c = Config::parse(
            "[general]\ntoggle_hotkey = \"Ctrl+Alt+F10\"\nexit_hotkey = \"Ctrl+Alt+F12\"\n",
        )
        .unwrap();
        assert!(c.migrate_hotkeys());
        assert_eq!(c.general.toggle_hotkey.to_string(), DEFAULT_TOGGLE_HOTKEY);
        assert_eq!(c.general.exit_hotkey.to_string(), DEFAULT_EXIT_HOTKEY);
        assert!(!c.migrate_hotkeys(), "only once");
        let mut custom =
            Config::parse("[general]\ntoggle_hotkey = \"Ctrl+Alt+F10\"\nexit_hotkey = \"Pause\"\n")
                .unwrap();
        assert!(!custom.migrate_hotkeys(), "a customised pair is left alone");
        let fresh = Config::parse("").unwrap();
        assert_eq!(
            fresh.general.toggle_hotkey.to_string(),
            DEFAULT_TOGGLE_HOTKEY
        );
        // the defaults are not media keys and both are valid, distinct combos
        assert!(!fresh
            .general
            .toggle_hotkey
            .overlaps(&fresh.general.exit_hotkey));
    }

    #[test]
    fn to_toml_with_nothing_bound() {
        let c = Config::parse("").unwrap();
        let b = Config::parse(&c.to_toml()).unwrap();
        assert!(b.axes.is_empty());
        assert_eq!(b.bindings.source_count(), 0);
    }
}
