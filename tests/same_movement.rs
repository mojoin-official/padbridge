//! End-to-end check of the headline requirement: mouse motion mapped to a stick must produce the
//! same camera movement the mouse itself would have.
//!
//! The "game" here is a model of a typical XInput title: it applies an inner radial deadzone and
//! a power-law response curve to the stick, multiplies by a turn rate, and integrates that every
//! frame. The mouse's own displacement is the ground truth. The real [`Engine`] sits in between.

use padbridge::config::Config;
use padbridge::engine::Engine;
use padbridge::pad::Report;

const HZ: usize = 1000;
const DT: f64 = 1.0 / HZ as f64;

struct Game {
    deadzone: f64,
    exponent: f64,
    /// Mouse-count equivalents per second at full, post-deadzone stick (what the user calibrated).
    cps: f64,
}

impl Game {
    /// Camera velocity (count-equivalents per second) for one right-stick sample.
    fn velocity(&self, r: &Report) -> (f64, f64) {
        let x = f64::from(r.thumb_rx) / 32767.0;
        let y = f64::from(r.thumb_ry) / 32767.0;
        let m = x.hypot(y);
        if m <= self.deadzone {
            return (0.0, 0.0);
        }
        let speed = ((m - self.deadzone) / (1.0 - self.deadzone))
            .min(1.0)
            .powf(self.exponent);
        (x / m * speed * self.cps, y / m * speed * self.cps)
    }

    /// The game integrates at the output rate (idealised, no sampling effects).
    fn integrate_every_tick(&self, reports: &[Report]) -> (f64, f64) {
        reports.iter().fold((0.0, 0.0), |a, r| {
            let v = self.velocity(r);
            (a.0 + v.0 * DT, a.1 + v.1 * DT)
        })
    }

    /// The game samples the stick once per frame and holds it for the frame (like a real game).
    fn integrate_at_fps(&self, reports: &[Report], fps: f64, phase: f64) -> (f64, f64) {
        let frame = 1.0 / fps;
        let total = reports.len() as f64 * DT;
        let (mut x, mut y) = (0.0, 0.0);
        let mut t = phase * frame;
        while t < total {
            let v = self.velocity(&reports[((t / DT) as usize).min(reports.len() - 1)]);
            x += v.0 * frame;
            y += v.1 * frame;
            t += frame;
        }
        (x, y)
    }
}

fn config(smoothing_ms: f64, deadzone: f64, exponent: f64, cps: f64, backlog_ms: f64) -> Config {
    Config::parse(&format!(
        "[general]\noutput_hz = {HZ}\n\
         [mouse]\nsmoothing_ms = {smoothing_ms}\ndeadzone = {deadzone}\n\
         curve_exponent = {exponent}\nmax_backlog_ms = {backlog_ms}\n\
         [axis.mouse_x]\ntarget = \"RightStickX\"\nfull_deflection_cps = {cps}\n\
         [axis.mouse_y]\ntarget = \"RightStickY\"\nfull_deflection_cps = {cps}\n"
    ))
    .unwrap()
}

/// Feeds `motion(tick) -> (dx, dy)` for `ticks` ticks, then lets the stick drain for `tail`
/// more. Returns every report plus the true mouse displacement in counts.
fn run(
    cfg: &Config,
    ticks: usize,
    tail: usize,
    motion: impl Fn(usize) -> (i64, i64),
) -> (Vec<Report>, (f64, f64)) {
    let mut engine = Engine::new(cfg);
    let (mut sx, mut sy) = (0i64, 0i64);
    let mut reports = Vec::with_capacity(ticks + tail);
    for t in 0..ticks + tail {
        let (dx, dy) = if t < ticks { motion(t) } else { (0, 0) };
        sx += dx;
        sy += dy;
        reports.push(engine.step(DT, dx, dy));
    }
    (reports, (sx as f64, sy as f64))
}

fn rel_err(got: f64, want: f64) -> f64 {
    ((got - want) / want).abs()
}

