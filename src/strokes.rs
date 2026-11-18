//! Decoding of Interception driver strokes (keyboard and mouse packets read below Windows).
//! Pure data handling, so it is tested on any platform; `driver.rs` does the I/O.

use crate::capture::Edge;
use crate::keys::{KeyId, MouseButton, KEY_NUMLOCK, KEY_PAUSE};

// Key stroke `state` bits (interception.h).
pub const KEY_UP: u16 = 0x01;
pub const KEY_E0: u16 = 0x02;
pub const KEY_E1: u16 = 0x04;

// Mouse stroke `state` bits.
pub const MOUSE_LEFT_DOWN: u16 = 0x001;
pub const MOUSE_LEFT_UP: u16 = 0x002;
pub const MOUSE_RIGHT_DOWN: u16 = 0x004;
pub const MOUSE_RIGHT_UP: u16 = 0x008;
pub const MOUSE_MIDDLE_DOWN: u16 = 0x010;
pub const MOUSE_MIDDLE_UP: u16 = 0x020;
pub const MOUSE_B4_DOWN: u16 = 0x040;
pub const MOUSE_B4_UP: u16 = 0x080;
pub const MOUSE_B5_DOWN: u16 = 0x100;
pub const MOUSE_B5_UP: u16 = 0x200;
pub const MOUSE_WHEEL: u16 = 0x400;
pub const MOUSE_HWHEEL: u16 = 0x800;

// Mouse stroke `flags` bits.
pub const MOUSE_MOVE_ABSOLUTE: u16 = 0x001;

/// Filter value meaning "every event of this device class".
pub const FILTER_ALL: u16 = 0xFFFF;

/// Every button edge a mouse stroke can carry: (state bit, button, edge).
pub const MOUSE_BUTTON_BITS: [(u16, MouseButton, Edge); 10] = [
    (MOUSE_LEFT_DOWN, MouseButton::Left, Edge::Down),
    (MOUSE_LEFT_UP, MouseButton::Left, Edge::Up),
    (MOUSE_RIGHT_DOWN, MouseButton::Right, Edge::Down),
    (MOUSE_RIGHT_UP, MouseButton::Right, Edge::Up),
    (MOUSE_MIDDLE_DOWN, MouseButton::Middle, Edge::Down),
    (MOUSE_MIDDLE_UP, MouseButton::Middle, Edge::Up),
    (MOUSE_B4_DOWN, MouseButton::X1, Edge::Down),
    (MOUSE_B4_UP, MouseButton::X1, Edge::Up),
    (MOUSE_B5_DOWN, MouseButton::X2, Edge::Down),
    (MOUSE_B5_UP, MouseButton::X2, Edge::Up),
];

/// `InterceptionKeyStroke`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyStroke {
    pub code: u16,
    pub state: u16,
    pub information: u32,
}

/// `InterceptionMouseStroke`. Also the size of `InterceptionStroke`, the buffer type the driver
/// API reads and writes.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MouseStroke {
    pub state: u16,
    pub flags: u16,
    pub rolling: i16,
    pub x: i32,
    pub y: i32,
    pub information: u32,
}

const _: () = {
    assert!(std::mem::size_of::<KeyStroke>() == 8);
    assert!(std::mem::size_of::<MouseStroke>() == 20);
    assert!(std::mem::offset_of!(MouseStroke, x) == 8);
};

/// `InterceptionStroke`: the raw 20-byte buffer the driver API reads and writes. A keyboard
/// stroke occupies its first 8 bytes, a mouse stroke all 20 (with 2 padding bytes after
/// `rolling`). Decoded field by field, so no unsafe casts are needed.
#[repr(C, align(4))]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct StrokeBuf(pub [u8; 20]);

impl StrokeBuf {
    fn u16_at(&self, i: usize) -> u16 {
        u16::from_ne_bytes([self.0[i], self.0[i + 1]])
    }
    fn u32_at(&self, i: usize) -> u32 {
        u32::from_ne_bytes([self.0[i], self.0[i + 1], self.0[i + 2], self.0[i + 3]])
    }
    fn put(&mut self, i: usize, bytes: &[u8]) {
        self.0[i..i + bytes.len()].copy_from_slice(bytes);
    }

