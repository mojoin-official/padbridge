//! Physical input sources (keys, mouse buttons, wheel notches) and their config names.
//!
//! Keys are identified by **hardware scan code**, not by virtual-key code. That makes bindings
//! follow the *physical position* of a key (WASD stays WASD on AZERTY), and it keeps the numpad
//! and the navigation cluster apart. Names follow a US keyboard.

use crate::util::norm;
use std::fmt;

/// Scan-code identity of a key. Plain keys are `0x01..=0xFF`; extended (`E0`-prefixed) keys are
/// `0xE000 | code`.
pub type KeyId = u16;

/// `Pause` is really the multi-byte sequence `E1 1D 45`; we give it a pseudo id.
pub const KEY_PAUSE: KeyId = 0xE145;
/// Plain scan code of NumLock (Windows reports it with the extended bit set, we normalise it).
pub const KEY_NUMLOCK: KeyId = 0x45;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WheelDir {
    Up,
    Down,
    Left,
    Right,
}

/// Anything the user can physically do that can be bound.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Key(KeyId),
    Mouse(MouseButton),
    /// One wheel notch. Wheel sources have no "up" edge; the mapper turns each notch into a
    /// short press.
    Wheel(WheelDir),
}

static KEY_TABLE: &[(&str, KeyId)] = &[
    ("Esc", 0x01),
    ("1", 0x02),
    ("2", 0x03),
    ("3", 0x04),
    ("4", 0x05),
    ("5", 0x06),
    ("6", 0x07),
    ("7", 0x08),
    ("8", 0x09),
    ("9", 0x0A),
    ("0", 0x0B),
    ("Minus", 0x0C),
    ("Equals", 0x0D),
    ("Backspace", 0x0E),
    ("Tab", 0x0F),
    ("Q", 0x10),
    ("W", 0x11),
    ("E", 0x12),
    ("R", 0x13),
    ("T", 0x14),
    ("Y", 0x15),
    ("U", 0x16),
    ("I", 0x17),
    ("O", 0x18),
    ("P", 0x19),
    ("LeftBracket", 0x1A),
    ("RightBracket", 0x1B),
    ("Enter", 0x1C),
    ("LCtrl", 0x1D),
    ("A", 0x1E),
    ("S", 0x1F),
    ("D", 0x20),
    ("F", 0x21),
    ("G", 0x22),
    ("H", 0x23),
    ("J", 0x24),
    ("K", 0x25),
    ("L", 0x26),
    ("Semicolon", 0x27),
    ("Apostrophe", 0x28),
    ("Grave", 0x29),
    ("LShift", 0x2A),
    ("Backslash", 0x2B),
    ("Z", 0x2C),
    ("X", 0x2D),
    ("C", 0x2E),
    ("V", 0x2F),
    ("B", 0x30),
    ("N", 0x31),
    ("M", 0x32),
    ("Comma", 0x33),
    ("Period", 0x34),
    ("Slash", 0x35),
    ("RShift", 0x36),
    ("NumpadMultiply", 0x37),
    ("LAlt", 0x38),
    ("Space", 0x39),
    ("CapsLock", 0x3A),
    ("F1", 0x3B),
    ("F2", 0x3C),
    ("F3", 0x3D),
    ("F4", 0x3E),
    ("F5", 0x3F),
    ("F6", 0x40),
    ("F7", 0x41),
    ("F8", 0x42),
    ("F9", 0x43),
    ("F10", 0x44),
    ("NumLock", KEY_NUMLOCK),
    ("ScrollLock", 0x46),
    ("Numpad7", 0x47),
    ("Numpad8", 0x48),
    ("Numpad9", 0x49),
    ("NumpadSubtract", 0x4A),
    ("Numpad4", 0x4B),
    ("Numpad5", 0x4C),
    ("Numpad6", 0x4D),
    ("NumpadAdd", 0x4E),
    ("Numpad1", 0x4F),
    ("Numpad2", 0x50),
    ("Numpad3", 0x51),
    ("Numpad0", 0x52),
    ("NumpadDecimal", 0x53),
    ("IntlBackslash", 0x56),
    ("F11", 0x57),
    ("F12", 0x58),
    ("F13", 0x64),
    ("F14", 0x65),
    ("F15", 0x66),
    ("F16", 0x67),
    ("F17", 0x68),
    ("F18", 0x69),
    ("F19", 0x6A),
    ("F20", 0x6B),
    ("F21", 0x6C),
    ("F22", 0x6D),
    ("F23", 0x6E),
    ("F24", 0x76),
    ("NumpadEnter", 0xE01C),
    ("RCtrl", 0xE01D),
    ("NumpadDivide", 0xE035),
    ("PrintScreen", 0xE037),
    ("RAlt", 0xE038),
    ("Home", 0xE047),
    ("Up", 0xE048),
    ("PageUp", 0xE049),
    ("Left", 0xE04B),
    ("Right", 0xE04D),
    ("End", 0xE04F),
    ("Down", 0xE050),
    ("PageDown", 0xE051),
    ("Insert", 0xE052),
    ("Delete", 0xE053),
    ("LWin", 0xE05B),
    ("RWin", 0xE05C),
    ("Menu", 0xE05D),
    ("Pause", KEY_PAUSE),
];