#[test]
fn steady_motion_matches_to_a_fraction_of_a_percent() {
    let cps = 8000.0;
    let game = Game {
        deadzone: 0.2652,
        exponent: 1.0,
        cps,
    };
    let cfg = config(10.0, 0.2652, 1.0, cps, 300.0);
    // 4 counts every tick = 4000 counts/s = half of full deflection, for 200 ms
    let (reports, truth) = run(&cfg, 200, 100, |_| (4, 0));
    let (gx, gy) = game.integrate_every_tick(&reports);
    println!("steady: mouse {truth:?}  game ({gx:.2}, {gy:.2})");
    assert!(rel_err(gx, truth.0) < 0.005, "x: {gx} vs {}", truth.0);
    assert!(gy.abs() < 1.0, "no spurious y motion: {gy}");
}

#[test]
fn deadzone_and_response_curve_are_cancelled() {
    for &(dz, p) in &[
        (0.0, 1.0),
        (0.24, 1.0),
        (0.2652, 2.0),
        (0.1, 1.5),
        (0.35, 0.7),
    ] {
        let cps = 6000.0;
        let game = Game {
            deadzone: dz,
            exponent: p,
            cps,
        };
        let cfg = config(8.0, dz, p, cps, 300.0);
        let (reports, truth) = run(&cfg, 300, 100, |t| (if t % 2 == 0 { 3 } else { 2 }, 0));
        let (gx, _) = game.integrate_every_tick(&reports);
        println!("dz {dz} p {p}: mouse {:.0} game {gx:.1}", truth.0);
        assert!(
            rel_err(gx, truth.0) < 0.01,
            "dz {dz} p {p}: {gx} vs {}",
            truth.0
        );
    }
}

#[test]
fn slow_precise_movement_is_not_swallowed_by_the_games_deadzone() {
    let cps = 8000.0;
    let game = Game {
        deadzone: 0.2652,
        exponent: 1.0,
        cps,
    };
    // 1 count every 4 ms = 250 counts/s, about 3 % of full deflection: far inside a naive deadzone
    let cfg = config(16.0, 0.2652, 1.0, cps, 300.0);
    let (reports, truth) = run(&cfg, 2000, 100, |t| (i64::from(t % 4 == 0), 0));
    let (gx, _) = game.integrate_every_tick(&reports);
    println!("slow: mouse {:.0} game {gx:.1}", truth.0);
    assert!(rel_err(gx, truth.0) < 0.02, "{gx} vs {}", truth.0);

    // Control: without compensation the same motion is lost to the game's deadzone.
    let naive = config(16.0, 0.0, 1.0, cps, 300.0);
    let (reports, truth) = run(&naive, 2000, 100, |t| (i64::from(t % 4 == 0), 0));
    let (lost, _) = game.integrate_every_tick(&reports);
    assert!(
        lost < truth.0 * 0.01,
        "uncompensated motion should be eaten by the deadzone: {lost}"
    );
}

#[test]
fn diagonal_motion_keeps_its_direction_and_both_distances() {
    let cps = 8000.0;
    let game = Game {
        deadzone: 0.2,
        exponent: 1.0,
        cps,
    };
    let cfg = config(10.0, 0.2, 1.0, cps, 300.0);
    // mouse moves right and UP (dy negative) in a 3:2 ratio
    let (reports, truth) = run(&cfg, 300, 100, |_| (3, -2));
    let (gx, gy) = game.integrate_every_tick(&reports);
    println!("diag: mouse {truth:?}  game ({gx:.1}, {gy:.1})");
    assert!(rel_err(gx, truth.0) < 0.01);
    // mouse up == camera up == stick +Y: the game sees +y where the mouse moved -y
    assert!(rel_err(gy, -truth.1) < 0.01, "{gy} vs {}", -truth.1);
}

#[test]
fn flick_faster_than_the_stick_can_express_is_delayed_not_lost() {
    let cps = 8000.0;
    let game = Game {
        deadzone: 0.2652,
        exponent: 1.0,
        cps,
    };
    // 3000 counts in 50 ms = 60000 counts/s, 7.5x faster than full deflection
    let cfg = config(10.0, 0.2652, 1.0, cps, 1000.0);
    let (reports, truth) = run(&cfg, 50, 600, |_| (60, 0));
    let (gx, _) = game.integrate_every_tick(&reports);
    let peak = reports.iter().map(|r| r.thumb_rx).max().unwrap();
    println!("flick: mouse {:.0} game {gx:.1} peak stick {peak}", truth.0);
    assert_eq!(peak, 32767, "stick should saturate during the flick");
    assert!(rel_err(gx, truth.0) < 0.005, "{gx} vs {}", truth.0);
}

