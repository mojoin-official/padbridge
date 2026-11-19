//! Nintendo Switch Pro Controller.

use super::SwitchLayout;
use crate::pad::{Buttons, PadState};
use crate::quantize::Resolution;

/// Swap face buttons for label-based layout (profile "a" = Nintendo A = east).
pub fn apply_layout(p: &PadState, layout: SwitchLayout) -> PadState {
    let mut q = *p;
    if layout == SwitchLayout::Labels {
        let b = p.buttons;
        q.buttons.set(Buttons::SOUTH, b.has(Buttons::EAST));
        q.buttons.set(Buttons::EAST, b.has(Buttons::SOUTH));
        q.buttons.set(Buttons::WEST, b.has(Buttons::NORTH));
        q.buttons.set(Buttons::NORTH, b.has(Buttons::WEST));
    }
    q
}

// Byte 3 (right side)
pub const Y: u8 = 0x01;
pub const X: u8 = 0x02;
pub const B: u8 = 0x04;
pub const A: u8 = 0x08;
pub const R: u8 = 0x40;
pub const ZR: u8 = 0x80;
// Byte 4 (shared)
pub const MINUS: u8 = 0x01;
pub const PLUS: u8 = 0x02;
pub const RSTICK: u8 = 0x04;
pub const LSTICK: u8 = 0x08;
pub const HOME: u8 = 0x10;
pub const CAPTURE: u8 = 0x20;
// Byte 5 (left side)
pub const DOWN: u8 = 0x01;
pub const UP: u8 = 0x02;
pub const RIGHT: u8 = 0x04;
pub const LEFT: u8 = 0x08;
pub const L: u8 = 0x40;
pub const ZL: u8 = 0x80;

/// ZL/ZR are digital on a Switch Pro; analog triggers press them past this point.
pub const TRIGGER_THRESHOLD: f32 = 0.3;

fn stick(x: f32, y: f32) -> [u8; 3] {
    let x = (2048 + Resolution::SWITCH.to_code(x)) as u16;
    let y = (2048 + Resolution::SWITCH.to_code(y)) as u16; // +y = up
    [
        (x & 0xFF) as u8,
        ((x >> 8) & 0x0F) as u8 | (((y & 0x0F) as u8) << 4),
        (y >> 4) as u8,
    ]
}

/// Standard full-mode input report 0x30 (IMU zeroed). Positional: SOUTH = B, EAST = A,
/// WEST = Y, NORTH = X (apply [`apply_layout`] first for label layout).
pub fn encode_0x30(p: &PadState, timer: u32) -> [u8; 64] {
    let bt = p.buttons;
    let mut r = 0u8;
    let mut s = 0u8;
    let mut l = 0u8;
    let set = |v: &mut u8, on: bool, bit: u8| {
        if on {
            *v |= bit
        }
    };
    set(&mut r, bt.has(Buttons::SOUTH), B);
    set(&mut r, bt.has(Buttons::EAST), A);
    set(&mut r, bt.has(Buttons::WEST), Y);
    set(&mut r, bt.has(Buttons::NORTH), X);
    set(&mut r, bt.has(Buttons::R1), R);
    set(&mut r, bt.has(Buttons::R2) || p.rt >= TRIGGER_THRESHOLD, ZR);
    set(&mut s, bt.has(Buttons::SELECT), MINUS);
    set(&mut s, bt.has(Buttons::START), PLUS);
    set(&mut s, bt.has(Buttons::R3), RSTICK);
    set(&mut s, bt.has(Buttons::L3), LSTICK);
    set(&mut s, bt.has(Buttons::HOME), HOME);
    set(
        &mut s,
        bt.has(Buttons::CAPTURE) || bt.has(Buttons::TOUCHPAD),
        CAPTURE,
    );
    set(&mut l, bt.has(Buttons::DPAD_DOWN), DOWN);
    set(&mut l, bt.has(Buttons::DPAD_UP), UP);
    set(&mut l, bt.has(Buttons::DPAD_RIGHT), RIGHT);
    set(&mut l, bt.has(Buttons::DPAD_LEFT), LEFT);
    set(&mut l, bt.has(Buttons::L1), L);
    set(&mut l, bt.has(Buttons::L2) || p.lt >= TRIGGER_THRESHOLD, ZL);

    let mut b = [0u8; 64];
    b[0] = 0x30;
    b[1] = (timer & 0xFF) as u8;
    b[2] = 0x91; // battery full, Pro Controller, USB powered
    b[3] = r;
    b[4] = s;
    b[5] = l;
    b[6..9].copy_from_slice(&stick(p.lx, p.ly));
    b[9..12].copy_from_slice(&stick(p.rx, p.ry));
    b[12] = 0x80; // vibrator report
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_and_sticks() {
        let mut p = PadState::neutral();
        p.buttons.set(Buttons::SOUTH, true);
        p.rx = 1.0;
        p.ry = -1.0;
        let b = encode_0x30(&p, 0);
        assert_eq!(b[3], B);
        let b2 = encode_0x30(&apply_layout(&p, SwitchLayout::Labels), 0);
        assert_eq!(b2[3], A);
        let rx = b[9] as u16 | ((b[10] as u16 & 0x0F) << 8);
        let ry = (b[10] as u16 >> 4) | ((b[11] as u16) << 4);
        assert_eq!((rx, ry), (4095, 1));
        let c = encode_0x30(&PadState::neutral(), 0);
        let cx = c[6] as u16 | ((c[7] as u16 & 0x0F) << 8);
        assert_eq!(cx, 2048);
    }
}
