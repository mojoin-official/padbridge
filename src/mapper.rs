//! Turns held inputs and mouse motion into a controller [`Report`], one nominal tick at a time.

use crate::bindings::Bindings;
use crate::config::{Config, Socd};
use crate::keys::{Source, WheelDir};
use crate::mouse::StickStage;
use crate::pad::{
    axis_to_i16, Output, Report, BTN_DPAD_DOWN, BTN_DPAD_LEFT, BTN_DPAD_RIGHT, BTN_DPAD_UP,
};
use std::collections::HashSet;
use std::sync::Arc;

/// Digital axes as (negative output, positive output):
/// left X, left Y, right X, right Y, D-pad X, D-pad Y.
const AXES: [(Output, Output); 6] = [
    (Output::LsLeft, Output::LsRight),
    (Output::LsDown, Output::LsUp),
    (Output::RsLeft, Output::RsRight),
    (Output::RsDown, Output::RsUp),
    (Output::DpadLeft, Output::DpadRight),
    (Output::DpadDown, Output::DpadUp),
];

const MAX_QUEUED_NOTCHES: u8 = 16;

#[derive(Copy, Clone, Debug, PartialEq)]
enum WheelPhase {
    Idle,
    /// Pressed; whole ticks remaining (integers, so pulse lengths are exact).
    On(u32),
    /// Released, waiting out the gap before the next notch; ticks remaining.
    Gap(u32),
}

#[derive(Copy, Clone, Debug)]
struct WheelState {
    queued: u8,
    phase: WheelPhase,
}

const WHEEL_IDLE: WheelState = WheelState {
    queued: 0,
    phase: WheelPhase::Idle,
};

fn wheel_index(d: WheelDir) -> usize {
    match d {
        WheelDir::Up => 0,
        WheelDir::Down => 1,
        WheelDir::Left => 2,
        WheelDir::Right => 3,
    }
}

const WHEEL_DIRS: [WheelDir; 4] = [
    WheelDir::Up,
    WheelDir::Down,
    WheelDir::Left,
    WheelDir::Right,
];

/// Where one mouse axis feeds into a stick stage.
#[derive(Copy, Clone, Debug)]
struct MouseRoute {
    from_y: bool,
    /// 0 = left stick, 1 = right stick.
    stick: usize,
    /// 0 = x component, 1 = y component.
    comp: usize,
    /// Counts -> stick-seconds, including sensitivity, inversion and the screen-down = stick-down
    /// sign convention.
    scale: f64,
}

pub struct Mapper {
    bindings: Arc<Bindings>,
    tick_dt: f64,
    socd: Socd,
    normalize: bool,
    ramp_s: f64,
    walk_scale: f64,
    trigger_value: u8,
    wheel_pulse_ticks: u32,

    pressed: HashSet<Source>,
    held: [u16; Output::COUNT],
    last: [i8; 6],
    slew: [(f64, f64); 2],
    wheels: [WheelState; 4],

    routes: Vec<MouseRoute>,
    stages: [Option<StickStage>; 2],
    acc: [[f64; 2]; 2],
}

