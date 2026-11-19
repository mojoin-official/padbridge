//! Keys → analog stick (usually WASD → left stick).

/// What happens when opposite directions are held together (e.g. A and D).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SocdMode {
    /// The most recently pressed direction wins (best for movement in most games).
    LastWins,
    /// The first pressed direction wins.
    FirstWins,
    /// Opposites cancel to centre.
    Neutral,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DigitalStickConfig {
    pub socd: SocdMode,
    /// true: diagonals are a full-length circle like a real stick (W+D = 0.707, 0.707).
    /// false: W+D = (1, 1) (the game clamps it).
    pub circular: bool,
    /// Time from centre to full, in ms. 0 = instant.
    pub ramp_ms: f32,
    /// Stick length while the Walk input is held.
    pub walk_scale: f32,
}

impl Default for DigitalStickConfig {
    fn default() -> Self {
        Self {
            socd: SocdMode::LastWins,
            circular: true,
            ramp_ms: 0.0,
            walk_scale: 0.5,
        }
    }
}

#[derive(Copy, Clone, Debug, Default)]
struct Axis {
    neg_t: Option<u64>,
    pos_t: Option<u64>,
}

impl Axis {
    fn update(&mut self, neg: bool, pos: bool, now: u64) {
        self.neg_t = if neg { self.neg_t.or(Some(now)) } else { None };
        self.pos_t = if pos { self.pos_t.or(Some(now)) } else { None };
    }
    fn value(&self, mode: SocdMode) -> f32 {
        match (self.neg_t, self.pos_t) {
            (None, None) => 0.0,
            (Some(_), None) => -1.0,
            (None, Some(_)) => 1.0,
            (Some(n), Some(p)) => match mode {
                SocdMode::Neutral => 0.0,
                // Equal timestamps (pressed in the same tick) fall back to neutral.
                SocdMode::LastWins => (p.cmp(&n) as i32) as f32,
                SocdMode::FirstWins => (n.cmp(&p) as i32) as f32,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct DigitalStick {
    pub cfg: DigitalStickConfig,
    x: Axis,
    y: Axis,
    tick: u64,
    cur: (f32, f32),
}

/// Directions held this tick.
#[derive(Copy, Clone, Debug, Default)]
pub struct Dirs {
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
    pub walk: bool,
}

impl DigitalStick {
    pub fn new(cfg: DigitalStickConfig) -> Self {
        Self {
            cfg,
            x: Axis::default(),
            y: Axis::default(),
            tick: 0,
            cur: (0.0, 0.0),
        }
    }

    /// Advance by `dt` seconds; returns raw stick (x, y), +y = up.
    pub fn tick(&mut self, d: Dirs, dt: f32) -> (f32, f32) {
        self.tick += 1;
        self.x.update(d.left, d.right, self.tick);
        self.y.update(d.down, d.up, self.tick);
        let (mut tx, mut ty) = (self.x.value(self.cfg.socd), self.y.value(self.cfg.socd));
        if self.cfg.circular {
            let m = tx.hypot(ty);
            if m > 1.0 {
                tx /= m;
                ty /= m;
            }
        }
        if d.walk {
            tx *= self.cfg.walk_scale;
            ty *= self.cfg.walk_scale;
        }
        if self.cfg.ramp_ms <= 0.0 {
            self.cur = (tx, ty);
        } else {
            let step = dt * 1000.0 / self.cfg.ramp_ms;
            let (dx, dy) = (tx - self.cur.0, ty - self.cur.1);
            let dist = dx.hypot(dy);
            if dist <= step {
                self.cur = (tx, ty);
            } else {
                self.cur.0 += dx / dist * step;
                self.cur.1 += dy / dist * step;
            }
        }
        self.cur
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn last_wins() {
        let mut s = DigitalStick::new(DigitalStickConfig::default());
        s.tick(
            Dirs {
                left: true,
                ..Default::default()
            },
            0.002,
        );
        let v = s.tick(
            Dirs {
                left: true,
                right: true,
                ..Default::default()
            },
            0.002,
        );
        assert_eq!(v.0, 1.0);
    }
    #[test]
    fn neutral_and_circle() {
        let mut s = DigitalStick::new(DigitalStickConfig {
            socd: SocdMode::Neutral,
            ..Default::default()
        });
        let v = s.tick(
            Dirs {
                left: true,
                right: true,
                up: true,
                ..Default::default()
            },
            0.002,
        );
        assert_eq!(v, (0.0, 1.0));
        let v = s.tick(
            Dirs {
                up: true,
                right: true,
                ..Default::default()
            },
            0.002,
        );
        assert!((v.0 - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
        assert!((v.1 - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5);
    }
    #[test]
    fn ramp() {
        let mut s = DigitalStick::new(DigitalStickConfig {
            ramp_ms: 100.0,
            ..Default::default()
        });
        let v = s.tick(
            Dirs {
                right: true,
                ..Default::default()
            },
            0.05,
        );
        assert!((v.0 - 0.5).abs() < 1e-5);
        let v = s.tick(
            Dirs {
                right: true,
                ..Default::default()
            },
            0.1,
        );
        assert_eq!(v.0, 1.0);
    }
}
