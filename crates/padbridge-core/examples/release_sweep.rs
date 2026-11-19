//! cargo run --release -p padbridge-core --example release_sweep — pick `release_ms` for a frame rate.
use padbridge_core::curve::{DeadzoneShape, GameCurve};
use padbridge_core::mouse::{Calibration, MouseConfig};
use padbridge_core::quantize::Resolution;
use padbridge_core::sim::{make_trace, run, score, Method, SimSetup};

fn main() {
    println!("| fps | release_ms | flick err % | tracking err % | micro seen % | drift % |");
    println!("|---:|---:|---:|---:|---:|---:|");
    for fps in [30.0, 60.0, 144.0] {
        for rel in [4.0, 8.0, 12.0, 17.0, 25.0, 34.0] {
            let s = SimSetup {
                curve: GameCurve {
                    inner: 0.2,
                    outer: 1.0,
                    exponent: 2.0,
                    shape: DeadzoneShape::Radial,
                },
                res: Resolution::DS4,
                calib: Calibration {
                    counts_per_360: 8000.0,
                    max_turn_deg_s: 180.0,
                },
                fps,
                output_hz: 500.0,
                mouse: MouseConfig {
                    max_carry_ms: 1000.0,
                    release_ms: rel,
                    ..Default::default()
                },
            };
            let tr = make_trace(42, 120, s.calib.full_deflection_cps());
            let r = score(&tr, &run(&s, Method::Accurate, &tr));
            println!(
                "| {fps} | {rel} | {:.2} | {:.2} | {:.0} | {:.3} |",
                r.flick_err_pct, r.track_err_pct, r.micro_seen_pct, r.drift_pct
            );
        }
    }
}