impl Mapper {
    pub fn new(cfg: &Config, bindings: Arc<Bindings>) -> Mapper {
        let tick_dt = 1.0 / f64::from(cfg.general.output_hz);
        let ticks = (cfg.mouse.smoothing_ms / 1000.0 / tick_dt).round().max(1.0) as usize;

        let mut routes = Vec::new();
        let mut stages: [Option<StickStage>; 2] = [None, None];
        for a in &cfg.axes {
            let from_y = a.source == crate::config::MouseAxis::Y;
            let stick = a.target.stick() as usize;
            let mut scale = a.sensitivity / a.full_deflection_cps;
            // Mouse "down" is +Y; a stick pushed down is -Y.
            if from_y {
                scale = -scale;
            }
            if a.invert {
                scale = -scale;
            }
            routes.push(MouseRoute {
                from_y,
                stick,
                comp: usize::from(a.target.is_y()),
                scale,
            });
            stages[stick].get_or_insert_with(|| {
                StickStage::new(
                    ticks,
                    tick_dt,
                    cfg.mouse.max_backlog_ms / 1000.0,
                    cfg.mouse.deadzone,
                    cfg.mouse.curve_exponent,
                )
            });
        }

        Mapper {
            bindings,
            tick_dt,
            socd: cfg.general.socd,
            normalize: cfg.general.normalize_diagonals,
            ramp_s: cfg.general.stick_ramp_ms / 1000.0,
            walk_scale: cfg.general.walk_scale,
            trigger_value: cfg.general.trigger_value,
            wheel_pulse_ticks: (cfg.general.wheel_pulse_ms / 1000.0 / tick_dt)
                .round()
                .max(1.0) as u32,
            pressed: HashSet::new(),
            held: [0; Output::COUNT],
            last: [0; 6],
            slew: [(0.0, 0.0); 2],
            wheels: [WHEEL_IDLE; 4],
            routes,
            stages,
            acc: [[0.0; 2]; 2],
        }
    }

    pub fn tick_dt(&self) -> f64 {
        self.tick_dt
    }

    pub fn has_mouse_axes(&self) -> bool {
        !self.routes.is_empty()
    }

    /// Releases everything and clears all motion state, returning to a neutral pad.
    pub fn reset(&mut self) {
        self.pressed.clear();
        self.held = [0; Output::COUNT];
        self.last = [0; 6];
        self.slew = [(0.0, 0.0); 2];
        self.wheels = [WHEEL_IDLE; 4];
        self.acc = [[0.0; 2]; 2];
        for s in self.stages.iter_mut().flatten() {
            s.reset();
        }
    }

    /// A bound input went down. Repeats for an already-pressed source are ignored.
    pub fn press(&mut self, src: Source) {
        if !self.bindings.contains(src) || !self.pressed.insert(src) {
            return;
        }
        let bindings = Arc::clone(&self.bindings);
        for &o in bindings.outputs(src) {
            let h = &mut self.held[o.index()];
            *h = h.saturating_add(1);
            if *h == 1 {
                self.on_rising(o);
            }
        }
    }

    /// A bound input went up. Releasing a source that is not pressed is a no-op.
    pub fn release(&mut self, src: Source) {
        if !self.pressed.remove(&src) {
            return;
        }
        let bindings = Arc::clone(&self.bindings);
        for &o in bindings.outputs(src) {
            let h = &mut self.held[o.index()];
            *h = h.saturating_sub(1);
        }
    }

    /// One wheel notch happened. Each notch becomes one press of the bound outputs, queued so
    /// fast scrolling yields as many distinct presses as notches (up to a small limit).
    pub fn wheel(&mut self, dir: WheelDir) {
        if !self.bindings.contains(Source::Wheel(dir)) {
            return;
        }
        let w = &mut self.wheels[wheel_index(dir)];
        w.queued = (w.queued + 1).min(MAX_QUEUED_NOTCHES);
    }

    /// Accumulates raw mouse counts (right/down positive), consumed by the next [`tick`](Self::tick).
    pub fn add_mouse(&mut self, dx: i64, dy: i64) {
        for r in &self.routes {
            let counts = if r.from_y { dy } else { dx } as f64;
            self.acc[r.stick][r.comp] += counts * r.scale;
        }
    }

    fn on_rising(&mut self, o: Output) {
        for (i, (neg, pos)) in AXES.iter().enumerate() {
            if o == *neg {
                self.last[i] = -1;
            } else if o == *pos {
                self.last[i] = 1;
            }
        }
    }

    fn is_held(&self, o: Output) -> bool {
        self.held[o.index()] > 0
    }

    /// Resolved digital axis value: -1, 0 or +1 (opposites resolved by the SOCD policy).
    fn axis_value(&self, i: usize) -> i8 {
        let (neg, pos) = AXES[i];
        match (self.is_held(neg), self.is_held(pos)) {
            (false, false) => 0,
            (true, false) => -1,
            (false, true) => 1,
            (true, true) => match self.socd {
                Socd::Neutral => 0,
                Socd::Last => self.last[i],
            },
        }
    }

