//! The virtual controller side: output names, axis targets and the XUSB report.

use crate::util::norm;

pub const BTN_DPAD_UP: u16 = 0x0001;
pub const BTN_DPAD_DOWN: u16 = 0x0002;
pub const BTN_DPAD_LEFT: u16 = 0x0004;
pub const BTN_DPAD_RIGHT: u16 = 0x0008;
pub const BTN_START: u16 = 0x0010;
pub const BTN_BACK: u16 = 0x0020;
pub const BTN_LEFT_THUMB: u16 = 0x0040;
pub const BTN_RIGHT_THUMB: u16 = 0x0080;
pub const BTN_LB: u16 = 0x0100;
pub const BTN_RB: u16 = 0x0200;
pub const BTN_GUIDE: u16 = 0x0400;
pub const BTN_A: u16 = 0x1000;
pub const BTN_B: u16 = 0x2000;
pub const BTN_X: u16 = 0x4000;
pub const BTN_Y: u16 = 0x8000;

/// Largest stick magnitude we ever emit. Symmetric on purpose: -32768 would make a fully
/// deflected stick slightly longer in one direction than the other.
pub const STICK_MAX: i16 = 32767;

/// Everything a physical input can be bound *to* (digital outputs; analog mouse motion is bound
/// separately through [`AxisTarget`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Output {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Back,
    Start,
    Guide,
    LeftStickClick,
    RightStickClick,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    Lt,
    Rt,
    LsUp,
    LsDown,
    LsLeft,
    LsRight,
    RsUp,
    RsDown,
    RsLeft,
    RsRight,
    /// Modifier: while held, digital left-stick deflection is scaled by `walk_scale`.
    Walk,
}

const OUTPUT_NAMES: [&str; Output::COUNT] = [
    "A",
    "B",
    "X",
    "Y",
    "LB",
    "RB",
    "Back",
    "Start",
    "Guide",
    "LeftStickClick",
    "RightStickClick",
    "DpadUp",
    "DpadDown",
    "DpadLeft",
    "DpadRight",
    "LT",
    "RT",
    "LeftStickUp",
    "LeftStickDown",
    "LeftStickLeft",
    "LeftStickRight",
    "RightStickUp",
    "RightStickDown",
    "RightStickLeft",
    "RightStickRight",
    "Walk",
];

static OUTPUT_ALIASES: &[(&str, Output)] = &[
    ("L3", Output::LeftStickClick),
    ("LS", Output::LeftStickClick),
    ("LeftThumb", Output::LeftStickClick),
    ("R3", Output::RightStickClick),
    ("RS", Output::RightStickClick),
    ("RightThumb", Output::RightStickClick),
    ("L1", Output::Lb),
    ("LeftBumper", Output::Lb),
    ("LeftShoulder", Output::Lb),
    ("R1", Output::Rb),
    ("RightBumper", Output::Rb),
    ("RightShoulder", Output::Rb),
    ("L2", Output::Lt),
    ("LeftTrigger", Output::Lt),
    ("R2", Output::Rt),
    ("RightTrigger", Output::Rt),
    ("View", Output::Back),
    ("Select", Output::Back),
    ("Menu", Output::Start),
    ("Xbox", Output::Guide),
    ("LSUp", Output::LsUp),
    ("LSDown", Output::LsDown),
    ("LSLeft", Output::LsLeft),
    ("LSRight", Output::LsRight),
    ("RSUp", Output::RsUp),
    ("RSDown", Output::RsDown),
    ("RSLeft", Output::RsLeft),
    ("RSRight", Output::RsRight),
];

impl Output {
    pub const COUNT: usize = 26;

    pub const ALL: [Output; Output::COUNT] = [
        Output::A,
        Output::B,
        Output::X,
        Output::Y,
        Output::Lb,
        Output::Rb,
        Output::Back,
        Output::Start,
        Output::Guide,
        Output::LeftStickClick,
        Output::RightStickClick,
        Output::DpadUp,
        Output::DpadDown,
        Output::DpadLeft,
        Output::DpadRight,
        Output::Lt,
        Output::Rt,
        Output::LsUp,
        Output::LsDown,
        Output::LsLeft,
        Output::LsRight,
        Output::RsUp,
        Output::RsDown,
        Output::RsLeft,
        Output::RsRight,
        Output::Walk,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        OUTPUT_NAMES[self as usize]
    }

    pub fn parse(text: &str) -> Option<Output> {
        let n = norm(text);
        Output::ALL
            .iter()
            .copied()
            .find(|o| norm(o.name()) == n)
            .or_else(|| {
                OUTPUT_ALIASES
                    .iter()
                    .find(|(a, _)| norm(a) == n)
                    .map(|(_, o)| *o)
            })
    }