#[test]
fn backlog_cap_bounds_how_long_a_flick_keeps_turning() {
    let cps = 8000.0;
    let cfg = config(10.0, 0.0, 1.0, cps, 100.0);
    let (reports, truth) = run(&cfg, 50, 1000, |_| (60, 0));
    let game = Game {
        deadzone: 0.0,
        exponent: 1.0,
        cps,
    };
    let (gx, _) = game.integrate_every_tick(&reports);
    // The stick turns at full speed for the 50 ms of the flick itself, then replays at most the
    // 100 ms backlog: 8000 counts/s * 0.150 s = 1200 counts, to within one 10 ms window.
    assert!(
        gx < truth.0 * 0.5,
        "excess must be dropped: {gx} of {}",
        truth.0
    );
    assert!((gx - cps * 0.150).abs() <= cps * 0.010, "{gx}");
    // After the hand stops the stick keeps turning for no longer than backlog + window.
    let after_hand_stops = reports[50..].iter().filter(|r| r.thumb_rx != 0).count();
    assert!(
        after_hand_stops <= 100 + 10 + 2,
        "{after_hand_stops} ticks of lag"
    );
    assert_eq!(
        reports.last().unwrap().thumb_rx,
        0,
        "stick returns to exactly centre"
    );
}

#[test]
fn human_like_motion_at_60fps_and_144fps() {
    // A smooth swing with hand tremor: speed ramps up and down like a real aim adjustment.
    let cps = 8000.0;
    let cfg_for = |smoothing| config(smoothing, 0.2652, 1.5, cps, 300.0);
    let game = Game {
        deadzone: 0.2652,
        exponent: 1.5,
        cps,
    };
    let mut seed = 99u64;
    let mut noise = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f64) / ((1u64 << 31) as f64) - 0.5
    };
    let n = 1200usize;
    let motion: Vec<(i64, i64)> = (0..n)
        .map(|t| {
            let phase = t as f64 / n as f64 * std::f64::consts::PI;
            let speed = 5.0 * phase.sin().powi(2); // counts per tick
            let jitter = noise();
            (
                (speed + jitter).round() as i64,
                (0.4 * speed - jitter).round() as i64,
            )
        })
        .collect();

    for (fps, smoothing) in [(60.0, 16.0), (144.0, 7.0)] {
        let cfg = cfg_for(smoothing);
        let (reports, truth) = run(&cfg, n, 150, |t| motion[t]);
        let mut worst: f64 = 0.0;
        for phase in [0.0, 0.25, 0.5, 0.75] {
            let (gx, gy) = game.integrate_at_fps(&reports, fps, phase);
            worst = worst.max(rel_err(gx, truth.0)).max(rel_err(gy, -truth.1));
            println!("{fps} fps phase {phase}: mouse {truth:?} game ({gx:.1}, {gy:.1})");
        }
        assert!(
            worst < 0.02,
            "{fps} fps: worst error {:.2} %",
            worst * 100.0
        );
    }
}

#[test]
fn an_unbound_axis_produces_no_motion() {
    let cfg =
        Config::parse("[axis.mouse_x]\ntarget = \"RightStickX\"\nfull_deflection_cps = 8000\n")
            .unwrap();
    let (reports, _) = run(&cfg, 200, 50, |_| (5, 5));
    assert!(reports
        .iter()
        .all(|r| r.thumb_ry == 0 && r.thumb_ly == 0 && r.thumb_lx == 0));
    assert!(reports.iter().any(|r| r.thumb_rx != 0));
}

#[test]
fn faster_hand_speed_scales_linearly_below_saturation() {
    // Doubling the mouse speed must double the camera speed (linear game, no deadzone).
    let cps = 8000.0;
    let game = Game {
        deadzone: 0.0,
        exponent: 1.0,
        cps,
    };
    let cfg = config(10.0, 0.0, 1.0, cps, 300.0);
    let speed_of = |per_tick: i64| {
        let (reports, _) = run(&cfg, 100, 50, move |_| (per_tick, 0));
        game.velocity(&reports[60]).0
    };
    let (a, b) = (speed_of(2), speed_of(4));
    assert!(rel_err(b, a * 2.0) < 0.001, "{a} -> {b}");
    assert!(
        rel_err(a, 2000.0) < 0.001,
        "2 counts/ms = 2000 counts/s, got {a}"
    );
}
