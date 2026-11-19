//! cargo run --release -p padbridge-core --example accuracy
//! Compares the old window-velocity method with the new displacement-conserving one,
//! for each controller mode's stick resolution, over several game curves and frame rates.

use padbridge_core::curve::{DeadzoneShape, GameCurve};
use padbridge_core::mouse::{Calibration, MouseConfig};
use padbridge_core::quantize::Resolution;
use padbridge_core::sim::{make_trace, run, score, Method, SimSetup};

fn main() {
    let games = [
        ("linear, no deadzone", GameCurve::default()),
        (
            "radial dz 0.20, exp 2",
            GameCurve {
                inner: 0.2,
                outer: 1.0,
                exponent: 2.0,
                shape: DeadzoneShape::Radial,
            },
        ),
        (
            "axial dz 0.265, exp 1.5, sat 0.95",
            GameCurve {
                inner: 0.265,
                outer: 0.95,
                exponent: 1.5,
                shape: DeadzoneShape::Axial,
            },
        ),
    ];
    let modes = [
        ("XInput 16-bit", Resolution::XINPUT),
        ("DS4/PlayStation 8-bit", Resolution::DS4),
        ("Switch Pro HID 12-bit", Resolution::SWITCH),
    ];
    println!("| game curve | mode | fps | method | flick err % | tracking err % | micro nudges seen % | end drift % |");
    println!("|---|---|---:|---|---:|---:|---:|---:|");
    for (gname, curve) in games {
        for (mname, res) in modes {
            for fps in [60.0, 144.0] {
                let s = SimSetup {
                    curve,
                    res,
                    calib: Calibration {
                        counts_per_360: 8000.0,
                        max_turn_deg_s: 180.0,
                    },
                    fps,
                    output_hz: 500.0,
                    mouse: MouseConfig {
                        max_carry_ms: 1000.0,
                        release_ms: 12.0,
                        ..Default::default()
                    },
                };
                let tr = make_trace(42, 120, s.calib.full_deflection_cps());
                for (label, m) in [
                    ("old (16 ms window)", Method::Legacy { window_ms: 16 }),
                    ("new (ledger)", Method::Accurate),
                ] {
                    let r = score(&tr, &run(&s, m, &tr));
                    println!(
                        "| {gname} | {mname} | {fps} | {label} | {:.2} | {:.2} | {:.0} | {:.3} |",
                        r.flick_err_pct, r.track_err_pct, r.micro_seen_pct, r.drift_pct
                    );
                }
            }
        }
    }
}
