//! Ties the pieces together: one call per output tick.

use crate::curve::GameCurve;
use crate::keyboard::{DigitalStick, DigitalStickConfig, Dirs};
use crate::modes::{OutputMode, Report};
use crate::mouse::{MouseConfig, MouseToStick};
use crate::pad::{Buttons, PadState};
use crate::touch::{TouchConfig, TouchEmulator};

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub mode: OutputMode,
    pub mouse: MouseConfig,
    /// The game's right-stick deadzone/curve (measure with the deadzone finder).
    pub curve: GameCurve,
    pub left_stick: DigitalStickConfig,
    pub touch: TouchConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            mode: OutputMode::XInput,
            mouse: MouseConfig::default(),
            curve: GameCurve::default(),
            left_stick: DigitalStickConfig::default(),
            touch: TouchConfig::default(),
        }
    }
}

/// Everything read from keyboard and mouse since the last tick (already mapped).
#[derive(Copy, Clone, Debug, Default)]
pub struct InputFrame {
    pub mouse_dx: i32,
    pub mouse_dy: i32,
    pub buttons: Buttons,
    pub move_dirs: Dirs,
    pub lt: f32,
    pub rt: f32,
}

pub struct Engine {
    mode: OutputMode,
    pub mouse: MouseToStick,
    pub left: DigitalStick,
    pub touch: TouchEmulator,
    frame: u32,
}

impl Engine {
    pub fn new(cfg: EngineConfig) -> Self {
        Self {
            mode: cfg.mode,
            mouse: MouseToStick::new(cfg.mouse, cfg.curve, cfg.mode.resolution()),
            left: DigitalStick::new(cfg.left_stick),
            touch: TouchEmulator::new(cfg.touch),
            frame: 0,
        }
    }

    pub fn mode(&self) -> OutputMode {
        self.mode
    }

    /// Switch controller type at runtime (the backend must re-plug its target).
    pub fn set_mode(&mut self, mode: OutputMode) {
        self.mode = mode;
        self.mouse.set_resolution(mode.resolution());
        self.mouse.reset();
    }

    pub fn tick(&mut self, input: &InputFrame, dt: f64) -> (PadState, Report) {
        self.frame = self.frame.wrapping_add(1);
        let (lx, ly) = self.left.tick(input.move_dirs, dt as f32);
        let mut pad = PadState {
            lx,
            ly,
            rx: 0.0,
            ry: 0.0,
            lt: input.lt,
            rt: input.rt,
            buttons: input.buttons,
        };

        let touches = if self.mode == OutputMode::GTouch {
            self.touch.push_mouse(input.mouse_dx, input.mouse_dy);
            self.touch.tick(&pad, dt as f32)
        } else {
            self.mouse.push(input.mouse_dx, input.mouse_dy);
            let q = self.mouse.tick(dt);
            pad.rx = q.raw.0;
            pad.ry = q.raw.1;
            Vec::new()
        };
        let report = self.mode.encode(&pad, &touches, self.frame);
        (pad, report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modes::{SwitchLayout, SwitchTransport};

    #[test]
    fn every_mode_produces_its_report() {
        for mode in [
            OutputMode::XInput,
            OutputMode::DualShock4,
            OutputMode::PlayStation,
            OutputMode::Switch {
                layout: SwitchLayout::Labels,
                transport: SwitchTransport::RawHid,
            },
            OutputMode::Switch {
                layout: SwitchLayout::Positional,
                transport: SwitchTransport::XInput,
            },
            OutputMode::GTouch,
        ] {
            let mut e = Engine::new(EngineConfig {
                mode,
                ..Default::default()
            });
            let f = InputFrame {
                mouse_dx: 40,
                ..Default::default()
            };
            let (pad, rep) = e.tick(&f, 0.002);
            match (mode, rep) {
                (OutputMode::GTouch, Report::Touch(t)) => assert_eq!(t.len(), 1),
                (OutputMode::GTouch, r) => panic!("{r:?}"),
                (_, Report::Touch(_)) => panic!(),
                _ => assert!(pad.rx > 0.0, "{mode:?}"),
            }
        }
    }
}
