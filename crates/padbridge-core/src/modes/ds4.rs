//! DualShock 4 reports. Also used for "PlayStation" mode: ViGEmBus can only emulate a
//! DS4, which PS4/PS5-aware PC games show PlayStation prompts for.

use crate::pad::{Buttons, PadState};
use crate::quantize::Resolution;
use crate::touch::TouchContact;

/// Same layout as ViGEm's `DS4_REPORT` / vigem-client `DS4Report`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ds4Report {
    pub thumb_lx: u8,
    pub thumb_ly: u8,
    pub thumb_rx: u8,
    pub thumb_ry: u8,
    /// Low nibble: d-pad hat (8 = released). Then face, shoulder and stick buttons.
    pub buttons: u16,
    /// Bit 0: PS, bit 1: touchpad click.
    pub special: u8,
    pub trigger_l: u8,
    pub trigger_r: u8,
}

impl Default for Ds4Report {
    fn default() -> Self {
        Self {
            thumb_lx: 0x80,
            thumb_ly: 0x80,
            thumb_rx: 0x80,
            thumb_ry: 0x80,
            buttons: 0x8,
            special: 0,
            trigger_l: 0,
            trigger_r: 0,
        }
    }
}

pub const SQUARE: u16 = 1 << 4;
pub const CROSS: u16 = 1 << 5;
pub const CIRCLE: u16 = 1 << 6;
pub const TRIANGLE: u16 = 1 << 7;
pub const L1: u16 = 1 << 8;
pub const R1: u16 = 1 << 9;
pub const L2: u16 = 1 << 10;
pub const R2: u16 = 1 << 11;
pub const SHARE: u16 = 1 << 12;
pub const OPTIONS: u16 = 1 << 13;
pub const L3: u16 = 1 << 14;
pub const R3: u16 = 1 << 15;
pub const SPECIAL_PS: u8 = 1 << 0;
pub const SPECIAL_TOUCHPAD: u8 = 1 << 1;

/// Touchpad size in DS4 units.
pub const TOUCH_W: f32 = 1920.0;
pub const TOUCH_H: f32 = 942.0;

const MAP: &[(u32, u16)] = &[
    (Buttons::WEST, SQUARE),
    (Buttons::SOUTH, CROSS),
    (Buttons::EAST, CIRCLE),
    (Buttons::NORTH, TRIANGLE),
    (Buttons::L1, L1),
    (Buttons::R1, R1),
    (Buttons::SELECT, SHARE),
    (Buttons::CAPTURE, SHARE),
    (Buttons::START, OPTIONS),
    (Buttons::L3, L3),
    (Buttons::R3, R3),
];

/// x: 0 = left. y: 0 = UP (DS4 convention), so +y (up) maps to smaller values.
fn axis_x(v: f32) -> u8 {
    (128 + Resolution::DS4.to_code(v)) as u8
}
fn axis_y(v: f32) -> u8 {
    (128 - Resolution::DS4.to_code(v)) as u8
}

pub fn encode(p: &PadState) -> Ds4Report {
    let mut buttons = p.hat() as u16;
    for &(b, d) in MAP {
        if p.buttons.has(b) {
            buttons |= d;
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
    if super::digital_trigger(p, Buttons::L2, lt) {
        buttons |= L2;
    }
    if super::digital_trigger(p, Buttons::R2, rt) {
        buttons |= R2;
    }
    let mut special = 0;
    if p.buttons.has(Buttons::HOME) {
        special |= SPECIAL_PS;
    }
    if p.buttons.has(Buttons::TOUCHPAD) {
        special |= SPECIAL_TOUCHPAD;
    }
    Ds4Report {
        thumb_lx: axis_x(p.lx),
        thumb_ly: axis_y(p.ly),
        thumb_rx: axis_x(p.rx),
        thumb_ry: axis_y(p.ry),
        buttons,
        special,
        trigger_l: super::trig_u8(lt),
        trigger_r: super::trig_u8(rt),
    }
}

/// Full 64-byte DS4 USB input report (id 0x01) with touchpad fingers, for backends that
/// accept raw reports (ViGEm's extended DS4 report starts at byte 1 of this).
/// `touches` are in SCREEN pixels of a `screen_w x screen_h` surface and are scaled to
/// the touchpad; at most two are used.
pub fn encode_usb_full(
    p: &PadState,
    touches: &[TouchContact],
    screen: (f32, f32),
    counter: u8,
    timestamp: u16,
) -> [u8; 64] {
    let r = encode(p);
    let mut b = [0u8; 64];
    b[0] = 0x01;
    b[1] = r.thumb_lx;
    b[2] = r.thumb_ly;
    b[3] = r.thumb_rx;
    b[4] = r.thumb_ry;
    b[5..7].copy_from_slice(&r.buttons.to_le_bytes());
    b[7] = (r.special & 0x03) | ((counter & 0x3F) << 2);
    b[8] = r.trigger_l;
    b[9] = r.trigger_r;
    b[10..12].copy_from_slice(&timestamp.to_le_bytes());
    b[30] = 0x1B; // cable connected, battery full
    b[33] = 1; // one touch packet
    b[34] = counter;
    for slot in 0..2 {
        let off = 35 + slot * 4;
        match touches.get(slot) {
            Some(t) if t.down => {
                let x = ((t.x / (screen.0 - 1.0).max(1.0)) * (TOUCH_W - 1.0))
                    .round()
                    .clamp(0.0, TOUCH_W - 1.0) as u16;
                let y = ((t.y / (screen.1 - 1.0).max(1.0)) * (TOUCH_H - 1.0))
                    .round()
                    .clamp(0.0, TOUCH_H - 1.0) as u16;
                b[off] = t.id & 0x7F;
                b[off + 1] = (x & 0xFF) as u8;
                b[off + 2] = ((x >> 8) & 0x0F) as u8 | (((y & 0x0F) as u8) << 4);
                b[off + 3] = (y >> 4) as u8;
            }
            _ => b[off] = 0x80, // not touching
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_matches_vigem_default() {
        assert_eq!(encode(&PadState::neutral()), Ds4Report::default());
    }
    #[test]
    fn buttons_axes_triggers() {
        let mut p = PadState::neutral();
        p.buttons.set(Buttons::SOUTH, true);
        p.buttons.set(Buttons::DPAD_UP, true);
        p.buttons.set(Buttons::HOME, true);
        p.ly = 1.0;
        p.rt = 0.5;
        let r = encode(&p);
        assert_eq!(r.buttons, (CROSS | R2));
        assert_eq!(r.thumb_ly, 1);
        assert_eq!(r.trigger_r, 128);
        assert_eq!(r.special, SPECIAL_PS);
    }
    #[test]
    fn touch_packing() {
        let t = [TouchContact {
            id: 3,
            x: 1919.0,
            y: 941.0,
            down: true,
        }];
        let b = encode_usb_full(&PadState::neutral(), &t, (1920.0, 942.0), 0, 0);
        let x = b[36] as u16 | ((b[37] as u16 & 0x0F) << 8);
        let y = (b[37] as u16 >> 4) | ((b[38] as u16) << 4);
        assert_eq!((b[35], x, y), (3, 1919, 941));
        assert_eq!(b[39], 0x80);
    }
}
