//! Mouse motion -> stick deflection with matching movement.
//!
//! # The problem
//! A mouse reports *displacement* ("moved 12 counts"); a stick sets *velocity* ("held at 40 %"),
//! and the game integrates that velocity over its own frame times. To make the camera end up
//! where the mouse would have put it, the stick signal must satisfy
//! `integral(speed(t) dt) == total displacement`.
//!
//! # The model
//! Everything is expressed in **stick-seconds**: displacement divided by the calibration
//! `full_deflection_cps` (the mouse speed that should equal a fully deflected stick). A speed of
//! `1.0` is then exactly "full stick".
//!
//! [`StickStage`] runs once per output tick and has two parts:
//!
//! 1. **Boxcar window.** Each tick's displacement is spread evenly over the last `N` ticks
//!    (`smoothing_ms`). The output speed is `window_sum / window_length`. Impulses become short
//!    plateaus the game can sample at its frame rate without missing them, and the total area
//!    under the output equals the total input exactly (the window has finite support, so the
//!    stick returns to exactly zero afterwards).
//! 2. **Speed-limited backlog.** The window sum may never exceed `window_length` (speed 1.0).
//!    Displacement that does not fit waits in a backlog and is committed as headroom appears, so
//!    a flick faster than the stick can express is *delayed*, not lost (up to
//!    `max_backlog_ms`). Commits are always parallel to the pending motion, never negative, so
//!    stick direction is preserved.
//!
//! [`StickStage::respond`] then undoes the game's own input shaping so that the *game's* result
//! equals the intended speed: radial deadzone compensation plus an inverse response curve.

/// Deadzone finder: seconds of centred stick before the ramp starts (time to switch to the game).
pub const RAMP_WAIT_S: f64 = 4.0;
/// Seconds the ramp takes to reach [`RAMP_MAX`].
pub const RAMP_DURATION_S: f64 = 20.0;
/// Largest deflection the finder tries; no sensible game deadzone is bigger.
pub const RAMP_MAX: f64 = 0.7;
/// Reaction-time allowance subtracted from the marked value (about 0.3 s of ramp).
pub const RAMP_REACTION: f64 = 0.01;

/// Stick deflection the deadzone finder sends `t` seconds after it started.
pub fn deadzone_ramp(t: f64) -> f64 {
    ((t - RAMP_WAIT_S) / RAMP_DURATION_S).clamp(0.0, 1.0) * RAMP_MAX
}

#[derive(Debug, Clone)]
pub struct StickStage {
    ring: Vec<(f64, f64)>,
    idx: usize,
    window: f64,
    pending: (f64, f64),
    backlog_cap: f64,
    deadzone: f64,
    inv_exponent: f64,
    tick_dt: f64,
    /// A new window length waiting to be applied the next time the stage is at rest.
    pending_ticks: Option<usize>,
}

impl StickStage {
    /// * `ticks` - window length in output ticks (at least 1; 1 disables smoothing).
    /// * `tick_dt` - nominal tick length in seconds.
    /// * `max_backlog_s` - largest backlog, in stick-seconds; excess motion is dropped.
    /// * `deadzone` - the game's inner radial deadzone as a fraction of full stick (`0..1`).
    /// * `curve_exponent` - the game's response exponent (`1.0` = linear).
    pub fn new(
        ticks: usize,
        tick_dt: f64,
        max_backlog_s: f64,
        deadzone: f64,
        curve_exponent: f64,
    ) -> Self {
        let n = ticks.max(1);
        StickStage {
            ring: vec![(0.0, 0.0); n],
            idx: 0,
            window: n as f64 * tick_dt,
            pending: (0.0, 0.0),
            backlog_cap: max_backlog_s.max(0.0),
            deadzone: deadzone.clamp(0.0, 0.99),
            inv_exponent: 1.0 / curve_exponent.max(0.01),
            tick_dt,
            pending_ticks: None,
        }
    }

    /// Window length in ticks currently in use.
    pub fn window_ticks(&self) -> usize {
        self.ring.len()
    }

    /// Changes the smoothing window. Applied only when the stage is at rest (nothing in the
    /// window, nothing queued), so changing it never loses or duplicates any movement.
    pub fn request_window(&mut self, ticks: usize) {
        let n = ticks.max(1);
        self.pending_ticks = (n != self.ring.len()).then_some(n);
    }

    fn at_rest(&self) -> bool {
        self.pending == (0.0, 0.0) && self.ring.iter().all(|e| *e == (0.0, 0.0))
    }

    pub fn window_len(&self) -> f64 {
        self.window
    }

    /// Length of the not-yet-committed backlog, in stick-seconds.
    pub fn backlog(&self) -> f64 {
        self.pending.0.hypot(self.pending.1)
    }

