//! The engine ties capture decisions and the mapper together behind one lock-friendly API.
//! Everything the Windows layer needs from the portable core goes through [`Engine`].

use crate::capture::{Action, Capture, Edge, Verdict};
use crate::config::Config;
use crate::keys::{Source, WheelDir};
use crate::mapper::Mapper;
use crate::pad::Report;
use std::sync::Arc;

/// One physical wheel notch, in Windows `WHEEL_DELTA` units.
const WHEEL_NOTCH: i32 = 120;

/// If the output thread stalls, replay at most this many catch-up ticks.
const MAX_CATCH_UP_TICKS: u32 = 64;

pub struct Engine {
    capture: Capture,
    mapper: Mapper,
    /// Partial wheel travel for vertical and horizontal wheels (high-resolution wheels report
    /// fractions of a notch).
    wheel_acc: [i32; 2],
}

impl Engine {
    pub fn new(cfg: &Config) -> Engine {
        let bindings = Arc::new(cfg.bindings.clone());
        let mapper = Mapper::new(cfg, Arc::clone(&bindings));
        let capture = Capture::new(
            bindings,
            cfg.general.toggle_hotkey.clone(),
            cfg.general.exit_hotkey.clone(),
            cfg.general.unbound,
            cfg.mouse.swallow_motion,
            mapper.has_mouse_axes(),
            cfg.general.start_enabled,
        );
        Engine {
            capture,
            mapper,
            wheel_acc: [0; 2],
        }
    }

    pub fn enabled(&self) -> bool {
        self.capture.enabled()
    }

    /// Forces capture on or off (used for fail-safe shutdown of mapping).
    /// Changes the mouse smoothing window (see [`Mapper::set_smoothing_ms`]).
    pub fn set_smoothing_ms(&mut self, ms: f64) {
        self.mapper.set_smoothing_ms(ms);
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.capture.set_enabled(on, &mut self.mapper);
    }

    /// A key or mouse button edge from the OS hook.
    pub fn on_input(&mut self, src: Source, edge: Edge) -> Verdict {
        self.capture.on_event(src, edge, &mut self.mapper)
    }

    /// Should the OS hook swallow a mouse-move event?
    pub fn swallow_motion(&self) -> bool {
        self.capture.swallow_motion()
    }

    /// A wheel event. `delta` is in `WHEEL_DELTA` units (120 per notch, fractions allowed).
    /// Returns whether the event must be swallowed.
    pub fn on_wheel(&mut self, vertical: bool, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let idx = usize::from(!vertical);
        let dir_for = |positive: bool| match (vertical, positive) {
            (true, true) => WheelDir::Up,
            (true, false) => WheelDir::Down,
            (false, true) => WheelDir::Right,
            (false, false) => WheelDir::Left,
        };
        let acc = &mut self.wheel_acc[idx];
        if *acc != 0 && (*acc > 0) != (delta > 0) {
            *acc = 0; // reversing direction discards stale partial travel
        }
        *acc += delta;
        let swallow = self.capture.wheel_swallowed(dir_for(delta > 0));
        while self.wheel_acc[idx].abs() >= WHEEL_NOTCH {
            let positive = self.wheel_acc[idx] > 0;
            self.wheel_acc[idx] -= if positive { WHEEL_NOTCH } else { -WHEEL_NOTCH };
            if swallow {
                self.mapper.wheel(dir_for(positive));
            }
        }
        swallow
    }

    /// Produces the next controller report. `elapsed` is the real time since the previous call
    /// in seconds; `dx`/`dy` are the raw mouse counts accumulated in that time.
    ///
    /// The mapper advances in fixed nominal ticks. If the caller was late, the missed ticks are
    /// replayed (with the mouse motion fed in on the first), so output stays exactly
    /// displacement-conserving and timers stay accurate even after a scheduling hiccup.
    pub fn step(&mut self, elapsed: f64, dx: i64, dy: i64) -> Report {
        if !self.capture.enabled() {
            return Report::default();
        }
        self.mapper.add_mouse(dx, dy);
        let ticks = ((elapsed / self.mapper.tick_dt()).round() as i64)
            .clamp(1, i64::from(MAX_CATCH_UP_TICKS)) as u32;
        let mut report = self.mapper.tick();
        for _ in 1..ticks {
            report = self.mapper.tick();
        }
        report
    }
}

