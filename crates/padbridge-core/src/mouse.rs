//! Mouse → right stick, displacement-accurate.
//!
//! A mouse reports how FAR it moved; a stick tells the game how FAST to turn. The old
//! approach ("velocity over a window, clamp to full deflection") loses rotation in three
//! places: anything faster than the game's top stick speed is cut off, movements that
//! land inside the game's deadzone vanish, and stick rounding (8-bit on DS4) biases every
//! sample. Here every mouse count goes into a ledger (`owed`). Each tick releases part of
//! the ledger as a stick rate, quantizes it as the GAME will see it, and subtracts only
//! what was actually delivered. Nothing is lost unless the ledger exceeds `max_carry_ms`
//! of full-speed turning (a deliberate cap so a huge flick does not keep spinning).

use crate::curve::GameCurve;
use crate::quantize::{Quantized, Resolution, StickQuantizer};

/// How mouse counts relate to the game's camera.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Calibration {
    /// Mouse counts for a 360° turn at your normal mouse sensitivity in this game.
    pub counts_per_360: f64,
    /// The game's top turn speed (degrees/s) with the stick fully deflected, at the
    /// game's controller sensitivity you will use (set it to maximum).
    pub max_turn_deg_s: f64,
}

impl Calibration {
    /// Build from a turn test: hold the stick fully over and time one full turn.
    pub fn from_turn_test(counts_per_360: f64, seconds_per_turn_at_full: f64) -> Self {
        Self {
            counts_per_360,
            max_turn_deg_s: 360.0 / seconds_per_turn_at_full.max(1e-6),
        }
    }
    /// Mouse speed (counts/s) that equals a fully held stick.
    pub fn full_deflection_cps(&self) -> f64 {
        self.counts_per_360 * self.max_turn_deg_s / 360.0
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MouseConfig {
    /// Mouse counts/s that equal full effective deflection (see [`Calibration`]).
    pub full_deflection_cps: f64,
    /// Vertical counts are multiplied by this (match the game's pitch/yaw speed ratio).
    pub y_ratio: f64,
    pub invert_y: bool,
    /// Owed movement is released with this time constant. Should be at least one game
    /// frame (games sample the stick once per frame). 8–16 ms is a good range.
    pub release_ms: f64,
    /// Ledger cap, in milliseconds of full-speed turning. Movement beyond it is dropped
    /// (counted in [`MouseStats::dropped_counts`]). 0 = drop everything over the limit
    /// immediately (old behaviour).
    pub max_carry_ms: f64,
}

impl MouseConfig {
    /// Recommended `release_ms` for a game running at `fps`: about one frame, so the
    /// stick holds steady across each game sample. In the simulator (DS4, dz 0.2, exp 2)
    /// one frame gives ~1% flick/tracking error; 1.5-2 frames halves that again at the
    /// cost of a little more smoothing. Below half a frame accuracy drops sharply.
    pub fn release_ms_for_fps(fps: f64) -> f64 {
        (1000.0 / fps.max(1.0)).clamp(6.0, 40.0)
    }
}

impl Default for MouseConfig {
    fn default() -> Self {
        Self {
            full_deflection_cps: 4000.0,
            y_ratio: 1.0,
            invert_y: false,
            release_ms: 12.0,
            max_carry_ms: 250.0,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct MouseStats {
    pub received_counts: (f64, f64),
    pub delivered_counts: (f64, f64),
    pub dropped_counts: f64,
    /// Ticks where the wanted speed exceeded the stick's top speed.
    pub saturated_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct MouseToStick {
    pub cfg: MouseConfig,
    pub curve: GameCurve,
    quant: StickQuantizer,
    owed: (f64, f64),
    pub stats: MouseStats,
}

impl MouseToStick {
    pub fn new(cfg: MouseConfig, curve: GameCurve, res: Resolution) -> Self {
        Self {
            cfg,
            curve,
            quant: StickQuantizer::new(res),
            owed: (0.0, 0.0),
            stats: MouseStats::default(),
        }
    }

    pub fn set_resolution(&mut self, res: Resolution) {
        self.quant = StickQuantizer::new(res);
    }

    /// Raw mouse delta (WM_INPUT / RAWMOUSE lLastX, lLastY; +y = mouse moved down).
    pub fn push(&mut self, dx: i32, dy: i32) {
        let sy = if self.cfg.invert_y { 1.0 } else { -1.0 };
        let x = dx as f64;
        let y = dy as f64 * self.cfg.y_ratio * sy;
        self.owed.0 += x;
        self.owed.1 += y;
        self.stats.received_counts.0 += x;
        self.stats.received_counts.1 += y;
    }

    /// Counts still waiting to be delivered (x, y-stick-up).
    pub fn owed(&self) -> (f64, f64) {
        self.owed
    }

    /// Drop anything owed (e.g. when capture is paused).
    pub fn reset(&mut self) {
        self.owed = (0.0, 0.0);
    }

    /// Advance one output tick of `dt` seconds. Returns the stick to send.
    pub fn tick(&mut self, dt: f64) -> Quantized {
        let full = self.cfg.full_deflection_cps.max(1e-9);
        let tau = (self.cfg.release_ms / 1000.0).max(dt).max(1e-6);
        let (mut ex, mut ey) = (self.owed.0 / tau / full, self.owed.1 / tau / full);
        let m = ex.hypot(ey);
        if m > 1.0 {
            ex /= m;
            ey /= m;
            self.stats.saturated_ticks += 1;
        }
        let q = self.quant.quantize(&self.curve, ex as f32, ey as f32);
        let dx = q.achieved.0 as f64 * full * dt;
        let dy = q.achieved.1 as f64 * full * dt;
        self.owed.0 -= dx;
        self.owed.1 -= dy;
        self.stats.delivered_counts.0 += dx;
        self.stats.delivered_counts.1 += dy;

        let cap = full * self.cfg.max_carry_ms / 1000.0;
        let o = self.owed.0.hypot(self.owed.1);
        if o > cap && o > 0.0 {
            let k = cap / o;
            self.stats.dropped_counts += o - cap;
            self.owed.0 *= k;
            self.owed.1 *= k;
        }
        q
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::DeadzoneShape;

    fn run(m: &mut MouseToStick, ticks: usize, dt: f64) {
        for _ in 0..ticks {
            m.tick(dt);
        }
    }

    #[test]
    fn release_for_fps() {
        assert!((MouseConfig::release_ms_for_fps(60.0) - 16.666).abs() < 0.01);
        assert_eq!(MouseConfig::release_ms_for_fps(1000.0), 6.0);
    }

    #[test]
    fn calibration() {
        let c = Calibration::from_turn_test(8000.0, 2.0);
        assert!((c.full_deflection_cps() - 4000.0).abs() < 1e-9);
    }

    #[test]
    fn conserves_displacement_with_deadzone_and_ds4() {
        let curve = GameCurve {
            inner: 0.2,
            outer: 1.0,
            exponent: 2.0,
            shape: DeadzoneShape::Radial,
        };
        let mut m = MouseToStick::new(MouseConfig::default(), curve, Resolution::DS4);
        m.push(37, -11);
        run(&mut m, 2000, 0.002);
        let (ox, oy) = m.owed();
        // Left in the ledger: less than one smallest effective step's worth.
        assert!(ox.abs() < 2.0 && oy.abs() < 2.0, "owed {ox},{oy}");
    }

    #[test]
    fn flick_over_top_speed_is_carried() {
        let cfg = MouseConfig {
            full_deflection_cps: 1000.0,
            max_carry_ms: 1000.0,
            ..Default::default()
        };
        let mut m = MouseToStick::new(cfg, GameCurve::linear(), Resolution::XINPUT);
        m.push(500, 0); // 500 counts in one tick: 0.5 s of full stick
        run(&mut m, 400, 0.002); // 0.8 s
        assert!(
            (m.stats.delivered_counts.0 - 500.0).abs() < 1.0,
            "{:?}",
            m.stats
        );
        assert!(m.stats.saturated_ticks > 0);
    }

    #[test]
    fn cap_drops_excess() {
        let cfg = MouseConfig {
            full_deflection_cps: 1000.0,
            max_carry_ms: 100.0,
            ..Default::default()
        };
        let mut m = MouseToStick::new(cfg, GameCurve::linear(), Resolution::XINPUT);
        m.push(500, 0);
        run(&mut m, 500, 0.002);
        assert!(m.stats.dropped_counts > 300.0);
    }
}