    pub fn key(&self) -> KeyStroke {
        KeyStroke {
            code: self.u16_at(0),
            state: self.u16_at(2),
            information: self.u32_at(4),
        }
    }

    pub fn set_key(&mut self, k: &KeyStroke) {
        self.put(0, &k.code.to_ne_bytes());
        self.put(2, &k.state.to_ne_bytes());
        self.put(4, &k.information.to_ne_bytes());
    }

    pub fn mouse(&self) -> MouseStroke {
        MouseStroke {
            state: self.u16_at(0),
            flags: self.u16_at(2),
            rolling: self.u16_at(4) as i16,
            x: self.u32_at(8) as i32,
            y: self.u32_at(12) as i32,
            information: self.u32_at(16),
        }
    }

    /// Writes the mouse fields; the padding bytes are left as received.
    pub fn set_mouse(&mut self, m: &MouseStroke) {
        self.put(0, &m.state.to_ne_bytes());
        self.put(2, &m.flags.to_ne_bytes());
        self.put(4, &m.rolling.to_ne_bytes());
        self.put(8, &m.x.to_ne_bytes());
        self.put(12, &m.y.to_ne_bytes());
        self.put(16, &m.information.to_ne_bytes());
    }
}

/// What one keyboard stroke means.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    /// A real key edge.
    Key(KeyId, Edge),
    /// Pass through untouched (fake shifts the keyboard driver adds around navigation keys).
    Ignore,
    /// The second half of the Pause sequence; give it the same verdict as the first half.
    PauseTail(Edge),
}

/// Decodes key strokes. Stateful only for the two-stroke Pause key (`E1 1D`, then `45`).
#[derive(Default, Debug)]
pub struct KeyDecoder {
    pause_pending: bool,
}

