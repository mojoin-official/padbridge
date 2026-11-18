//! Geometry for the pictures in the app: a full-size keyboard, a mouse and a controller.
//! Plain data, so it is unit-tested here (every name must parse, nothing may overlap).

use crate::keys::Source;
use crate::pad::Output;

/// One clickable region. Coordinates are in layout units (1.0 = one standard key).
#[derive(Copy, Clone, Debug)]
pub struct Slot {
    /// Config name of the input or output.
    pub name: &'static str,
    /// Short text drawn on the region.
    pub label: &'static str,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

const fn k(name: &'static str, label: &'static str, x: f32, y: f32, w: f32) -> Slot {
    Slot {
        name,
        label,
        x,
        y,
        w,
        h: 1.0,
    }
}

const fn kt(name: &'static str, label: &'static str, x: f32, y: f32, w: f32, h: f32) -> Slot {
    Slot {
        name,
        label,
        x,
        y,
        w,
        h,
    }
}

/// Full-size US keyboard (104 keys). Width 22.5, height 6.5.
pub const KEYBOARD_SIZE: (f32, f32) = (22.5, 6.5);

pub static KEYBOARD: &[Slot] = &[
    // function row
    k("Esc", "Esc", 0.0, 0.0, 1.0),
    k("F1", "F1", 2.0, 0.0, 1.0),
    k("F2", "F2", 3.0, 0.0, 1.0),
    k("F3", "F3", 4.0, 0.0, 1.0),
    k("F4", "F4", 5.0, 0.0, 1.0),
    k("F5", "F5", 6.5, 0.0, 1.0),
    k("F6", "F6", 7.5, 0.0, 1.0),
    k("F7", "F7", 8.5, 0.0, 1.0),
    k("F8", "F8", 9.5, 0.0, 1.0),
    k("F9", "F9", 11.0, 0.0, 1.0),
    k("F10", "F10", 12.0, 0.0, 1.0),
    k("F11", "F11", 13.0, 0.0, 1.0),
    k("F12", "F12", 14.0, 0.0, 1.0),
    k("PrintScreen", "PrtSc", 15.25, 0.0, 1.0),
    k("ScrollLock", "ScrLk", 16.25, 0.0, 1.0),
    k("Pause", "Pause", 17.25, 0.0, 1.0),
    // number row
    k("Grave", "`", 0.0, 1.5, 1.0),
    k("1", "1", 1.0, 1.5, 1.0),
    k("2", "2", 2.0, 1.5, 1.0),
    k("3", "3", 3.0, 1.5, 1.0),
    k("4", "4", 4.0, 1.5, 1.0),
    k("5", "5", 5.0, 1.5, 1.0),
    k("6", "6", 6.0, 1.5, 1.0),
    k("7", "7", 7.0, 1.5, 1.0),
    k("8", "8", 8.0, 1.5, 1.0),
    k("9", "9", 9.0, 1.5, 1.0),
    k("0", "0", 10.0, 1.5, 1.0),
    k("Minus", "-", 11.0, 1.5, 1.0),
    k("Equals", "=", 12.0, 1.5, 1.0),
    k("Backspace", "Backspace", 13.0, 1.5, 2.0),
    k("Insert", "Ins", 15.25, 1.5, 1.0),
    k("Home", "Home", 16.25, 1.5, 1.0),
    k("PageUp", "PgUp", 17.25, 1.5, 1.0),
    k("NumLock", "Num", 18.5, 1.5, 1.0),
    k("NumpadDivide", "/", 19.5, 1.5, 1.0),
    k("NumpadMultiply", "*", 20.5, 1.5, 1.0),
    k("NumpadSubtract", "-", 21.5, 1.5, 1.0),
    // top letter row
    k("Tab", "Tab", 0.0, 2.5, 1.5),
    k("Q", "Q", 1.5, 2.5, 1.0),
    k("W", "W", 2.5, 2.5, 1.0),
    k("E", "E", 3.5, 2.5, 1.0),
    k("R", "R", 4.5, 2.5, 1.0),
    k("T", "T", 5.5, 2.5, 1.0),
    k("Y", "Y", 6.5, 2.5, 1.0),
    k("U", "U", 7.5, 2.5, 1.0),
    k("I", "I", 8.5, 2.5, 1.0),
    k("O", "O", 9.5, 2.5, 1.0),
    k("P", "P", 10.5, 2.5, 1.0),
    k("LeftBracket", "[", 11.5, 2.5, 1.0),
    k("RightBracket", "]", 12.5, 2.5, 1.0),
    k("Backslash", "\\", 13.5, 2.5, 1.5),
    k("Delete", "Del", 15.25, 2.5, 1.0),
    k("End", "End", 16.25, 2.5, 1.0),
    k("PageDown", "PgDn", 17.25, 2.5, 1.0),
    k("Numpad7", "7", 18.5, 2.5, 1.0),
    k("Numpad8", "8", 19.5, 2.5, 1.0),
    k("Numpad9", "9", 20.5, 2.5, 1.0),
    kt("NumpadAdd", "+", 21.5, 2.5, 1.0, 2.0),
    // home row
    k("CapsLock", "Caps", 0.0, 3.5, 1.75),
    k("A", "A", 1.75, 3.5, 1.0),
    k("S", "S", 2.75, 3.5, 1.0),
    k("D", "D", 3.75, 3.5, 1.0),
    k("F", "F", 4.75, 3.5, 1.0),
    k("G", "G", 5.75, 3.5, 1.0),
    k("H", "H", 6.75, 3.5, 1.0),
    k("J", "J", 7.75, 3.5, 1.0),
    k("K", "K", 8.75, 3.5, 1.0),
    k("L", "L", 9.75, 3.5, 1.0),
    k("Semicolon", ";", 10.75, 3.5, 1.0),
    k("Apostrophe", "'", 11.75, 3.5, 1.0),
    k("Enter", "Enter", 12.75, 3.5, 2.25),
    k("Numpad4", "4", 18.5, 3.5, 1.0),
    k("Numpad5", "5", 19.5, 3.5, 1.0),
    k("Numpad6", "6", 20.5, 3.5, 1.0),
    // bottom letter row
    k("LShift", "Shift", 0.0, 4.5, 2.25),
    k("Z", "Z", 2.25, 4.5, 1.0),
    k("X", "X", 3.25, 4.5, 1.0),
    k("C", "C", 4.25, 4.5, 1.0),
    k("V", "V", 5.25, 4.5, 1.0),
    k("B", "B", 6.25, 4.5, 1.0),
    k("N", "N", 7.25, 4.5, 1.0),
    k("M", "M", 8.25, 4.5, 1.0),
    k("Comma", ",", 9.25, 4.5, 1.0),
    k("Period", ".", 10.25, 4.5, 1.0),
    k("Slash", "/", 11.25, 4.5, 1.0),
    k("RShift", "Shift", 12.25, 4.5, 2.75),
    k("Up", "\u{2191}", 16.25, 4.5, 1.0),
    k("Numpad1", "1", 18.5, 4.5, 1.0),
    k("Numpad2", "2", 19.5, 4.5, 1.0),
    k("Numpad3", "3", 20.5, 4.5, 1.0),
    kt("NumpadEnter", "Ent", 21.5, 4.5, 1.0, 2.0),
    // space row
    k("LCtrl", "Ctrl", 0.0, 5.5, 1.25),
    k("LWin", "Win", 1.25, 5.5, 1.25),
    k("LAlt", "Alt", 2.5, 5.5, 1.25),
    k("Space", "Space", 3.75, 5.5, 6.25),
    k("RAlt", "Alt", 10.0, 5.5, 1.25),
    k("RWin", "Win", 11.25, 5.5, 1.25),
    k("Menu", "Menu", 12.5, 5.5, 1.25),
    k("RCtrl", "Ctrl", 13.75, 5.5, 1.25),
    k("Left", "\u{2190}", 15.25, 5.5, 1.0),
    k("Down", "\u{2193}", 16.25, 5.5, 1.0),
    k("Right", "\u{2192}", 17.25, 5.5, 1.0),
    k("Numpad0", "0", 18.5, 5.5, 2.0),
    k("NumpadDecimal", ".", 20.5, 5.5, 1.0),
];

/// Mouse picture, 10 x 15 units. Body outline is drawn separately.
pub const MOUSE_SIZE: (f32, f32) = (10.0, 15.0);

pub static MOUSE: &[Slot] = &[
    kt("MouseLeft", "Left", 1.0, 0.6, 3.6, 5.6),
    kt("MouseRight", "Right", 5.4, 0.6, 3.6, 5.6),
    kt("WheelUp", "\u{2191}", 4.6, 0.7, 0.8, 1.2),
    kt("MouseMiddle", "Mid", 4.6, 2.1, 0.8, 1.6),
    kt("WheelDown", "\u{2193}", 4.6, 3.9, 0.8, 1.2),
    kt("WheelLeft", "\u{2190}", 3.8, 6.4, 1.1, 0.9),
    kt("WheelRight", "\u{2192}", 5.1, 6.4, 1.1, 0.9),
    kt("Mouse5", "M5", 0.0, 7.0, 1.0, 1.6),
    kt("Mouse4", "M4", 0.0, 8.8, 1.0, 1.6),
];

/// Controller picture, 20 x 12 units. Body outline is drawn separately.
pub const PAD_SIZE: (f32, f32) = (20.0, 12.0);

pub static PAD: &[Slot] = &[
    kt("LT", "LT", 2.5, 0.0, 2.6, 1.0),
    kt("LB", "LB", 2.5, 1.2, 2.6, 0.9),
    kt("RT", "RT", 14.9, 0.0, 2.6, 1.0),
    kt("RB", "RB", 14.9, 1.2, 2.6, 0.9),
    // left stick: four directions around a click
    kt("LeftStickUp", "\u{2191}", 3.4, 2.9, 1.2, 1.0),
    kt("LeftStickLeft", "\u{2190}", 2.2, 4.0, 1.2, 1.0),
    kt("LeftStickClick", "L3", 3.4, 4.0, 1.2, 1.0),
    kt("LeftStickRight", "\u{2192}", 4.6, 4.0, 1.2, 1.0),
    kt("LeftStickDown", "\u{2193}", 3.4, 5.1, 1.2, 1.0),
    // face buttons
    kt("Y", "Y", 15.4, 2.9, 1.2, 1.0),
    kt("X", "X", 14.2, 4.0, 1.2, 1.0),
    kt("B", "B", 16.6, 4.0, 1.2, 1.0),
    kt("A", "A", 15.4, 5.1, 1.2, 1.0),
    // centre
    kt("Back", "Back", 7.6, 3.8, 1.4, 0.8),
    kt("Guide", "Guide", 9.3, 2.9, 1.4, 1.0),
    kt("Start", "Start", 11.0, 3.8, 1.4, 0.8),
    // d-pad
    kt("DpadUp", "\u{2191}", 6.9, 6.2, 1.1, 0.95),
    kt("DpadLeft", "\u{2190}", 5.8, 7.15, 1.1, 0.95),
    kt("DpadRight", "\u{2192}", 8.0, 7.15, 1.1, 0.95),
    kt("DpadDown", "\u{2193}", 6.9, 8.1, 1.1, 0.95),
    // right stick
    kt("RightStickUp", "\u{2191}", 12.0, 6.2, 1.1, 0.95),
    kt("RightStickLeft", "\u{2190}", 10.9, 7.15, 1.1, 0.95),
    kt("RightStickClick", "R3", 12.0, 7.15, 1.1, 0.95),
    kt("RightStickRight", "\u{2192}", 13.1, 7.15, 1.1, 0.95),
    kt("RightStickDown", "\u{2193}", 12.0, 8.1, 1.1, 0.95),
    // modifier, not a physical button
    kt("Walk", "Walk", 9.2, 10.4, 1.6, 0.9),
];

/// Short label for an output, used under bound keys.
pub fn output_short(o: Output) -> &'static str {
    match o {
        Output::A => "A",
        Output::B => "B",
        Output::X => "X",
        Output::Y => "Y",
        Output::Lb => "LB",
        Output::Rb => "RB",
        Output::Back => "Back",
        Output::Start => "Start",
        Output::Guide => "Guide",
        Output::LeftStickClick => "L3",
        Output::RightStickClick => "R3",
        Output::DpadUp => "D\u{2191}",
        Output::DpadDown => "D\u{2193}",
        Output::DpadLeft => "D\u{2190}",
        Output::DpadRight => "D\u{2192}",
        Output::Lt => "LT",
        Output::Rt => "RT",
        Output::LsUp => "LS\u{2191}",
        Output::LsDown => "LS\u{2193}",
        Output::LsLeft => "LS\u{2190}",
        Output::LsRight => "LS\u{2192}",
        Output::RsUp => "RS\u{2191}",
        Output::RsDown => "RS\u{2193}",
        Output::RsLeft => "RS\u{2190}",
        Output::RsRight => "RS\u{2192}",
        Output::Walk => "Walk",
    }
}