static KEY_ALIASES: &[(&str, &str)] = &[
    ("Escape", "Esc"),
    ("Return", "Enter"),
    ("Spacebar", "Space"),
    ("Back", "Backspace"),
    ("Caps", "CapsLock"),
    ("Backtick", "Grave"),
    ("Tilde", "Grave"),
    ("Dash", "Minus"),
    ("Hyphen", "Minus"),
    ("Quote", "Apostrophe"),
    ("LBracket", "LeftBracket"),
    ("RBracket", "RightBracket"),
    ("LeftCtrl", "LCtrl"),
    ("RightCtrl", "RCtrl"),
    ("LeftShift", "LShift"),
    ("RightShift", "RShift"),
    ("LeftAlt", "LAlt"),
    ("RightAlt", "RAlt"),
    ("LeftWin", "LWin"),
    ("RightWin", "RWin"),
    ("Apps", "Menu"),
    ("Del", "Delete"),
    ("Ins", "Insert"),
    ("PgUp", "PageUp"),
    ("PgDn", "PageDown"),
    ("PrtSc", "PrintScreen"),
    ("Break", "Pause"),
    ("Num0", "Numpad0"),
    ("Num1", "Numpad1"),
    ("Num2", "Numpad2"),
    ("Num3", "Numpad3"),
    ("Num4", "Numpad4"),
    ("Num5", "Numpad5"),
    ("Num6", "Numpad6"),
    ("Num7", "Numpad7"),
    ("Num8", "Numpad8"),
    ("Num9", "Numpad9"),
];

static POINTER_TABLE: &[(&str, Source)] = &[
    ("MouseLeft", Source::Mouse(MouseButton::Left)),
    ("MouseRight", Source::Mouse(MouseButton::Right)),
    ("MouseMiddle", Source::Mouse(MouseButton::Middle)),
    ("Mouse4", Source::Mouse(MouseButton::X1)),
    ("Mouse5", Source::Mouse(MouseButton::X2)),
    ("WheelUp", Source::Wheel(WheelDir::Up)),
    ("WheelDown", Source::Wheel(WheelDir::Down)),
    ("WheelLeft", Source::Wheel(WheelDir::Left)),
    ("WheelRight", Source::Wheel(WheelDir::Right)),
];

static POINTER_ALIASES: &[(&str, &str)] = &[
    ("LMB", "MouseLeft"),
    ("Mouse1", "MouseLeft"),
    ("RMB", "MouseRight"),
    ("Mouse2", "MouseRight"),
    ("MMB", "MouseMiddle"),
    ("Mouse3", "MouseMiddle"),
    ("MouseX1", "Mouse4"),
    ("MouseX2", "Mouse5"),
    ("ScrollUp", "WheelUp"),
    ("ScrollDown", "WheelDown"),
    ("ScrollLeft", "WheelLeft"),
    ("ScrollRight", "WheelRight"),
];