impl KeyDecoder {
    pub fn decode(&mut self, k: &KeyStroke) -> KeyEvent {
        let edge = if k.state & KEY_UP != 0 {
            Edge::Up
        } else {
            Edge::Down
        };
        let code = k.code & 0xFF;
        let e0 = k.state & KEY_E0 != 0;
        let e1 = k.state & KEY_E1 != 0;
        if e1 && code == 0x1D {
            self.pause_pending = true;
            return KeyEvent::Key(KEY_PAUSE, edge);
        }
        if self.pause_pending {
            self.pause_pending = false;
            if code == 0x45 && !e0 {
                return KeyEvent::PauseTail(edge);
            }
        }
        if code == 0 || (e0 && (code == 0x2A || code == 0x36)) {
            return KeyEvent::Ignore;
        }
        if !e0 && code == 0x45 {
            return KeyEvent::Key(KEY_NUMLOCK, edge);
        }
        KeyEvent::Key(if e0 { 0xE000 | code } else { code }, edge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(code: u16, state: u16) -> KeyStroke {
        KeyStroke {
            code,
            state,
            information: 0,
        }
    }

    #[test]
    fn plain_and_extended_keys() {
        let mut d = KeyDecoder::default();
        assert_eq!(d.decode(&k(0x11, 0)), KeyEvent::Key(0x11, Edge::Down)); // W
        assert_eq!(d.decode(&k(0x11, KEY_UP)), KeyEvent::Key(0x11, Edge::Up));
        assert_eq!(
            d.decode(&k(0x1D, KEY_E0)),
            KeyEvent::Key(0xE01D, Edge::Down)
        ); // right ctrl
        assert_eq!(
            d.decode(&k(0x1C, KEY_E0 | KEY_UP)),
            KeyEvent::Key(0xE01C, Edge::Up)
        ); // numpad enter
        assert_eq!(d.decode(&k(0x1C, 0)), KeyEvent::Key(0x1C, Edge::Down)); // enter
    }

    #[test]
    fn names_match_the_hook_path() {
        // Same ids the low-level hook produces, so bindings work identically in both modes.
        use crate::keys::Source;
        let mut d = KeyDecoder::default();
        for (name, code, state) in [
            ("W", 0x11, 0),
            ("RCtrl", 0x1D, KEY_E0),
            ("Up", 0x48, KEY_E0),
            ("Numpad8", 0x48, 0),
            ("NumpadEnter", 0x1C, KEY_E0),
            ("LWin", 0x5B, KEY_E0),
        ] {
            match d.decode(&k(code, state)) {
                KeyEvent::Key(id, _) => assert_eq!(Source::Key(id), Source::parse(name).unwrap()),
                other => panic!("{name}: {other:?}"),
            }
        }
    }

    #[test]
    fn fake_shifts_are_ignored() {
        let mut d = KeyDecoder::default();
        assert_eq!(d.decode(&k(0x2A, KEY_E0)), KeyEvent::Ignore);
        assert_eq!(d.decode(&k(0x36, KEY_E0 | KEY_UP)), KeyEvent::Ignore);
        assert_eq!(d.decode(&k(0x2A, 0)), KeyEvent::Key(0x2A, Edge::Down));
    }

    #[test]
    fn pause_is_one_key_and_numlock_another() {
        let mut d = KeyDecoder::default();
        assert_eq!(
            d.decode(&k(0x1D, KEY_E1)),
            KeyEvent::Key(KEY_PAUSE, Edge::Down)
        );
        assert_eq!(d.decode(&k(0x45, 0)), KeyEvent::PauseTail(Edge::Down));
        assert_eq!(
            d.decode(&k(0x1D, KEY_E1 | KEY_UP)),
            KeyEvent::Key(KEY_PAUSE, Edge::Up)
        );
        assert_eq!(d.decode(&k(0x45, KEY_UP)), KeyEvent::PauseTail(Edge::Up));
        // a NumLock press on its own
        assert_eq!(
            d.decode(&k(0x45, 0)),
            KeyEvent::Key(KEY_NUMLOCK, Edge::Down)
        );
    }

    #[test]
    fn strokes_round_trip_through_the_shared_buffer() {
        let key = KeyStroke {
            code: 0x1E,
            state: KEY_E0 | KEY_UP,
            information: 0xDEAD_BEEF,
        };
        let mut buf = StrokeBuf::default();
        buf.set_key(&key);
        assert_eq!(buf.key(), key);
        assert_eq!(&buf.0[8..], &[0u8; 12], "a key stroke only uses 8 bytes");

        let m = MouseStroke {
            state: MOUSE_LEFT_DOWN | MOUSE_WHEEL,
            flags: 0,
            rolling: -120,
            x: -5,
            y: 70000,
            information: 42,
        };
        let mut buf = StrokeBuf([0xAB; 20]);
        buf.set_mouse(&m);
        assert_eq!(buf.mouse(), m);
        assert_eq!(&buf.0[6..8], &[0xAB, 0xAB], "padding left as received");
    }

    #[test]
    fn buffer_layout_matches_the_c_structs() {
        // Little-endian x86/x64: InterceptionMouseStroke { u16 state; u16 flags; i16 rolling;
        // <2 pad>; i32 x; i32 y; u32 information; }
        let mut raw = [0u8; 20];
        raw[0] = 0x01; // LEFT_DOWN
        raw[4..6].copy_from_slice(&(-120i16).to_le_bytes());
        raw[8..12].copy_from_slice(&7i32.to_le_bytes());
        raw[12..16].copy_from_slice(&(-3i32).to_le_bytes());
        let m = StrokeBuf(raw).mouse();
        assert_eq!(
            (m.state, m.rolling, m.x, m.y),
            (MOUSE_LEFT_DOWN, -120, 7, -3)
        );
        assert_eq!(std::mem::size_of::<StrokeBuf>(), 20);
    }

    #[test]
    fn button_bits_are_distinct() {
        let mut seen = 0u16;
        for (bit, _, _) in MOUSE_BUTTON_BITS {
            assert_eq!(seen & bit, 0);
            seen |= bit;
        }
        assert_eq!(seen & (MOUSE_WHEEL | MOUSE_HWHEEL), 0);
    }
}