/// Helper for the Windows layer's notices.
pub fn describe(action: Action) -> &'static str {
    match action {
        Action::Toggled(true) => "mapping ENABLED",
        Action::Toggled(false) => "mapping disabled (input passes through)",
        Action::Exit => "exit requested",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::BTN_A;

    fn engine(extra: &str) -> Engine {
        let text = format!(
            "[general]\noutput_hz = 1000\n{extra}\n[mouse]\nsmoothing_ms = 4\n\
             [axis.mouse_x]\ntarget = \"RightStickX\"\nfull_deflection_cps = 1000\n\
             [bindings]\nA = [\"Space\", \"WheelUp\"]\n"
        );
        Engine::new(&Config::parse(&text).unwrap())
    }

    #[test]
    fn disabled_engine_reports_neutral_and_discards_motion() {
        let mut e = engine("start_enabled = false");
        assert_eq!(e.step(0.001, 5000, 5000), Report::default());
        e.set_enabled(true);
        // motion from while it was disabled must not leak in
        let sum: i64 = (0..30)
            .map(|_| i64::from(e.step(0.001, 0, 0).thumb_rx))
            .sum();
        assert_eq!(sum, 0);
    }

    #[test]
    fn toggling_off_drops_held_buttons_immediately() {
        let mut e = engine("");
        e.on_input(Source::parse("Space").unwrap(), Edge::Down);
        assert_eq!(e.step(0.001, 0, 0).buttons, BTN_A);
        e.set_enabled(false);
        assert_eq!(e.step(0.001, 0, 0), Report::default());
    }

    #[test]
    fn late_calls_replay_missed_ticks() {
        let mut a = engine("");
        let mut b = engine("");
        // `a` is serviced every tick; `b` is serviced once, three ticks late
        let mut last_a = a.step(0.001, 40, 0);
        for _ in 1..3 {
            last_a = a.step(0.001, 0, 0);
        }
        let last_b = b.step(0.003, 40, 0);
        assert_ne!(last_a.thumb_rx, 0, "still inside the smoothing window");
        // after the same number of nominal ticks both engines are in the same state
        assert_eq!(last_a, last_b);
        // and they stay in lock-step afterwards
        for _ in 0..10 {
            assert_eq!(a.step(0.001, 0, 0), b.step(0.001, 0, 0));
        }
    }

    #[test]
    fn wheel_notches_accumulate_and_swallow_only_when_bound() {
        let mut e = engine("wheel_pulse_ms = 5");
        // WheelUp is bound
        assert!(e.on_wheel(true, 120));
        let mut pressed = false;
        for _ in 0..20 {
            pressed |= e.step(0.001, 0, 0).buttons & BTN_A != 0;
        }
        assert!(pressed);
        // WheelDown is not bound: passes through
        assert!(!e.on_wheel(true, -120));
        // horizontal wheel is not bound
        assert!(!e.on_wheel(false, 120));
    }

    #[test]
    fn high_resolution_wheel_needs_a_full_notch() {
        let mut e = engine("wheel_pulse_ms = 5");
        for _ in 0..7 {
            assert!(e.on_wheel(true, 15)); // 105 units: not a notch yet
        }
        let early: u16 = (0..20).map(|_| e.step(0.001, 0, 0).buttons & BTN_A).sum();
        assert_eq!(early, 0);
        e.on_wheel(true, 15); // 120 total
        let later: u16 = (0..20).map(|_| e.step(0.001, 0, 0).buttons & BTN_A).sum();
        assert_ne!(later, 0);
    }

    #[test]
    fn reversing_the_wheel_discards_partial_travel() {
        let mut e = engine("wheel_pulse_ms = 5");
        e.on_wheel(true, 100);
        e.on_wheel(true, -100);
        e.on_wheel(true, 100); // fresh 100 only, still no notch
        let pressed: u16 = (0..20).map(|_| e.step(0.001, 0, 0).buttons & BTN_A).sum();
        assert_eq!(pressed, 0);
    }

    #[test]
    fn describe_actions() {
        assert!(describe(Action::Toggled(true)).contains("ENABLED"));
        assert!(describe(Action::Exit).contains("exit"));
    }
}
