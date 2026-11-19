//! Canonical controller state, independent of the emulated controller.
//!
//! Face buttons are named by POSITION (south/east/west/north) so one profile works for
//! every mode: South = Xbox A = PlayStation Cross = Nintendo B.

/// Button bit set (positional naming).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Buttons(pub u32);

impl Buttons {
    pub const SOUTH: u32 = 1 << 0;
    pub const EAST: u32 = 1 << 1;
    pub const WEST: u32 = 1 << 2;
    pub const NORTH: u32 = 1 << 3;
    pub const L1: u32 = 1 << 4; // LB / L1 / L
    pub const R1: u32 = 1 << 5; // RB / R1 / R
    pub const SELECT: u32 = 1 << 6; // Back / Share / Minus
    pub const START: u32 = 1 << 7; // Start / Options / Plus
    pub const L3: u32 = 1 << 8;
    pub const R3: u32 = 1 << 9;
    pub const HOME: u32 = 1 << 10; // Guide / PS / Home
    pub const DPAD_UP: u32 = 1 << 11;
    pub const DPAD_DOWN: u32 = 1 << 12;
    pub const DPAD_LEFT: u32 = 1 << 13;
    pub const DPAD_RIGHT: u32 = 1 << 14;
    pub const TOUCHPAD: u32 = 1 << 15; // DS4 touchpad click
    pub const CAPTURE: u32 = 1 << 16; // Switch capture
    /// Digital trigger bits. Set automatically by encoders from the analog value when
    /// the target needs them (DS4 L2/R2 bits, Switch ZL/ZR).
    pub const L2: u32 = 1 << 17;
    pub const R2: u32 = 1 << 18;

    #[inline]
    pub fn has(self, b: u32) -> bool {
        self.0 & b != 0
    }
    #[inline]
    pub fn set(&mut self, b: u32, on: bool) {
        if on {
            self.0 |= b
        } else {
            self.0 &= !b
        }
    }

    /// Every name accepted by [`Buttons::from_name`].
    pub const NAMES: &'static [(&'static str, u32)] = &[
        ("south", Self::SOUTH),
        ("a", Self::SOUTH),
        ("cross", Self::SOUTH),
        ("east", Self::EAST),
        ("b", Self::EAST),
        ("circle", Self::EAST),
        ("west", Self::WEST),
        ("x", Self::WEST),
        ("square", Self::WEST),
        ("north", Self::NORTH),
        ("y", Self::NORTH),
        ("triangle", Self::NORTH),
        ("l1", Self::L1),
        ("lb", Self::L1),
        ("r1", Self::R1),
        ("rb", Self::R1),
        ("select", Self::SELECT),
        ("back", Self::SELECT),
        ("share", Self::SELECT),
        ("minus", Self::SELECT),
        ("start", Self::START),
        ("options", Self::START),
        ("plus", Self::START),
        ("l3", Self::L3),
        ("ls", Self::L3),
        ("r3", Self::R3),
        ("rs", Self::R3),
        ("home", Self::HOME),
        ("guide", Self::HOME),
        ("ps", Self::HOME),
        ("dpadup", Self::DPAD_UP),
        ("dpaddown", Self::DPAD_DOWN),
        ("dpadleft", Self::DPAD_LEFT),
        ("dpadright", Self::DPAD_RIGHT),
        ("touchpad", Self::TOUCHPAD),
        ("capture", Self::CAPTURE),
    ];

    /// Case-insensitive, ignores `_`, `-` and spaces. Note: "a/b/x/y" are XBOX labels
    /// (positional), not Nintendo labels.
    pub fn from_name(name: &str) -> Option<u32> {
        let n: String = name
            .chars()
            .filter(|c| !matches!(c, '_' | '-' | ' '))
            .flat_map(char::to_lowercase)
            .collect();
        Self::NAMES.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)
    }
}

/// Normalized controller state. Sticks are RAW stick positions in [-1, 1] with +y = up
/// (what the controller reports, before the game's deadzone/curve). Triggers in [0, 1].
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PadState {
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
    pub buttons: Buttons,
}

impl PadState {
    pub fn neutral() -> Self {
        Self::default()
    }

    /// D-pad as a DS4-style hat: 0 = N, clockwise to 7 = NW, 8 = released.
    /// Opposite directions cancel.
    pub fn hat(&self) -> u8 {
        let b = self.buttons;
        let up = b.has(Buttons::DPAD_UP) && !b.has(Buttons::DPAD_DOWN);
        let down = b.has(Buttons::DPAD_DOWN) && !b.has(Buttons::DPAD_UP);
        let left = b.has(Buttons::DPAD_LEFT) && !b.has(Buttons::DPAD_RIGHT);
        let right = b.has(Buttons::DPAD_RIGHT) && !b.has(Buttons::DPAD_LEFT);
        match (up, right, down, left) {
            (true, false, _, false) => 0,
            (true, true, _, _) => 1,
            (false, true, false, _) => 2,
            (_, true, true, _) => 3,
            (_, false, true, false) => 4,
            (_, _, true, true) => 5,
            (false, _, false, true) => 6,
            (true, _, _, true) => 7,
            _ => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(Buttons::from_name("Dpad_Up"), Some(Buttons::DPAD_UP));
        assert_eq!(Buttons::from_name("CROSS"), Some(Buttons::SOUTH));
        assert_eq!(Buttons::from_name("nope"), None);
    }
    #[test]
    fn hat() {
        let mut p = PadState::neutral();
        assert_eq!(p.hat(), 8);
        p.buttons.set(Buttons::DPAD_UP, true);
        p.buttons.set(Buttons::DPAD_RIGHT, true);
        assert_eq!(p.hat(), 1);
        p.buttons.set(Buttons::DPAD_DOWN, true); // up+down cancel -> right
        assert_eq!(p.hat(), 2);
        let mut q = PadState::neutral();
        q.buttons.set(Buttons::DPAD_LEFT, true);
        q.buttons.set(Buttons::DPAD_DOWN, true);
        assert_eq!(q.hat(), 5);
    }
}