    pub fn reset(&mut self) {
        self.ring.iter_mut().for_each(|e| *e = (0.0, 0.0));
        self.idx = 0;
        self.pending = (0.0, 0.0);
    }

    /// Feeds one tick of motion (in stick-seconds) and returns the *linear* stick vector for
    /// this tick, `|v| <= 1`. Call [`respond`](Self::respond) to turn it into the value to send.
    pub fn push(&mut self, dx: f64, dy: f64) -> (f64, f64) {
        if let Some(n) = self.pending_ticks {
            if self.at_rest() {
                self.ring = vec![(0.0, 0.0); n];
                self.idx = 0;
                self.window = n as f64 * self.tick_dt;
                self.pending_ticks = None;
            }
        }
        self.pending.0 += dx;
        self.pending.1 += dy;

        // Sum of the entries that stay in the window once the oldest slot is reused.
        let (mut bx, mut by) = (0.0, 0.0);
        for (i, e) in self.ring.iter().enumerate() {
            if i != self.idx {
                bx += e.0;
                by += e.1;
            }
        }

        // Largest fraction `alpha` of the pending motion that keeps |base + alpha * pending|
        // within the window length (speed <= 1). Solve |b + a p|^2 = W^2 for a.
        let (px, py) = self.pending;
        let pp = px * px + py * py;
        let bb = bx * bx + by * by;
        let w2 = self.window * self.window;
        let alpha = if pp <= 0.0 || bb >= w2 {
            0.0
        } else {
            let bp = bx * px + by * py;
            let disc = (bp * bp - pp * (bb - w2)).max(0.0);
            ((-bp + disc.sqrt()) / pp).clamp(0.0, 1.0)
        };

        let commit = (px * alpha, py * alpha);
        self.pending = (px - commit.0, py - commit.1);
        if alpha >= 1.0 {
            self.pending = (0.0, 0.0);
        }
        self.ring[self.idx] = commit;
        self.idx = (self.idx + 1) % self.ring.len();

        // Bound what is left over for later ticks; anything beyond it is intentionally
        // discarded. This happens only *after* this tick's share was committed, so a limit of 0
        // means "no catch-up" (motion faster than full stick is cut), never "no motion".
        let plen = self.backlog();
        if plen > self.backlog_cap {
            let k = if plen > 0.0 {
                self.backlog_cap / plen
            } else {
                0.0
            };
            self.pending.0 *= k;
            self.pending.1 *= k;
        }

        ((bx + commit.0) / self.window, (by + commit.1) / self.window)
    }

