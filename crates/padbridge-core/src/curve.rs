//! Model of what a GAME does to the stick (deadzone + response curve + saturation),
//! and its exact inverse. padbridge inverts the curve so that a mouse movement of N
//! counts/s gives a camera speed proportional to N, instead of being crushed by the
//! game's deadzone (small movements lost) or bent by its exponent (slow aim crawls).

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeadzoneShape {
    /// Deadzone and curve act on the stick's length (most modern games).
    Radial,
    /// Deadzone and curve act on each axis separately (many older / UE3-era games).
    Axial,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GameCurve {
    /// Raw stick length below which the game ignores input. XInput's documented right
    /// stick deadzone is 8689/32767 = 0.265; many games use 0.1..0.25.
    pub inner: f32,
    /// Raw stick length at which the game reaches its top speed (1.0 = edge; some games
    /// saturate earlier, e.g. 0.95).
    pub outer: f32,
    /// Exponent of the game's response curve after the deadzone (1 = linear, ~2 common).
    pub exponent: f32,
    pub shape: DeadzoneShape,
}

impl Default for GameCurve {
    fn default() -> Self {
        Self {
            inner: 0.0,
            outer: 1.0,
            exponent: 1.0,
            shape: DeadzoneShape::Radial,
        }
    }
}

impl GameCurve {
    pub fn linear() -> Self {
        Self::default()
    }

    fn fwd1(&self, a: f32) -> f32 {
        let span = (self.outer - self.inner).max(1e-6);
        let t = ((a.abs() - self.inner) / span).clamp(0.0, 1.0);
        t.powf(self.exponent.max(1e-3)).copysign(a) * (t > 0.0) as i32 as f32
    }

    fn inv1(&self, e: f32) -> f32 {
        if e == 0.0 {
            return 0.0;
        }
        let m = e.abs().min(1.0);
        let t = m.powf(1.0 / self.exponent.max(1e-3));
        (self.inner + t * (self.outer - self.inner))
            .min(1.0)
            .copysign(e)
    }

    /// Raw stick → effective output the game acts on (each component in [-1, 1],
    /// length ≤ 1 for radial).
    pub fn forward(&self, x: f32, y: f32) -> (f32, f32) {
        match self.shape {
            DeadzoneShape::Axial => (self.fwd1(x), self.fwd1(y)),
            DeadzoneShape::Radial => {
                let r = x.hypot(y);
                if r <= 0.0 {
                    return (0.0, 0.0);
                }
                let m = self.fwd1(r.min(1.0));
                (x / r * m, y / r * m)
            }
        }
    }

    /// Effective output wanted → raw stick to send. Lengths above 1 are clamped
    /// (direction kept). Exact inverse of [`GameCurve::forward`] inside its range.
    pub fn inverse(&self, ex: f32, ey: f32) -> (f32, f32) {
        match self.shape {
            DeadzoneShape::Axial => (self.inv1(ex), self.inv1(ey)),
            DeadzoneShape::Radial => {
                let m = ex.hypot(ey);
                if m <= 0.0 {
                    return (0.0, 0.0);
                }
                let r = self.inv1(m.min(1.0));
                (ex / m * r, ey / m * r)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for shape in [DeadzoneShape::Radial, DeadzoneShape::Axial] {
            let c = GameCurve {
                inner: 0.2,
                outer: 0.95,
                exponent: 2.0,
                shape,
            };
            for &(x, y) in &[(0.3f32, 0.1f32), (-0.05, 0.6), (0.7, -0.7), (0.001, 0.0)] {
                let (rx, ry) = c.inverse(x, y);
                let (fx, fy) = c.forward(rx, ry);
                let (ex, ey) = if shape == DeadzoneShape::Radial && x.hypot(y) > 1.0 {
                    (x / x.hypot(y), y / x.hypot(y))
                } else {
                    (x, y)
                };
                assert!(
                    (fx - ex).abs() < 1e-4 && (fy - ey).abs() < 1e-4,
                    "{shape:?} {x},{y} -> {fx},{fy}"
                );
            }
        }
    }
    #[test]
    fn deadzone_kills_small_raw() {
        let c = GameCurve {
            inner: 0.25,
            ..Default::default()
        };
        assert_eq!(c.forward(0.2, 0.0), (0.0, 0.0));
        assert!(c.inverse(0.01, 0.0).0 > 0.25);
    }
}
