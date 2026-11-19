//! Offline accuracy harness: a model game that samples the stick once per frame,
//! applies its deadzone/curve and turns the camera, driven by a synthetic mouse trace.
//! Used by tests and `cargo run --example accuracy` to compare the old "window
//! velocity" method with the displacement-conserving method, per controller mode.

use crate::curve::GameCurve;
use crate::mouse::{Calibration, MouseConfig, MouseToStick};
use crate::quantize::Resolution;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SegKind {
    Track,
    Flick,
    Micro,
    Pause,
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub kind: SegKind,
    pub start_ms: usize,
    pub end_ms: usize,
}

/// 1000 Hz mouse trace (dx, dy) plus its segments.
#[derive(Clone, Debug)]
pub struct Trace {
    pub samples: Vec<(i32, i32)>,
    pub segments: Vec<Segment>,
}

struct Rng(u64);
impl Rng {
    fn f(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.f()
    }
}

/// Deterministic mixed trace: slow tracking, fast flicks (up to 3x the stick's top
/// speed), 1–3 count micro-adjustments, pauses. Speeds are relative to `full_cps`.
pub fn make_trace(seed: u64, segments: usize, full_cps: f64) -> Trace {
    let mut r = Rng(seed ^ 0x9E3779B97F4A7C15);
    let mut samples = Vec::new();
    let mut segs = Vec::new();
    let mut carry = (0.0f64, 0.0f64);
    let mut emit = |samples: &mut Vec<(i32, i32)>, vx: f64, vy: f64| {
        carry.0 += vx / 1000.0;
        carry.1 += vy / 1000.0;
        let (ix, iy) = (carry.0.trunc(), carry.1.trunc());
        carry.0 -= ix;
        carry.1 -= iy;
        samples.push((ix as i32, iy as i32));
    };
    for _ in 0..segments {
        let start = samples.len();
        let pick = r.f();
        let kind = if pick < 0.4 {
            SegKind::Track
        } else if pick < 0.7 {
            SegKind::Flick
        } else if pick < 0.85 {
            SegKind::Micro
        } else {
            SegKind::Pause
        };
        let ang = r.range(0.0, std::f64::consts::TAU);
        let (cx, cy) = (ang.cos(), ang.sin() * 0.5);
        match kind {
            SegKind::Track => {
                let speed = r.range(0.03, 0.4) * full_cps;
                let n = r.range(300.0, 800.0) as usize;
                for i in 0..n {
                    let wob = 1.0 + 0.2 * (i as f64 * 0.02).sin();
                    emit(&mut samples, cx * speed * wob, cy * speed * wob);
                }
            }
            SegKind::Flick => {
                let peak = r.range(0.8, 3.0) * full_cps;
                let n = r.range(80.0, 160.0) as usize;
                for i in 0..n {
                    let s = (std::f64::consts::PI * (i as f64 + 0.5) / n as f64)
                        .sin()
                        .powi(2)
                        * peak;
                    emit(&mut samples, cx * s, cy * s);
                }
            }
            SegKind::Micro => {
                let n = r.range(150.0, 300.0) as usize;
                for i in 0..n {
                    if i % 37 == 0 {
                        let k =
                            r.range(1.0, 3.99).floor() as i32 * if r.f() < 0.5 { -1 } else { 1 };
                        samples.push((k, 0));
                    } else {
                        samples.push((0, 0));
                    }
                }
            }
            SegKind::Pause => {
                let n = r.range(100.0, 300.0) as usize;
                samples.extend(std::iter::repeat_n((0, 0), n));
            }
        }
        segs.push(Segment {
            kind,
            start_ms: start,
            end_ms: samples.len(),
        });
        // Settle gap so each segment's rotation can be measured on its own.
        samples.extend(std::iter::repeat_n((0, 0), 300));
    }
    samples.extend(std::iter::repeat_n((0, 0), 1000));
    Trace {
        samples,
        segments: segs,
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Method {
    /// Old approach: average velocity over a window, clamp at full, inverse curve,
    /// round to the nearest code. No ledger.
    Legacy { window_ms: usize },
    /// Displacement-conserving ledger + effective-space quantizer.
    Accurate,
}

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct SimResult {
    /// Mean |error| of each segment's rotation, % of that segment's rotation.
    pub flick_err_pct: f64,
    pub track_err_pct: f64,
    /// Micro-adjustments (1–3 counts): % of them the camera responded to at all.
    pub micro_seen_pct: f64,
    /// Whole-trace end position error, % of total path length.
    pub drift_pct: f64,
}

#[derive(Clone, Debug)]
pub struct SimSetup {
    pub curve: GameCurve,
    pub res: Resolution,
    pub calib: Calibration,
    pub fps: f64,
    pub output_hz: f64,
    pub mouse: MouseConfig,
}

/// Run the trace through a method and a model game. Returns per-ms camera yaw/pitch in
/// mouse counts (so it compares directly with the trace).
pub fn run(setup: &SimSetup, method: Method, trace: &Trace) -> Vec<(f64, f64)> {
    let full = setup.calib.full_deflection_cps();
    let out_every = (1000.0 / setup.output_hz).round().max(1.0) as usize;
    let dt_out = out_every as f64 / 1000.0;
    let mut cfg = setup.mouse;
    cfg.full_deflection_cps = full;
    let mut acc = MouseToStick::new(cfg, setup.curve, setup.res);
    let mut window: std::collections::VecDeque<(i32, i32)> = Default::default();
    let mut pending = (0i32, 0i32);
    let mut stick = (0.0f32, 0.0f32);

    let mut rng = Rng(7);
    let mut next_frame = 0.0f64;
    let mut cam = (0.0f64, 0.0f64);
    let mut last_frame_t = 0.0f64;
    let mut held_eff = (0.0f32, 0.0f32);
    let mut out = Vec::with_capacity(trace.samples.len());

    for (ms, &(dx, dy)) in trace.samples.iter().enumerate() {
        pending.0 += dx;
        pending.1 += dy;
        if let Method::Legacy { window_ms } = method {
            window.push_back((dx, dy));
            while window.len() > window_ms.max(1) {
                window.pop_front();
            }
        }
        if ms % out_every == 0 {
            match method {
                Method::Accurate => {
                    acc.push(pending.0, pending.1);
                    let q = acc.tick(dt_out);
                    stick = q.raw;
                }
                Method::Legacy { .. } => {
                    let (sx, sy) = window
                        .iter()
                        .fold((0i64, 0i64), |a, b| (a.0 + b.0 as i64, a.1 + b.1 as i64));
                    let w = window.len().max(1) as f64 / 1000.0;
                    let (mut ex, mut ey) = (sx as f64 / w / full, -(sy as f64) / w / full);
                    let m = ex.hypot(ey);
                    if m > 1.0 {
                        ex /= m;
                        ey /= m;
                    }
                    let (rx, ry) = setup.curve.inverse(ex as f32, ey as f32);
                    stick = (
                        setup.res.to_norm(setup.res.to_code(rx)),
                        setup.res.to_norm(setup.res.to_code(ry)),
                    );
                }
            }
            pending = (0, 0);
        }
        // Game frame: sample the stick, turn by (effective * top speed * frame time).
        let t = ms as f64 / 1000.0;
        if t >= next_frame {
            let fdt = t - last_frame_t;
            cam.0 += held_eff.0 as f64 * full * fdt;
            cam.1 += -(held_eff.1 as f64) * full * fdt;
            held_eff = setup.curve.forward(stick.0, stick.1);
            last_frame_t = t;
            next_frame = t + (1.0 / setup.fps) * rng.range(0.9, 1.1);
        }
        let partial = t - last_frame_t;
        out.push((
            cam.0 + held_eff.0 as f64 * full * partial,
            cam.1 - held_eff.1 as f64 * full * partial,
        ));
    }
    out
}

pub fn score(trace: &Trace, cam: &[(f64, f64)]) -> SimResult {
    let mut ideal = Vec::with_capacity(trace.samples.len());
    let mut p = (0.0f64, 0.0f64);
    let mut path = 0.0;
    for &(dx, dy) in &trace.samples {
        p.0 += dx as f64;
        p.1 += dy as f64;
        path += (dx as f64).hypot(dy as f64);
        ideal.push(p);
    }
    let settle = 250; // ms after a segment before measuring (inside the 300 ms gap)
    let (mut fe, mut fnum, mut te, mut tnum, mut mseen, mut mnum) = (0.0, 0, 0.0, 0, 0, 0);
    for s in &trace.segments {
        let a = s.start_ms.saturating_sub(1);
        let b = (s.end_ms + settle).min(cam.len() - 1);
        let want = (ideal[b].0 - ideal[a].0, ideal[b].1 - ideal[a].1);
        let got = (cam[b].0 - cam[a].0, cam[b].1 - cam[a].1);
        let wl = want.0.hypot(want.1);
        let err = (got.0 - want.0).hypot(got.1 - want.1);
        match s.kind {
            SegKind::Flick => {
                fe += err / wl.max(1e-9);
                fnum += 1
            }
            SegKind::Track => {
                te += err / wl.max(1e-9);
                tnum += 1
            }
            SegKind::Micro => {
                // Each nudge: did the camera move at all within 60 ms?
                for i in s.start_ms..s.end_ms {
                    if trace.samples[i].0 != 0 {
                        mnum += 1;
                        let j = (i + 60).min(cam.len() - 1);
                        if (cam[j].0 - cam[i.saturating_sub(1)].0).abs() > 0.25 {
                            mseen += 1;
                        }
                    }
                }
            }
            SegKind::Pause => {}
        }
    }
    let last = cam.len() - 1;
    let drift = (cam[last].0 - ideal[last].0).hypot(cam[last].1 - ideal[last].1);
    SimResult {
        flick_err_pct: 100.0 * fe / fnum.max(1) as f64,
        track_err_pct: 100.0 * te / tnum.max(1) as f64,
        micro_seen_pct: 100.0 * mseen as f64 / mnum.max(1) as f64,
        drift_pct: 100.0 * drift / path.max(1e-9),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::DeadzoneShape;

    fn setup(res: Resolution) -> SimSetup {
        SimSetup {
            curve: GameCurve {
                inner: 0.2,
                outer: 1.0,
                exponent: 2.0,
                shape: DeadzoneShape::Radial,
            },
            res,
            calib: Calibration {
                counts_per_360: 8000.0,
                max_turn_deg_s: 180.0,
            },
            fps: 60.0,
            output_hz: 500.0,
            mouse: MouseConfig {
                max_carry_ms: 1000.0,
                ..Default::default()
            },
        }
    }

    #[test]
    fn accurate_beats_legacy_everywhere() {
        for res in [Resolution::XINPUT, Resolution::DS4, Resolution::SWITCH] {
            let s = setup(res);
            let tr = make_trace(42, 60, s.calib.full_deflection_cps());
            let new = score(&tr, &run(&s, Method::Accurate, &tr));
            let old = score(&tr, &run(&s, Method::Legacy { window_ms: 16 }, &tr));
            assert!(
                new.flick_err_pct < old.flick_err_pct,
                "{res:?} {new:?} {old:?}"
            );
            assert!(new.drift_pct < 1.0, "{res:?} {new:?}");
            assert!(
                new.micro_seen_pct >= old.micro_seen_pct,
                "{res:?} {new:?} {old:?}"
            );
            assert!(
                new.flick_err_pct < 3.0 && new.track_err_pct < 3.0,
                "{res:?} {new:?}"
            );
        }
    }
}