    fn advance_wheels(&mut self) {
        for dir in WHEEL_DIRS {
            let i = wheel_index(dir);
            let src = Source::Wheel(dir);
            match self.wheels[i].phase {
                WheelPhase::On(rem) => {
                    if rem <= 1 {
                        self.release(src);
                        self.wheels[i].phase = WheelPhase::Gap(self.wheel_pulse_ticks);
                    } else {
                        self.wheels[i].phase = WheelPhase::On(rem - 1);
                    }
                }
                WheelPhase::Gap(rem) => {
                    self.wheels[i].phase = if rem <= 1 {
                        WheelPhase::Idle
                    } else {
                        WheelPhase::Gap(rem - 1)
                    };
                }
                WheelPhase::Idle => {}
            }
            if self.wheels[i].phase == WheelPhase::Idle && self.wheels[i].queued > 0 {
                self.wheels[i].queued -= 1;
                self.wheels[i].phase = WheelPhase::On(self.wheel_pulse_ticks);
                self.press(src);
            }
        }
    }

    fn limit(&self, v: (f64, f64)) -> (f64, f64) {
        if self.normalize {
            let m = v.0.hypot(v.1);
            if m > 1.0 {
                return (v.0 / m, v.1 / m);
            }
            v
        } else {
            (v.0.clamp(-1.0, 1.0), v.1.clamp(-1.0, 1.0))
        }
    }