/// Output groups for the "map to" picker.
pub static OUTPUT_GROUPS: &[(&str, &[Output])] = &[
    (
        "Face buttons",
        &[Output::A, Output::B, Output::X, Output::Y],
    ),
    (
        "Shoulders & triggers",
        &[Output::Lb, Output::Rb, Output::Lt, Output::Rt],
    ),
    (
        "Left stick",
        &[
            Output::LsUp,
            Output::LsDown,
            Output::LsLeft,
            Output::LsRight,
            Output::LeftStickClick,
        ],
    ),
    (
        "Right stick",
        &[
            Output::RsUp,
            Output::RsDown,
            Output::RsLeft,
            Output::RsRight,
            Output::RightStickClick,
        ],
    ),
    (
        "D-pad",
        &[
            Output::DpadUp,
            Output::DpadDown,
            Output::DpadLeft,
            Output::DpadRight,
        ],
    ),
    (
        "System",
        &[Output::Back, Output::Start, Output::Guide, Output::Walk],
    ),
];

/// Resolves a keyboard or mouse slot to its input.
pub fn slot_source(s: &Slot) -> Source {
    Source::parse(s.name).expect("layout names are validated by tests")
}

/// Resolves a controller slot to its output.
pub fn slot_output(s: &Slot) -> Output {
    Output::parse(s.name).expect("layout names are validated by tests")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn overlaps(a: &Slot, b: &Slot) -> bool {
        let e = 1e-4;
        a.x < b.x + b.w - e && b.x < a.x + a.w - e && a.y < b.y + b.h - e && b.y < a.y + a.h - e
    }

    fn check(slots: &[Slot], size: (f32, f32)) {
        for (i, a) in slots.iter().enumerate() {
            assert!(a.w > 0.0 && a.h > 0.0, "{}", a.name);
            assert!(
                a.x >= 0.0
                    && a.y >= 0.0
                    && a.x + a.w <= size.0 + 1e-4
                    && a.y + a.h <= size.1 + 1e-4,
                "{} outside the picture",
                a.name
            );
            for b in &slots[i + 1..] {
                assert!(!overlaps(a, b), "{} overlaps {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn keyboard_is_complete_valid_and_non_overlapping() {
        check(KEYBOARD, KEYBOARD_SIZE);
        let mut seen = HashSet::new();
        for s in KEYBOARD {
            assert!(matches!(slot_source(s), Source::Key(_)), "{}", s.name);
            assert!(seen.insert(slot_source(s)), "{} twice", s.name);
        }
        assert_eq!(KEYBOARD.len(), 104);
    }

    #[test]
    fn mouse_covers_every_mouse_input() {
        check(MOUSE, MOUSE_SIZE);
        let srcs: HashSet<Source> = MOUSE.iter().map(slot_source).collect();
        assert_eq!(srcs.len(), 9);
        assert!(srcs.iter().all(|s| !matches!(s, Source::Key(_))));
    }

    #[test]
    fn pad_covers_every_output_exactly_once() {
        check(PAD, PAD_SIZE);
        let outs: HashSet<Output> = PAD.iter().map(slot_output).collect();
        assert_eq!(outs.len(), PAD.len());
        assert_eq!(outs.len(), Output::COUNT);
    }

    #[test]
    fn output_groups_cover_every_output_once() {
        let mut seen = HashSet::new();
        for (_, outs) in OUTPUT_GROUPS {
            for o in *outs {
                assert!(seen.insert(*o), "{o:?} twice");
                assert!(!output_short(*o).is_empty());
            }
        }
        assert_eq!(seen.len(), Output::COUNT);
    }
}
