//! GameSir "G-Touch"-style output: the controller acts as FINGERS on a touch screen.
//!
//! GameSir's G-Touch mode (G5, G6, G4 Pro, X2, X3 …) makes the controller send system
//! touch events, simulating finger touches at fixed screen positions, so touch-only
//! mobile games can be played. padbridge reproduces that from keyboard and mouse:
//!
//! * buttons press a finger at a configured screen point;
//! * the left stick drives a finger around a virtual-joystick centre;
//! * the mouse drags an "aim" finger, pixel-accurate: sub-pixel motion is carried, and
//!   when the finger nears the edge of its region it is lifted and put back at the
//!   anchor WITHOUT losing the movement that was still owed.
//!
//! The output is a list of contacts in screen pixels for a touch-injection backend
//! (Windows `InjectSyntheticPointerInput`, Android uinput/scrcpy, …). The first two
//! contacts can also be sent through the DS4 touchpad (see `modes::ds4`).

use crate::pad::PadState;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TouchContact {
    /// Stable id while the finger is down.
    pub id: u8,
    pub x: f32,
    pub y: f32,
    pub down: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TouchButton {
    /// Any of these `Buttons` bits presses this finger.
    pub mask: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TouchConfig {
    pub screen_w: f32,
    pub screen_h: f32,
    pub buttons: Vec<TouchButton>,
    /// Virtual joystick for the left stick: centre and radius (px). radius 0 = off.
    pub joy_center: (f32, f32),
    pub joy_radius: f32,
    /// Aim drag: anchor point, how far the finger may travel before re-anchoring (px),
    /// and screen pixels per mouse count (the game's touch sensitivity calibration).
    pub aim_anchor: (f32, f32),
    pub aim_radius: f32,
    pub px_per_count: f64,
    /// Lift the aim finger after this long without mouse movement (ms). 0 = never.
    pub aim_idle_lift_ms: f32,
}

impl Default for TouchConfig {
    fn default() -> Self {
        Self {
            screen_w: 1920.0,
            screen_h: 1080.0,
            buttons: Vec::new(),
            joy_center: (300.0, 780.0),
            joy_radius: 150.0,
            aim_anchor: (1400.0, 540.0),
            aim_radius: 300.0,
            px_per_count: 0.5,
            aim_idle_lift_ms: 400.0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum Aim {
    Up,
    /// Finger down at `pos`.
    Down {
        pos: (f64, f64),
    },
    /// Lifted for one frame before re-anchoring.
    Relifting,
}

#[derive(Clone, Debug)]
pub struct TouchEmulator {
    pub cfg: TouchConfig,
    aim: Aim,
    owed_px: (f64, f64),
    idle_ms: f32,
    /// Movement delivered to the game (px), for accuracy checks.
    pub delivered_px: (f64, f64),
}

const AIM_ID: u8 = 0;
const JOY_ID: u8 = 1;
const BTN_ID0: u8 = 2;

impl TouchEmulator {
    pub fn new(cfg: TouchConfig) -> Self {
        Self {
            cfg,
            aim: Aim::Up,
            owed_px: (0.0, 0.0),
            idle_ms: 0.0,
            delivered_px: (0.0, 0.0),
        }
    }

    /// Raw mouse delta (+y = down, same as screen).
    pub fn push_mouse(&mut self, dx: i32, dy: i32) {
        self.owed_px.0 += dx as f64 * self.cfg.px_per_count;
        self.owed_px.1 += dy as f64 * self.cfg.px_per_count;
        if dx != 0 || dy != 0 {
            self.idle_ms = 0.0;
        }
    }

    /// Advance one frame. Returns every finger that is down, plus fingers lifting this
    /// frame (`down = false`), so a backend can send the matching "up" events.
    pub fn tick(&mut self, pad: &PadState, dt: f32) -> Vec<TouchContact> {
        let mut out = Vec::new();
        self.tick_aim(dt, &mut out);

        if self.cfg.joy_radius > 0.0 {
            let (x, y) = (pad.lx, pad.ly);
            let down = x != 0.0 || y != 0.0;
            if down {
                out.push(TouchContact {
                    id: JOY_ID,
                    x: self.cfg.joy_center.0 + x * self.cfg.joy_radius,
                    y: self.cfg.joy_center.1 - y * self.cfg.joy_radius,
                    down: true,
                });
            }
        }
        for (i, b) in self.cfg.buttons.iter().enumerate() {
            if pad.buttons.0 & b.mask != 0 {
                out.push(TouchContact {
                    id: BTN_ID0 + i as u8,
                    x: b.x,
                    y: b.y,
                    down: true,
                });
            }
        }
        out
    }

    fn clamp_screen(&self, p: (f64, f64)) -> (f64, f64) {
        (
            p.0.clamp(0.0, self.cfg.screen_w as f64 - 1.0),
            p.1.clamp(0.0, self.cfg.screen_h as f64 - 1.0),
        )
    }

    fn tick_aim(&mut self, dt: f32, out: &mut Vec<TouchContact>) {
        let anchor = (self.cfg.aim_anchor.0 as f64, self.cfg.aim_anchor.1 as f64);
        let moving = self.owed_px.0.abs() >= 0.5 || self.owed_px.1.abs() >= 0.5;
        self.idle_ms += dt * 1000.0;
        match self.aim {
            Aim::Up | Aim::Relifting => {
                if moving {
                    // Finger goes down at the anchor; movement starts next frame so the
                    // game registers the touch-down before any drag.
                    self.aim = Aim::Down { pos: anchor };
                    out.push(TouchContact {
                        id: AIM_ID,
                        x: anchor.0 as f32,
                        y: anchor.1 as f32,
                        down: true,
                    });
                } else {
                    self.aim = Aim::Up;
                }
            }
            Aim::Down { pos } => {
                let idle_lift =
                    self.cfg.aim_idle_lift_ms > 0.0 && self.idle_ms >= self.cfg.aim_idle_lift_ms;
                // Deliver whole pixels (nearest); the fraction stays owed for the next frame.
                let mx = self.owed_px.0.round();
                let my = self.owed_px.1.round();
                let want = (pos.0 + mx, pos.1 + my);
                let r = self.cfg.aim_radius as f64;
                let off = (want.0 - anchor.0, want.1 - anchor.1);
                let dist = off.0.hypot(off.1);
                let (np, lift) = if dist > r {
                    // Move only to the region edge, keep the rest owed, then re-anchor.
                    let k = r / dist;
                    let edge = (anchor.0 + off.0 * k, anchor.1 + off.1 * k);
                    ((edge.0.round(), edge.1.round()), true)
                } else {
                    (want, idle_lift)
                };
                let np = self.clamp_screen(np);
                let moved = (np.0 - pos.0, np.1 - pos.1);
                self.owed_px.0 -= moved.0;
                self.owed_px.1 -= moved.1;
                self.delivered_px.0 += moved.0;
                self.delivered_px.1 += moved.1;
                if lift {
                    out.push(TouchContact {
                        id: AIM_ID,
                        x: np.0 as f32,
                        y: np.1 as f32,
                        down: false,
                    });
                    self.aim = Aim::Relifting;
                } else {
                    self.aim = Aim::Down { pos: np };
                    out.push(TouchContact {
                        id: AIM_ID,
                        x: np.0 as f32,
                        y: np.1 as f32,
                        down: true,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::Buttons;

    #[test]
    fn long_drag_is_lossless_across_reanchors() {
        let mut t = TouchEmulator::new(TouchConfig {
            px_per_count: 0.37,
            aim_idle_lift_ms: 0.0,
            ..Default::default()
        });
        let pad = PadState::neutral();
        let mut lifts = 0;
        for i in 0..2000 {
            if i < 1500 {
                t.push_mouse(7, -2);
            }
            for c in t.tick(&pad, 0.004) {
                if c.id == 0 && !c.down {
                    lifts += 1;
                }
            }
        }
        let want = (1500.0 * 7.0 * 0.37, -1500.0 * 2.0 * 0.37);
        assert!(lifts > 5);
        assert!(
            (t.delivered_px.0 - want.0).abs() < 1.0,
            "{:?} vs {:?}",
            t.delivered_px,
            want
        );
        assert!((t.delivered_px.1 - want.1).abs() < 1.0);
    }

    #[test]
    fn buttons_and_joystick() {
        let cfg = TouchConfig {
            buttons: vec![TouchButton {
                mask: Buttons::SOUTH,
                x: 1700.0,
                y: 900.0,
            }],
            ..Default::default()
        };
        let mut t = TouchEmulator::new(cfg);
        let mut pad = PadState::neutral();
        pad.buttons.set(Buttons::SOUTH, true);
        pad.lx = 1.0;
        let c = t.tick(&pad, 0.004);
        assert!(c.iter().any(|c| c.id == 1 && c.x == 450.0));
        assert!(c.iter().any(|c| c.id == 2 && c.x == 1700.0));
    }
}
