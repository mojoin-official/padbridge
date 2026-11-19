//! Xbox 360 / XInput (XUSB) report.

use crate::pad::{Buttons, PadState};
use crate::quantize::Resolution;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct XUsbReport {
    pub buttons: u16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub thumb_lx: i16,
    pub thumb_ly: i16,
    pub thumb_rx: i16,
    pub thumb_ry: i16,
}

pub const UP: u16 = 0x0001;
pub const DOWN: u16 = 0x0002;
pub const LEFT: u16 = 0x0004;
pub const RIGHT: u16 = 0x0008;
pub const START: u16 = 0x0010;
pub const BACK: u16 = 0x0020;
pub const LTHUMB: u16 = 0x0040;
pub const RTHUMB: u16 = 0x0080;
pub const LB: u16 = 0x0100;
pub const RB: u16 = 0x0200;
pub const GUIDE: u16 = 0x0400;
pub const A: u16 = 0x1000;
pub const B: u16 = 0x2000;
pub const X: u16 = 0x4000;
pub const Y: u16 = 0x8000;

const MAP: &[(u32, u16)] = &[
    (Buttons::DPAD_UP, UP),
    (Buttons::DPAD_DOWN, DOWN),
    (Buttons::DPAD_LEFT, LEFT),
    (Buttons::DPAD_RIGHT, RIGHT),
    (Buttons::START, START),
    (Buttons::SELECT, BACK),
    (Buttons::L3, LTHUMB),
    (Buttons::R3, RTHUMB),
    (Buttons::L1, LB),
    (Buttons::R1, RB),
    (Buttons::HOME, GUIDE),
    (Buttons::SOUTH, A),
    (Buttons::EAST, B),
    (Buttons::WEST, X),
    (Buttons::NORTH, Y),
    // No native equivalent: touchpad click -> Back, capture -> Back.
    (Buttons::TOUCHPAD, BACK),
    (Buttons::CAPTURE, BACK),
];

fn axis(v: f32) -> i16 {
    Resolution::XINPUT.to_code(v) as i16
}

pub fn encode(p: &PadState) -> XUsbReport {
    let mut buttons = 0u16;
    for &(b, x) in MAP {
        if p.buttons.has(b) {
            buttons |= x;
        }
    }
    let lt = if p.buttons.has(Buttons::L2) {
        p.lt.max(1.0)
    } else {
        p.lt
    };
    let rt = if p.buttons.has(Buttons::R2) {
        p.rt.max(1.0)
    } else {
        p.rt
    };
    XUsbReport {
        buttons,
        left_trigger: super::trig_u8(lt),
        right_trigger: super::trig_u8(rt),
        thumb_lx: axis(p.lx),
        thumb_ly: axis(p.ly),
        thumb_rx: axis(p.rx),
        thumb_ry: axis(p.ry),
    }
}

impl XUsbReport {
    /// XUSB_REPORT / XINPUT_GAMEPAD byte layout (12 bytes, little endian).
    pub fn to_bytes(&self) -> [u8; 12] {
        let mut b = [0u8; 12];
        b[0..2].copy_from_slice(&self.buttons.to_le_bytes());
        b[2] = self.left_trigger;
        b[3] = self.right_trigger;
        b[4..6].copy_from_slice(&self.thumb_lx.to_le_bytes());
        b[6..8].copy_from_slice(&self.thumb_ly.to_le_bytes());
        b[8..10].copy_from_slice(&self.thumb_rx.to_le_bytes());
        b[10..12].copy_from_slice(&self.thumb_ry.to_le_bytes());
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basic() {
        let mut p = PadState::neutral();
        p.buttons.set(Buttons::SOUTH, true);
        p.buttons.set(Buttons::DPAD_RIGHT, true);
        p.rx = -1.0;
        p.ry = 0.5;
        p.rt = 1.0;
        let r = encode(&p);
        assert_eq!(r.buttons, A | RIGHT);
        assert_eq!(r.thumb_rx, -32767);
        assert_eq!(r.thumb_ry, 16384);
        assert_eq!(r.right_trigger, 255);
    }
}
