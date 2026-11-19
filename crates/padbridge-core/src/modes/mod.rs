//! Emulated controller types and their wire formats.
//!
//! | Mode          | Windows backend today           | Stick bits | Notes                                  |
//! |---------------|---------------------------------|-----------:|----------------------------------------|
//! | `xinput`      | ViGEmBus Xbox 360 target        | 16         | Widest game support                    |
//! | `ds4`         | ViGEmBus DualShock 4 target     | 8          | PlayStation prompts, touchpad click    |
//! | `playstation` | same as `ds4`                   | 8          | ViGEmBus cannot emulate a DualSense    |
//! | `switch`      | XInput with Nintendo layout, or raw Switch Pro HID report for a HID backend | 16 / 12 | |
//! | `gtouch`      | touch contacts (GameSir G-Touch style) | n/a  | Finger emulation for touch-only games  |

pub mod ds4;
pub mod switch;
pub mod xinput;

use crate::pad::PadState;
use crate::quantize::Resolution;
use crate::touch::TouchContact;

/// Which face-button labels the profile's a/b/x/y mean in Switch mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwitchLayout {
    /// Keep physical positions: profile "a" (south) = Nintendo B. Muscle memory from Xbox.
    Positional,
    /// Keep labels: profile "a" = Nintendo A (east). Swaps south<->east and west<->north.
    Labels,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SwitchTransport {
    /// Send through the Xbox 360 target (works on Windows with ViGEmBus today).
    XInput,
    /// Build real Switch Pro 0x30 input reports (12-bit sticks) for a HID backend
    /// (Linux uhid / USB gadget / custom driver). No ViGEmBus target exists for this.
    RawHid,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OutputMode {
    XInput,
    DualShock4,
    PlayStation,
    Switch {
        layout: SwitchLayout,
        transport: SwitchTransport,
    },
    GTouch,
}

impl OutputMode {
    pub const ALL_NAMES: &'static [&'static str] = &[
        "xinput",
        "ds4",
        "playstation",
        "switch",
        "switch-hid",
        "gtouch",
    ];

    pub fn parse(s: &str) -> Option<Self> {
        let n: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        Some(match n.as_str() {
            "xinput" | "xbox" | "xbox360" | "x360" => Self::XInput,
            "ds4" | "dualshock4" | "dualshock" => Self::DualShock4,
            "playstation" | "ps" | "ps4" | "ps5" => Self::PlayStation,
            "switch" | "switchpro" | "nintendo" => Self::Switch {
                layout: SwitchLayout::Positional,
                transport: SwitchTransport::XInput,
            },
            "switchhid" | "switchraw" => Self::Switch {
                layout: SwitchLayout::Positional,
                transport: SwitchTransport::RawHid,
            },
            "gtouch" | "gamesirgtouch" | "gamesir" | "touch" => Self::GTouch,
            _ => return None,
        })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::XInput => "Xbox 360 (XInput)",
            Self::DualShock4 => "DualShock 4",
            Self::PlayStation => "PlayStation (DualShock 4)",
            Self::Switch {
                transport: SwitchTransport::XInput,
                ..
            } => "Switch layout (via XInput)",
            Self::Switch {
                transport: SwitchTransport::RawHid,
                ..
            } => "Switch Pro (raw HID)",
            Self::GTouch => "GameSir G-Touch (touch)",
        }
    }

    /// Stick resolution on the wire. The mouse quantizer must use this.
    pub fn resolution(&self) -> Resolution {
        match self {
            Self::XInput | Self::GTouch => Resolution::XINPUT,
            Self::DualShock4 | Self::PlayStation => Resolution::DS4,
            Self::Switch {
                transport: SwitchTransport::XInput,
                ..
            } => Resolution::XINPUT,
            Self::Switch {
                transport: SwitchTransport::RawHid,
                ..
            } => Resolution::SWITCH,
        }
    }

    /// Encode one frame. G-Touch needs the stateful [`crate::touch::TouchEmulator`];
    /// pass its contacts in `touches` (ignored by other modes except DS4, whose first
    /// two contacts go to the touchpad in the full report).
    pub fn encode(&self, pad: &PadState, touches: &[TouchContact], frame: u32) -> Report {
        match *self {
            Self::XInput => Report::XUsb(xinput::encode(pad)),
            Self::DualShock4 | Self::PlayStation => Report::Ds4(ds4::encode(pad)),
            Self::Switch { layout, transport } => {
                let p = switch::apply_layout(pad, layout);
                match transport {
                    SwitchTransport::XInput => Report::XUsb(xinput::encode(&p)),
                    SwitchTransport::RawHid => Report::SwitchHid(switch::encode_0x30(&p, frame)),
                }
            }
            Self::GTouch => Report::Touch(touches.to_vec()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Report {
    XUsb(xinput::XUsbReport),
    Ds4(ds4::Ds4Report),
    SwitchHid([u8; 64]),
    Touch(Vec<TouchContact>),
}

#[inline]
pub(crate) fn trig_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[inline]
pub(crate) fn digital_trigger(pad: &PadState, bit: u32, v: f32) -> bool {
    pad.buttons.has(bit) || v > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_names() {
        for n in OutputMode::ALL_NAMES {
            assert!(OutputMode::parse(n).is_some(), "{n}");
        }
        assert_eq!(
            OutputMode::parse("GameSir G-Touch"),
            Some(OutputMode::GTouch)
        );
        assert_eq!(OutputMode::parse("PS5"), Some(OutputMode::PlayStation));
        assert_eq!(OutputMode::PlayStation.resolution(), Resolution::DS4);
    }
}