/// Canonical names of every bindable source, for `--list-names`.
pub fn all_names() -> Vec<&'static str> {
    KEY_TABLE
        .iter()
        .map(|(n, _)| *n)
        .chain(POINTER_TABLE.iter().map(|(n, _)| *n))
        .collect()
}

/// Canonical *and* alias names, used for "did you mean" suggestions.
pub fn all_names_with_aliases() -> Vec<&'static str> {
    let mut v = all_names();
    v.extend(KEY_ALIASES.iter().map(|(a, _)| *a));
    v.extend(POINTER_ALIASES.iter().map(|(a, _)| *a));
    v
}

fn lookup_key(n: &str) -> Option<KeyId> {
    KEY_TABLE
        .iter()
        .find(|(name, _)| norm(name) == n)
        .map(|(_, id)| *id)
}

fn lookup_pointer(n: &str) -> Option<Source> {
    POINTER_TABLE
        .iter()
        .find(|(name, _)| norm(name) == n)
        .map(|(_, s)| *s)
}

impl Source {
    /// Parses a config name: a canonical name, an alias, or a raw scan code written `sc:0x1E` /
    /// `sc:E05B`.
    pub fn parse(text: &str) -> Result<Source, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("empty input name".to_string());
        }
        let n = norm(t);
        if let Some(rest) = n.strip_prefix("sc:") {
            let hex = rest.strip_prefix("0x").unwrap_or(rest);
            return match u16::from_str_radix(hex, 16) {
                Ok(id) if id != 0 => Ok(Source::Key(id)),
                _ => Err(format!(
                    "'{t}' is not a valid scan code (expected e.g. sc:0x1E or sc:E05B)"
                )),
            };
        }
        if let Some(id) = lookup_key(&n) {
            return Ok(Source::Key(id));
        }
        if let Some(s) = lookup_pointer(&n) {
            return Ok(s);
        }
        if let Some((_, canon)) = KEY_ALIASES.iter().find(|(a, _)| norm(a) == n) {
            if let Some(id) = lookup_key(&norm(canon)) {
                return Ok(Source::Key(id));
            }
        }
        if let Some((_, canon)) = POINTER_ALIASES.iter().find(|(a, _)| norm(a) == n) {
            if let Some(s) = lookup_pointer(&norm(canon)) {
                return Ok(s);
            }
        }
        Err(format!("unknown input name '{t}'"))
    }

    /// Wheel notches are momentary and have no release edge.
    pub fn is_wheel(self) -> bool {
        matches!(self, Source::Wheel(_))
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Key(id) => match KEY_TABLE.iter().find(|(_, k)| k == id) {
                Some((name, _)) => f.write_str(name),
                None => write!(f, "sc:0x{id:04X}"),
            },
            other => match POINTER_TABLE.iter().find(|(_, s)| s == other) {
                Some((name, _)) => f.write_str(name),
                None => write!(f, "{other:?}"),
            },
        }
    }
}

