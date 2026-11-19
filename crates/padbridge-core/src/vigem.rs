//! Windows output through ViGEmBus (feature `vigem`).
//!
//! Written against vigem-client 0.1.4 with the `unstable_ds4` feature.
//! Supported: XInput, Switch (XInput transport), DS4 / PlayStation.
//! Not supported by ViGEmBus: DualSense, Switch Pro HID, touch injection (G-Touch).

use crate::modes::{OutputMode, Report, SwitchTransport};
use vigem_client::{
    Client, DS4Report, DualShock4Wired, TargetId, XButtons, XGamepad, Xbox360Wired,
};

pub enum VigemPad {
    X360(Xbox360Wired<Client>),
    Ds4(DualShock4Wired<Client>),
}

pub struct VigemBackend {
    pad: VigemPad,
}

impl VigemBackend {
    /// Plug in the virtual controller for `mode`. `ids` overrides the USB vendor/product
    /// IDs the virtual device reports (e.g. to look like a specific third-party pad).
    pub fn connect(mode: OutputMode, ids: Option<(u16, u16)>) -> Result<Self, String> {
        let client = Client::connect().map_err(|e| {
            format!("cannot open the ViGEmBus driver ({e:?}). Is ViGEmBus installed?")
        })?;
        let pad = match mode {
            OutputMode::XInput
            | OutputMode::Switch {
                transport: SwitchTransport::XInput,
                ..
            } => {
                let id = ids
                    .map(|(vendor, product)| TargetId { vendor, product })
                    .unwrap_or(TargetId::XBOX360_WIRED);
                let mut t = Xbox360Wired::new(client, id);
                t.plugin().map_err(|e| format!("plugin failed: {e:?}"))?;
                t.wait_ready()
                    .map_err(|e| format!("wait_ready failed: {e:?}"))?;
                VigemPad::X360(t)
            }
            OutputMode::DualShock4 | OutputMode::PlayStation => {
                let id = ids
                    .map(|(vendor, product)| TargetId { vendor, product })
                    .unwrap_or(TargetId::DUALSHOCK4_WIRED);
                let mut t = DualShock4Wired::new(client, id);
                t.plugin().map_err(|e| format!("plugin failed: {e:?}"))?;
                t.wait_ready()
                    .map_err(|e| format!("wait_ready failed: {e:?}"))?;
                VigemPad::Ds4(t)
            }
            OutputMode::Switch {
                transport: SwitchTransport::RawHid,
                ..
            } => {
                return Err("Switch Pro raw HID needs a HID backend; ViGEmBus has no Switch target. Use mode 'switch' (XInput transport) on Windows.".into())
            }
            OutputMode::GTouch => {
                return Err("G-Touch mode outputs touch contacts; use a touch-injection backend, not ViGEmBus.".into())
            }
        };
        Ok(Self { pad })
    }

    pub fn send(&mut self, report: &Report) -> Result<(), String> {
        match (&mut self.pad, report) {
            (VigemPad::X360(t), Report::XUsb(r)) => {
                let g = XGamepad {
                    buttons: XButtons { raw: r.buttons },
                    left_trigger: r.left_trigger,
                    right_trigger: r.right_trigger,
                    thumb_lx: r.thumb_lx,
                    thumb_ly: r.thumb_ly,
                    thumb_rx: r.thumb_rx,
                    thumb_ry: r.thumb_ry,
                };
                t.update(&g).map_err(|e| format!("{e:?}"))
            }
            (VigemPad::Ds4(t), Report::Ds4(r)) => {
                let d = DS4Report {
                    thumb_lx: r.thumb_lx,
                    thumb_ly: r.thumb_ly,
                    thumb_rx: r.thumb_rx,
                    thumb_ry: r.thumb_ry,
                    buttons: r.buttons,
                    special: r.special,
                    trigger_l: r.trigger_l,
                    trigger_r: r.trigger_r,
                };
                t.update(&d).map_err(|e| format!("{e:?}"))
            }
            _ => Err("report does not match the plugged-in controller type (re-connect after changing mode)".into()),
        }
    }
}