    /// Advances one nominal tick and returns the report to send. Mouse counts added since the
    /// previous tick are consumed here.
    pub fn tick(&mut self) -> Report {
        self.advance_wheels();

        let mut report = Report::default();

        for o in Output::ALL {
            if self.is_held(o) {
                if let Some(bit) = o.button_bit() {
                    report.buttons |= bit;
                }
            }
        }
        match self.axis_value(4) {
            1 => report.buttons |= BTN_DPAD_RIGHT,
            -1 => report.buttons |= BTN_DPAD_LEFT,
            _ => {}
        }
        match self.axis_value(5) {
            1 => report.buttons |= BTN_DPAD_UP,
            -1 => report.buttons |= BTN_DPAD_DOWN,
            _ => {}
        }
        if self.is_held(Output::Lt) {
            report.left_trigger = self.trigger_value;
        }
        if self.is_held(Output::Rt) {
            report.right_trigger = self.trigger_value;
        }

        let walk = self.is_held(Output::Walk);
        let mut sticks = [(0.0f64, 0.0f64); 2];
        for (s, out) in sticks.iter_mut().enumerate() {
            // Keyboard-driven (digital) part, optionally slewed.
            let k = if s == 0 && walk { self.walk_scale } else { 1.0 };
            let target = (
                f64::from(self.axis_value(2 * s)) * k,
                f64::from(self.axis_value(2 * s + 1)) * k,
            );
            let cur = &mut self.slew[s];
            if self.ramp_s > 0.0 {
                let step = self.tick_dt / self.ramp_s;
                cur.0 += (target.0 - cur.0).clamp(-step, step);
                cur.1 += (target.1 - cur.1).clamp(-step, step);
            } else {
                *cur = target;
            }
            let digital = *cur;
            let digital = if self.normalize {
                let m = digital.0.hypot(digital.1);
                if m > 1.0 {
                    (digital.0 / m, digital.1 / m)
                } else {
                    digital
                }
            } else {
                (digital.0.clamp(-1.0, 1.0), digital.1.clamp(-1.0, 1.0))
            };

            // Mouse-driven (analog) part. The stage always runs so its window drains.
            let mouse = match self.stages[s].as_mut() {
                Some(stage) => {
                    let motion = self.acc[s];
                    self.acc[s] = [0.0, 0.0];
                    let v = stage.push(motion[0], motion[1]);
                    stage.respond(v)
                }
                None => (0.0, 0.0),
            };

            *out = (digital.0 + mouse.0, digital.1 + mouse.1);
        }

        let l = self.limit(sticks[0]);
        let r = self.limit(sticks[1]);
        report.thumb_lx = axis_to_i16(l.0);
        report.thumb_ly = axis_to_i16(l.1);
        report.thumb_rx = axis_to_i16(r.0);
        report.thumb_ry = axis_to_i16(r.1);
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pad::{BTN_A, BTN_B, BTN_LB};

    fn mk(extra_general: &str, bindings: &str) -> Mapper {
        let text = format!(
            "[general]\noutput_hz = 1000\n{extra_general}\n[mouse]\nsmoothing_ms = 4\n\
             [axis.mouse_x]\ntarget = \"RightStickX\"\nfull_deflection_cps = 1000\n\
             [axis.mouse_y]\ntarget = \"RightStickY\"\nfull_deflection_cps = 1000\n\
             [bindings]\n{bindings}\n"
        );
        let cfg = Config::parse(&text).unwrap();
        let b = Arc::new(cfg.bindings.clone());
        Mapper::new(&cfg, b)
    }

    fn src(n: &str) -> Source {
        Source::parse(n).unwrap()
    }

    const WASD: &str = "LeftStickUp = \"W\"\nLeftStickLeft = \"A\"\nLeftStickDown = \"S\"\nLeftStickRight = \"D\"\n";

    #[test]
    fn buttons_and_triggers() {
        let mut m = mk(
            "trigger_value = 200",
            "A = \"Space\"\nLB = [\"Q\", \"Mouse4\"]\nRT = \"MouseLeft\"\nLT = \"MouseRight\"",
        );
        assert_eq!(m.tick(), Report::default());
        m.press(src("Space"));
        m.press(src("MouseLeft"));
        let r = m.tick();
        assert_eq!(r.buttons, BTN_A);
        assert_eq!(r.right_trigger, 200);
        assert_eq!(r.left_trigger, 0);
        m.release(src("Space"));
        m.release(src("MouseLeft"));
        assert_eq!(m.tick(), Report::default());
    }

    #[test]
    fn two_sources_one_output_release_only_when_both_up() {
        let mut m = mk("", "LB = [\"Q\", \"Mouse4\"]");
        m.press(src("Q"));
        m.press(src("Mouse4"));
        assert_eq!(m.tick().buttons, BTN_LB);
        m.release(src("Q"));
        assert_eq!(m.tick().buttons, BTN_LB);
        m.release(src("Mouse4"));
        assert_eq!(m.tick().buttons, 0);
    }

    #[test]
    fn one_source_two_outputs() {
        let mut m = mk("", "A = \"Space\"\nB = \"Space\"");
        m.press(src("Space"));
        assert_eq!(m.tick().buttons, BTN_A | BTN_B);
    }

    #[test]
    fn key_repeat_and_stray_release_are_harmless() {
        let mut m = mk("", "A = \"Space\"");
        m.press(src("Space"));
        m.press(src("Space"));
        m.release(src("Space"));
        assert_eq!(
            m.tick().buttons,
            0,
            "double press must not leave a hold count behind"
        );
        m.release(src("Space"));
        m.release(src("Q"));
        assert_eq!(m.tick(), Report::default());
    }

    #[test]
    fn diagonal_is_a_unit_circle_when_normalised() {
        let mut m = mk("", WASD);
        m.press(src("W"));
        m.press(src("D"));
        let r = m.tick();
        let (x, y) = (f64::from(r.thumb_lx), f64::from(r.thumb_ly));
        assert!((x.hypot(y) - 32767.0).abs() < 2.0, "{x} {y}");
        assert!((x - y).abs() < 1.0);
    }

    #[test]
    fn diagonal_is_a_square_corner_when_not_normalised() {
        let mut m = mk("normalize_diagonals = false", WASD);
        m.press(src("W"));
        m.press(src("D"));
        let r = m.tick();
        assert_eq!((r.thumb_lx, r.thumb_ly), (32767, 32767));
    }

    #[test]
    fn stick_directions_have_correct_signs() {
        let mut m = mk("", WASD);
        m.press(src("W"));
        let r = m.tick();
        assert_eq!((r.thumb_lx, r.thumb_ly), (0, 32767));
        m.release(src("W"));
        m.press(src("S"));
        assert_eq!(m.tick().thumb_ly, -32767);
        m.release(src("S"));
        m.press(src("A"));
        assert_eq!(m.tick().thumb_lx, -32767);
    }

    #[test]
    fn socd_last_wins_and_hands_back() {
        let mut m = mk("socd = \"last\"", WASD);
        m.press(src("A"));
        m.press(src("D"));
        assert_eq!(m.tick().thumb_lx, 32767, "D pressed last");
        m.release(src("D"));
        assert_eq!(m.tick().thumb_lx, -32767, "A still held");
        m.press(src("D"));
        assert_eq!(m.tick().thumb_lx, 32767);
        m.release(src("A"));
        assert_eq!(m.tick().thumb_lx, 32767);
    }

    #[test]
    fn socd_neutral_cancels() {
        let mut m = mk("socd = \"neutral\"", WASD);
        m.press(src("A"));
        m.press(src("D"));
        assert_eq!(m.tick().thumb_lx, 0);
        m.release(src("A"));
        assert_eq!(m.tick().thumb_lx, 32767);
    }

    #[test]
    fn dpad_bits_follow_the_same_rules() {
        let mut m = mk(
            "",
            "DpadUp = \"Up\"\nDpadDown = \"Down\"\nDpadLeft = \"Left\"\nDpadRight = \"Right\"",
        );
        m.press(src("Up"));
        m.press(src("Right"));
        assert_eq!(m.tick().buttons, BTN_DPAD_UP | BTN_DPAD_RIGHT);
        m.press(src("Down"));
        assert_eq!(
            m.tick().buttons,
            BTN_DPAD_DOWN | BTN_DPAD_RIGHT,
            "last of Up/Down wins"
        );
    }

    #[test]
    fn walk_scales_only_the_left_stick() {
        let mut m = mk(
            "walk_scale = 0.5",
            &format!("{WASD}Walk = \"LAlt\"\nRightStickUp = \"I\""),
        );
        m.press(src("LAlt"));
        m.press(src("W"));
        m.press(src("I"));
        let r = m.tick();
        assert_eq!(r.thumb_ly, axis_i(0.5));
        assert_eq!(r.thumb_ry, 32767);
    }

    fn axis_i(v: f64) -> i16 {
        axis_to_i16(v)
    }

    #[test]
    fn ramp_takes_the_configured_time() {
        let mut m = mk("stick_ramp_ms = 10", WASD);
        m.press(src("W"));
        let mut ticks = 0;
        let mut last = 0;
        loop {
            let r = m.tick();
            ticks += 1;
            assert!(r.thumb_ly >= last, "monotonic");
            last = r.thumb_ly;
            if r.thumb_ly == 32767 || ticks > 100 {
                break;
            }
        }
        assert_eq!(ticks, 10, "10 ms at 1000 Hz");
        m.release(src("W"));
        let mut down = 0;
        while m.tick().thumb_ly != 0 {
            down += 1;
            assert!(down <= 20);
        }
        assert!(down >= 8, "release also ramps (took {down})");
    }

    #[test]
    fn wheel_notches_are_distinct_queued_presses() {
        let mut m = mk("wheel_pulse_ms = 10", "A = \"WheelUp\"");
        for _ in 0..3 {
            m.wheel(WheelDir::Up);
        }
        let mut edges = 0;
        let mut was = false;
        for _ in 0..200 {
            let on = m.tick().buttons & BTN_A != 0;
            if on && !was {
                edges += 1;
            }
            was = on;
        }
        assert_eq!(edges, 3);
        assert!(!was);
    }

    #[test]
    fn wheel_press_length_matches_config() {
        let mut m = mk("wheel_pulse_ms = 20", "A = \"WheelDown\"");
        m.wheel(WheelDir::Down);
        let mut on_ticks = 0;
        for _ in 0..100 {
            if m.tick().buttons & BTN_A != 0 {
                on_ticks += 1;
            }
        }
        assert_eq!(on_ticks, 20);
    }

    #[test]
    fn unbound_wheel_is_ignored() {
        let mut m = mk("", "A = \"WheelUp\"");
        m.wheel(WheelDir::Down);
        for _ in 0..50 {
            assert_eq!(m.tick(), Report::default());
        }
    }

    #[test]
    fn reset_neutralises_everything() {
        let mut m = mk("", &format!("{WASD}A = \"Space\"\nB = \"WheelUp\""));
        m.press(src("W"));
        m.press(src("Space"));
        m.wheel(WheelDir::Up);
        m.add_mouse(500, 500);
        m.tick();
        m.reset();
        for _ in 0..50 {
            assert_eq!(m.tick(), Report::default());
        }
        // and a release arriving after the reset must not underflow anything
        m.release(src("W"));
        m.press(src("W"));
        assert_eq!(m.tick().thumb_ly, 32767);
    }

    #[test]
    fn mouse_sign_conventions() {
        let mut m = mk("", "");
        // right and down on the mouse -> stick right and DOWN (negative Y)
        m.add_mouse(100, 100);
        let r = (0..20).map(|_| m.tick()).fold((0i64, 0i64), |a, r| {
            (a.0 + i64::from(r.thumb_rx), a.1 + i64::from(r.thumb_ry))
        });
        assert!(r.0 > 0, "x {r:?}");
        assert!(r.1 < 0, "y {r:?}");
    }

    #[test]
    fn invert_flips_the_axis() {
        let text = "[general]\noutput_hz = 1000\n[mouse]\nsmoothing_ms = 4\n\
                    [axis.mouse_y]\ntarget = \"RightStickY\"\nfull_deflection_cps = 1000\ninvert = true\n";
        let cfg = Config::parse(text).unwrap();
        let mut m = Mapper::new(&cfg, Arc::new(cfg.bindings.clone()));
        m.add_mouse(0, 100);
        let sum: i64 = (0..20).map(|_| i64::from(m.tick().thumb_ry)).sum();
        assert!(sum > 0, "inverted: mouse down looks up");
    }

    #[test]
    fn mouse_axes_can_be_swapped_across_sticks() {
        let text = "[general]\noutput_hz = 1000\n[mouse]\nsmoothing_ms = 4\n\
                    [axis.mouse_x]\ntarget = \"LeftStickY\"\nfull_deflection_cps = 1000\n\
                    [axis.mouse_y]\ntarget = \"RightStickX\"\nfull_deflection_cps = 1000\n";
        let cfg = Config::parse(text).unwrap();
        let mut m = Mapper::new(&cfg, Arc::new(cfg.bindings.clone()));
        m.add_mouse(100, 0);
        let r = (0..20).map(|_| m.tick()).fold((0i64, 0i64, 0i64), |a, r| {
            (
                a.0 + i64::from(r.thumb_lx),
                a.1 + i64::from(r.thumb_ly),
                a.2 + i64::from(r.thumb_rx),
            )
        });
        assert_eq!(r.0, 0);
        assert!(r.1 > 0, "mouse X drove left stick Y");
        assert_eq!(r.2, 0);
    }

    #[test]
    fn keyboard_and_mouse_on_one_stick_combine_within_the_unit_circle() {
        let mut m = mk("", "RightStickRight = \"L\"");
        m.press(src("L"));
        m.add_mouse(100_000, 100_000);
        for _ in 0..30 {
            let r = m.tick();
            let mag = f64::from(r.thumb_rx).hypot(f64::from(r.thumb_ry));
            assert!(mag <= 32767.0 + 1.5, "{mag}");
        }
    }

    #[test]
    fn disabled_mouse_axes_report_nothing() {
        let cfg = Config::parse("[bindings]\nA = \"Space\"\n").unwrap();
        let mut m = Mapper::new(&cfg, Arc::new(cfg.bindings.clone()));
        assert!(!m.has_mouse_axes());
        m.add_mouse(1000, 1000);
        assert_eq!(m.tick(), Report::default());
    }
}