/// Converts the fields of a Windows `KBDLLHOOKSTRUCT` into a [`KeyId`].
///
/// Returns `None` for events that must be ignored: the "fake shift" events Windows synthesises
/// around extended navigation keys, the fake Left Ctrl that AltGr keyboard layouts emit before
/// every AltGr press (scan code `0x21D`), and events with no scan code.
pub fn hook_key_id(vk: u32, scan: u32, flags: u32) -> Option<KeyId> {
    const VK_PAUSE: u32 = 0x13;
    const VK_NUMLOCK: u32 = 0x90;
    const LLKHF_EXTENDED: u32 = 0x01;
    const ALTGR_FAKE_LCTRL: u32 = 0x21D;
    if scan == ALTGR_FAKE_LCTRL {
        return None;
    }
    // Pause and NumLock share scan code 0x45 and differ only in odd extended-bit conventions;
    // the virtual-key code is the reliable discriminator.
    if vk == VK_PAUSE {
        return Some(KEY_PAUSE);
    }
    if vk == VK_NUMLOCK {
        return Some(KEY_NUMLOCK);
    }
    let ext = flags & LLKHF_EXTENDED != 0;
    let sc = scan & 0xFF;
    if sc == 0 {
        return None;
    }
    // E0 2A / E0 36 are fake shifts injected by the keyboard driver for NumLock-on navigation keys.
    if ext && (sc == 0x2A || sc == 0x36) {
        return None;
    }
    Some(if ext {
        0xE000 | sc as KeyId
    } else {
        sc as KeyId
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tables_have_no_duplicates() {
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for (n, id) in KEY_TABLE {
            assert!(ids.insert(*id), "duplicate scan code for {n}");
            assert!(names.insert(norm(n)), "duplicate name {n}");
        }
        for (n, _) in POINTER_TABLE {
            assert!(names.insert(norm(n)), "duplicate name {n}");
        }
        for (a, _) in KEY_ALIASES.iter().chain(POINTER_ALIASES) {
            assert!(
                names.insert(norm(a)),
                "alias {a} collides with another name"
            );
        }
    }

    #[test]
    fn every_alias_resolves() {
        for (a, canon) in KEY_ALIASES.iter().chain(POINTER_ALIASES) {
            assert_eq!(
                Source::parse(a).unwrap(),
                Source::parse(canon).unwrap(),
                "alias {a}"
            );
        }
    }

    #[test]
    fn canonical_names_round_trip() {
        for name in all_names() {
            let s = Source::parse(name).unwrap();
            assert_eq!(s.to_string(), name);
        }
    }

    #[test]
    fn parsing_is_forgiving_about_case_and_separators() {
        assert_eq!(Source::parse("space").unwrap(), Source::Key(0x39));
        assert_eq!(
            Source::parse("Mouse_Left").unwrap(),
            Source::Mouse(MouseButton::Left)
        );
        assert_eq!(Source::parse("page-up").unwrap(), Source::Key(0xE049));
    }

    #[test]
    fn raw_scan_codes() {
        assert_eq!(Source::parse("sc:0x1E").unwrap(), Source::Key(0x1E));
        assert_eq!(Source::parse("SC:e05b").unwrap(), Source::Key(0xE05B));
        assert!(Source::parse("sc:zz").is_err());
        assert!(Source::parse("sc:0").is_err());
        assert_eq!(Source::Key(0xE0FF).to_string(), "sc:0xE0FF");
    }

    #[test]
    fn unknown_name_is_an_error() {
        assert!(Source::parse("Spce").is_err());
        assert!(Source::parse("").is_err());
    }

    #[test]
    fn numpad_and_navigation_are_distinct() {
        assert_ne!(Source::parse("Enter"), Source::parse("NumpadEnter"));
        assert_ne!(Source::parse("Left"), Source::parse("Numpad4"));
    }

    #[test]
    fn hook_ids() {
        // W
        assert_eq!(hook_key_id(0x57, 0x11, 0), Some(0x11));
        // Right Ctrl (extended)
        assert_eq!(hook_key_id(0xA3, 0x1D, 1), Some(0xE01D));
        // Numpad Enter vs Enter
        assert_eq!(hook_key_id(0x0D, 0x1C, 1), Some(0xE01C));
        assert_eq!(hook_key_id(0x0D, 0x1C, 0), Some(0x1C));
        // Pause / NumLock discriminated by VK
        assert_eq!(hook_key_id(0x13, 0x45, 0), Some(KEY_PAUSE));
        assert_eq!(hook_key_id(0x90, 0x45, 1), Some(KEY_NUMLOCK));
        // fake shifts and empty scan codes are dropped
        assert_eq!(hook_key_id(0xA0, 0x2A, 1), None);
        assert_eq!(hook_key_id(0xA1, 0x36, 1), None);
        assert_eq!(hook_key_id(0x41, 0, 0), None);
        // the fake Left Ctrl AltGr layouts emit is dropped, a real Left Ctrl is kept
        assert_eq!(hook_key_id(0xA2, 0x21D, 0), None);
        assert_eq!(hook_key_id(0xA2, 0x1D, 0), Some(0x1D));
        // real shifts are kept
        assert_eq!(hook_key_id(0xA0, 0x2A, 0), Some(0x2A));
    }
}