    /// Canonical names plus aliases (for suggestions).
    pub fn all_names_with_aliases() -> Vec<&'static str> {
        OUTPUT_NAMES
            .iter()
            .copied()
            .chain(OUTPUT_ALIASES.iter().map(|(a, _)| *a))
            .collect()
    }

    /// The XUSB button bit for plain button outputs (D-pad bits are produced from resolved axes
    /// by the mapper, not from here).
    pub fn button_bit(self) -> Option<u16> {
        Some(match self {
            Output::A => BTN_A,
            Output::B => BTN_B,
            Output::X => BTN_X,
            Output::Y => BTN_Y,
            Output::Lb => BTN_LB,
            Output::Rb => BTN_RB,
            Output::Back => BTN_BACK,
            Output::Start => BTN_START,
            Output::Guide => BTN_GUIDE,
            Output::LeftStickClick => BTN_LEFT_THUMB,
            Output::RightStickClick => BTN_RIGHT_THUMB,
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Stick {
    Left,
    Right,
}

/// The analog axes mouse motion can be bound to.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AxisTarget {
    LeftX,
    LeftY,
    RightX,
    RightY,
}

impl AxisTarget {
    pub const ALL: [AxisTarget; 4] = [
        AxisTarget::LeftX,
        AxisTarget::LeftY,
        AxisTarget::RightX,
        AxisTarget::RightY,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AxisTarget::LeftX => "LeftStickX",
            AxisTarget::LeftY => "LeftStickY",
            AxisTarget::RightX => "RightStickX",
            AxisTarget::RightY => "RightStickY",
        }
    }

    pub fn parse(text: &str) -> Option<AxisTarget> {
        let n = norm(text);
        AxisTarget::ALL
            .iter()
            .copied()
            .find(|a| norm(a.name()) == n)
            .or(match n.as_str() {
                "lsx" => Some(AxisTarget::LeftX),
                "lsy" => Some(AxisTarget::LeftY),
                "rsx" => Some(AxisTarget::RightX),
                "rsy" => Some(AxisTarget::RightY),
                _ => None,
            })
    }

    pub fn stick(self) -> Stick {
        match self {
            AxisTarget::LeftX | AxisTarget::LeftY => Stick::Left,
            AxisTarget::RightX | AxisTarget::RightY => Stick::Right,
        }
    }

    pub fn is_y(self) -> bool {
        matches!(self, AxisTarget::LeftY | AxisTarget::RightY)
    }
}

/// One complete Xbox 360 input report.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

/// Converts a normalised axis value in `[-1, 1]` to the XInput integer range.
pub fn axis_to_i16(v: f64) -> i16 {
    (v * f64::from(STICK_MAX))
        .round()
        .clamp(-f64::from(STICK_MAX), f64::from(STICK_MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn all_is_in_enum_order_and_names_unique() {
        let mut seen = HashSet::new();
        for (i, o) in Output::ALL.iter().enumerate() {
            assert_eq!(o.index(), i);
            assert!(seen.insert(norm(o.name())));
        }
        assert_eq!(Output::ALL.len(), OUTPUT_NAMES.len());
    }

    #[test]
    fn names_round_trip_and_aliases_resolve() {
        for o in Output::ALL {
            assert_eq!(Output::parse(o.name()), Some(o));
        }
        for (a, o) in OUTPUT_ALIASES {
            assert_eq!(Output::parse(a), Some(*o));
            assert!(
                !OUTPUT_NAMES.iter().any(|n| norm(n) == norm(a)),
                "alias {a} shadows a canonical name"
            );
        }
        assert_eq!(Output::parse("left_stick-up"), Some(Output::LsUp));
        assert_eq!(Output::parse("nonsense"), None);
    }

    #[test]
    fn button_bits_are_distinct_and_exclude_dpad() {
        let mut mask = 0u16;
        for o in Output::ALL {
            if let Some(bit) = o.button_bit() {
                assert_eq!(mask & bit, 0, "bit reused by {:?}", o);
                mask |= bit;
            }
        }
        assert_eq!(mask & 0x000F, 0);
        // Matches the ViGEm / XUSB layout verified against the vigem-client source.
        assert_eq!(BTN_A | BTN_B | BTN_X | BTN_Y, 0xF000);
        assert_eq!(BTN_LB | BTN_RB | BTN_GUIDE, 0x0700);
    }

    #[test]
    fn axis_targets() {
        assert_eq!(AxisTarget::parse("right_stick_x"), Some(AxisTarget::RightX));
        assert_eq!(AxisTarget::parse("LSY"), Some(AxisTarget::LeftY));
        assert_eq!(AxisTarget::parse("x"), None);
        assert_eq!(AxisTarget::RightY.stick(), Stick::Right);
        assert!(AxisTarget::LeftY.is_y() && !AxisTarget::LeftX.is_y());
    }

    #[test]
    fn i16_conversion_is_symmetric_and_clamped() {
        assert_eq!(axis_to_i16(0.0), 0);
        assert_eq!(axis_to_i16(1.0), 32767);
        assert_eq!(axis_to_i16(-1.0), -32767);
        assert_eq!(axis_to_i16(5.0), 32767);
        assert_eq!(axis_to_i16(-5.0), -32767);
        assert_eq!(axis_to_i16(0.5), 16384);
    }
}