    /// Maps the desired speed vector to the stick vector to send, compensating the game's inner
    /// radial deadzone and response curve. Direction is preserved exactly.
    ///
    /// If the game computes `speed = ((raw - dz) / (1 - dz)) ^ p` for `raw > dz` (else 0), this
    /// returns `raw = dz + (1 - dz) * speed ^ (1 / p)`, so the game ends up at exactly `speed`.
    pub fn respond(&self, v: (f64, f64)) -> (f64, f64) {
        let m = v.0.hypot(v.1);
        if m < 1e-12 {
            return (0.0, 0.0);
        }
        let speed = m.min(1.0);
        let raw = self.deadzone + (1.0 - self.deadzone) * speed.powf(self.inv_exponent);
        (v.0 / m * raw, v.1 / m * raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f64 = 0.001;

    fn lcg(state: &mut u64) -> f64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*state >> 33) as f64) / ((1u64 << 31) as f64)
    }

    fn mag(v: (f64, f64)) -> f64 {
        v.0.hypot(v.1)
    }

    /// Runs the stage until it is idle and returns (area under the output, ticks used).
    fn drain(s: &mut StickStage, max_ticks: usize) -> (f64, f64, usize) {
        let (mut ax, mut ay) = (0.0, 0.0);
        for t in 0..max_ticks {
            let v = s.push(0.0, 0.0);
            ax += v.0 * DT;
            ay += v.1 * DT;
            if s.backlog() == 0.0 && v == (0.0, 0.0) {
                return (ax, ay, t + 1);
            }
        }
        (ax, ay, max_ticks)
    }

    #[test]
    fn impulse_becomes_plateau_with_exact_area() {
        let mut s = StickStage::new(10, DT, 1.0, 0.0, 1.0);
        let d = 0.004; // 4 ms at full stick spread over 10 ms -> speed 0.4
        let first = s.push(d, 0.0);
        assert!((first.0 - 0.4).abs() < 1e-12, "{first:?}");
        let mut area = first.0 * DT;
        for _ in 0..9 {
            let v = s.push(0.0, 0.0);
            assert!((v.0 - 0.4).abs() < 1e-12);
            area += v.0 * DT;
        }
        // the 11th tick the window has moved past the impulse
        let v = s.push(0.0, 0.0);
        assert_eq!(v, (0.0, 0.0));
        assert!((area - d).abs() < 1e-12);
    }

    #[test]
    fn area_is_conserved_for_random_unsaturated_input() {
        let mut s = StickStage::new(8, DT, 1.0, 0.0, 1.0);
        let mut seed = 7u64;
        let (mut sent_x, mut sent_y) = (0.0, 0.0);
        let (mut got_x, mut got_y) = (0.0, 0.0);
        for _ in 0..5000 {
            // sporadic small motion; per-tick displacement stays far below the window length
            let dx = (lcg(&mut seed) - 0.5) * 0.0004;
            let dy = (lcg(&mut seed) - 0.5) * 0.0004;
            sent_x += dx;
            sent_y += dy;
            let v = s.push(dx, dy);
            assert!(mag(v) <= 1.0 + 1e-12);
            got_x += v.0 * DT;
            got_y += v.1 * DT;
        }
        let (ax, ay, _) = drain(&mut s, 100);
        got_x += ax;
        got_y += ay;
        assert!((got_x - sent_x).abs() < 1e-12, "{got_x} vs {sent_x}");
        assert!((got_y - sent_y).abs() < 1e-12, "{got_y} vs {sent_y}");
    }

    #[test]
    fn output_returns_to_exact_zero_after_input_stops() {
        let mut s = StickStage::new(12, DT, 1.0, 0.0, 1.0);
        for _ in 0..50 {
            s.push(0.0003, -0.0002);
        }
        let (_, _, ticks) = drain(&mut s, 200);
        assert!(ticks <= 13, "took {ticks} ticks to go idle");
        assert_eq!(s.push(0.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn speed_never_exceeds_full_stick_and_backlog_is_paid_back() {
        let mut s = StickStage::new(10, DT, 5.0, 0.0, 1.0);
        // 50 ms of motion delivered in one tick: 5x faster than the stick can express
        let big = 0.05;
        let (mut area, mut peak) = (0.0, 0.0f64);
        let v = s.push(big, 0.0);
        area += v.0 * DT;
        peak = peak.max(mag(v));
        for _ in 0..200 {
            let v = s.push(0.0, 0.0);
            assert!(mag(v) <= 1.0 + 1e-12);
            area += v.0 * DT;
            peak = peak.max(mag(v));
        }
        assert!(peak > 0.999, "stick should saturate, peak {peak}");
        assert!((area - big).abs() < 1e-9, "displacement preserved: {area}");
        assert_eq!(s.backlog(), 0.0);
    }

    #[test]
    fn saturation_preserves_direction() {
        let mut s = StickStage::new(10, DT, 5.0, 0.0, 1.0);
        let mut worst: f64 = 0.0;
        let v = s.push(0.03, 0.01); // direction atan(1/3)
        let want = 0.01f64.atan2(0.03);
        worst = worst.max((v.1.atan2(v.0) - want).abs());
        for _ in 0..100 {
            let v = s.push(0.0, 0.0);
            if mag(v) > 1e-9 {
                worst = worst.max((v.1.atan2(v.0) - want).abs());
            }
        }
        assert!(worst < 1e-9, "direction drift {worst}");
    }

    #[test]
    fn backlog_cap_drops_excess() {
        let mut s = StickStage::new(10, DT, 0.02, 0.0, 1.0);
        s.push(1.0, 0.0); // absurd flick
        let (area, _, _) = drain(&mut s, 1000);
        // only about cap + one window can come out
        assert!(area < 0.02 + 0.010 + 1e-9, "area {area}");
        assert!(area > 0.015, "area {area}");
    }

    #[test]
    fn zero_catch_up_still_passes_all_normal_motion() {
        // Regression: a catch-up limit of 0 (the Snappy preset) used to discard every movement.
        let mut s = StickStage::new(6, DT, 0.0, 0.0, 1.0);
        let mut sent = 0.0;
        let mut got = 0.0;
        for _ in 0..500 {
            let d = 0.0004; // 40 % of full stick, well within what the stick can express
            sent += d;
            got += s.push(d, 0.0).0 * DT;
        }
        for _ in 0..20 {
            got += s.push(0.0, 0.0).0 * DT;
        }
        assert!((got - sent).abs() < 1e-12, "{got} vs {sent}");
        assert_eq!(s.backlog(), 0.0);
    }

    #[test]
    fn zero_catch_up_cuts_only_the_excess_and_stops_at_once() {
        let mut s = StickStage::new(10, DT, 0.0, 0.0, 1.0);
        let first = s.push(1.0, 0.0); // absurd flick
        assert!(first.0 > 0.999, "the stick still goes to full: {first:?}");
        assert_eq!(s.backlog(), 0.0, "nothing is queued for later");
        let mut moving = 0;
        while s.push(0.0, 0.0).0 > 0.0 {
            moving += 1;
            assert!(moving < 20);
        }
        assert!(moving <= 10, "returns to centre within one window");
    }

    #[test]
    fn deadzone_ramp_waits_then_rises_slowly_to_its_cap() {
        assert_eq!(deadzone_ramp(0.0), 0.0);
        assert_eq!(deadzone_ramp(RAMP_WAIT_S), 0.0);
        let mid = deadzone_ramp(RAMP_WAIT_S + RAMP_DURATION_S / 2.0);
        assert!((mid - RAMP_MAX / 2.0).abs() < 1e-12);
        assert_eq!(deadzone_ramp(RAMP_WAIT_S + RAMP_DURATION_S + 5.0), RAMP_MAX);
        // slow enough that a typical reaction time costs about RAMP_REACTION
        let per_second = RAMP_MAX / RAMP_DURATION_S;
        assert!((per_second * 0.3 - RAMP_REACTION).abs() < 0.002);
    }

    #[test]
    fn window_change_waits_for_rest_and_conserves_motion() {
        let mut s = StickStage::new(10, DT, 1.0, 0.0, 1.0);
        let mut sent = 0.0;
        let mut got = 0.0;
        for i in 0..200 {
            if i == 50 {
                s.request_window(4); // asked mid-motion
            }
            let d = if i < 120 { 0.0003 } else { 0.0 };
            sent += d;
            got += s.push(d, 0.0).0 * DT;
            if i < 120 {
                assert_eq!(s.window_ticks(), 10, "not applied while moving");
            }
        }
        assert_eq!(s.window_ticks(), 4, "applied once at rest");
        assert!((got - sent).abs() < 1e-12, "{got} vs {sent}");
        // the new window works: an impulse now lasts 4 ticks
        s.push(0.0004, 0.0);
        let mut ticks = 1;
        while s.push(0.0, 0.0).0 > 0.0 {
            ticks += 1;
        }
        assert_eq!(ticks, 4);
        s.request_window(4);
        assert!(s.pending_ticks.is_none(), "same size is a no-op");
    }

    #[test]
    fn single_tick_window_is_memoryless() {
        let mut s = StickStage::new(1, DT, 1.0, 0.0, 1.0);
        let v = s.push(0.0005, 0.0);
        assert!((v.0 - 0.5).abs() < 1e-12);
        assert_eq!(s.push(0.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn reset_clears_everything() {
        let mut s = StickStage::new(10, DT, 1.0, 0.0, 1.0);
        s.push(0.5, 0.5);
        s.reset();
        assert_eq!(s.backlog(), 0.0);
        assert_eq!(s.push(0.0, 0.0), (0.0, 0.0));
    }

    /// The game-side model `respond` is designed to cancel.
    fn game_speed(raw: f64, dz: f64, p: f64) -> f64 {
        if raw <= dz {
            0.0
        } else {
            ((raw - dz) / (1.0 - dz)).powf(p)
        }
    }

    #[test]
    fn respond_inverts_deadzone_and_curve() {
        for &(dz, p) in &[
            (0.0, 1.0),
            (0.2652, 1.0),
            (0.24, 2.0),
            (0.1, 0.6),
            (0.5, 3.0),
        ] {
            let s = StickStage::new(4, DT, 1.0, dz, p);
            for i in 1..=100 {
                let speed = i as f64 / 100.0;
                let out = s.respond((speed, 0.0));
                let got = game_speed(mag(out), dz, p);
                assert!(
                    (got - speed).abs() < 1e-9,
                    "dz {dz} p {p} speed {speed}: game computes {got}"
                );
            }
        }
    }

    #[test]
    fn respond_preserves_direction_and_clamps() {
        let s = StickStage::new(4, DT, 1.0, 0.25, 1.0);
        let out = s.respond((0.3, -0.4));
        assert!((out.1.atan2(out.0) - (-0.4f64).atan2(0.3)).abs() < 1e-12);
        let big = s.respond((3.0, 4.0));
        assert!((mag(big) - 1.0).abs() < 1e-12);
        assert_eq!(s.respond((0.0, 0.0)), (0.0, 0.0));
        // a vanishingly small motion lands exactly on the deadzone edge, which the game maps to 0
        let tiny = s.respond((1e-9, 0.0));
        assert!((mag(tiny) - 0.25).abs() < 1e-6);
    }
}
