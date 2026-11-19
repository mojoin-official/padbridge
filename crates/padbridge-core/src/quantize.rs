//! Turning a wanted effective output into integer stick codes.
//!
//! Rounding the raw stick value is not enough: after the game's deadzone and curve, one
//! raw step can be a big effective step (especially with 8-bit DS4 sticks), and rounding
//! the wrong way inside the deadzone silently drops slow movement. The quantizer instead
//! searches the codes around the inverse-curve point and picks the one whose EFFECTIVE
//! result (as the game will see it) is closest. It reports the achieved effective value
//! so [`crate::mouse::MouseToStick`] can carry the remainder into the next tick
//! (sigma-delta), which makes the long-run average exact at any resolution.

use crate::curve::GameCurve;

/// Stick resolution of an emulated controller, as symmetric integer codes
/// `-half ..= half`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub half: i32,
}

impl Resolution {
    /// Xbox 360 / XInput: 16-bit signed.
    pub const XINPUT: Resolution = Resolution { half: 32767 };
    /// DualShock 4 (and PlayStation via DS4): 8-bit, centre 128.
    pub const DS4: Resolution = Resolution { half: 127 };
    /// Switch Pro: 12-bit, centre 2048.
    pub const SWITCH: Resolution = Resolution { half: 2047 };

    #[inline]
    pub fn to_norm(self, code: i32) -> f32 {
        code as f32 / self.half as f32
    }
    #[inline]
    pub fn to_code(self, v: f32) -> i32 {
        (v.clamp(-1.0, 1.0) * self.half as f32).round() as i32
    }
}

#[derive(Copy, Clone, Debug)]
pub struct StickQuantizer {
    pub res: Resolution,
}

/// Result of quantizing one stick.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Quantized {
    /// Integer codes, `-half..=half`, +y = up.
    pub code: (i32, i32),
    /// Normalized raw stick value (exactly `code / half`).
    pub raw: (f32, f32),
    /// Effective output the game will compute from `raw`.
    pub achieved: (f32, f32),
}

impl StickQuantizer {
    pub fn new(res: Resolution) -> Self {
        Self { res }
    }

    pub fn quantize(&self, curve: &GameCurve, ex: f32, ey: f32) -> Quantized {
        if ex == 0.0 && ey == 0.0 {
            return Quantized {
                code: (0, 0),
                raw: (0.0, 0.0),
                achieved: (0.0, 0.0),
            };
        }
        let h = self.res.half;
        let (rx, ry) = curve.inverse(ex, ey);
        let (fx, fy) = (rx * h as f32, ry * h as f32);
        let (bx, by) = (fx.floor() as i32, fy.floor() as i32);
        let mut best = (0i32, 0i32);
        let mut best_g = (0.0f32, 0.0f32);
        let mut best_d = f32::INFINITY;
        for cx in (bx - 1)..=(bx + 2) {
            for cy in (by - 1)..=(by + 2) {
                let (cx, cy) = (cx.clamp(-h, h), cy.clamp(-h, h));
                let g = curve.forward(self.res.to_norm(cx), self.res.to_norm(cy));
                let d = (g.0 - ex).powi(2) + (g.1 - ey).powi(2);
                // Tie-break toward the smaller code (less overshoot).
                if d < best_d - 1e-12
                    || ((d - best_d).abs() <= 1e-12
                        && cx.abs() + cy.abs() < best.0.abs() + best.1.abs())
                {
                    best_d = d;
                    best = (cx, cy);
                    best_g = g;
                }
            }
        }
        Quantized {
            code: best,
            raw: (self.res.to_norm(best.0), self.res.to_norm(best.1)),
            achieved: best_g,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::{DeadzoneShape, GameCurve};

    #[test]
    fn nearest_effective_beats_rounding_in_deadzone() {
        let c = GameCurve {
            inner: 0.2,
            outer: 1.0,
            exponent: 1.0,
            shape: DeadzoneShape::Radial,
        };
        let q = StickQuantizer::new(Resolution::DS4);
        let r = q.quantize(&c, 0.004, 0.0);
        // Must land outside the deadzone or at zero, whichever is closer in effect.
        assert!(r.achieved.0 >= 0.0 && (r.achieved.0 - 0.004).abs() <= 0.0062);
    }

    #[test]
    fn xinput_is_fine_grained() {
        let c = GameCurve {
            inner: 0.265,
            outer: 1.0,
            exponent: 2.0,
            shape: DeadzoneShape::Radial,
        };
        let q = StickQuantizer::new(Resolution::XINPUT);
        let r = q.quantize(&c, 0.3, -0.2);
        assert!((r.achieved.0 - 0.3).abs() < 1e-3 && (r.achieved.1 + 0.2).abs() < 1e-3);
    }
}
