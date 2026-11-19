# padbridge-core 0.2

Standalone engine produced by the daily scheduled task (19 Nov 2026). It is **not wired into
the padbridge app yet**; it lives here so it builds and tests in CI.

What it adds:

- **Controller modes**: report encoders for Xbox 360 (XInput), DualShock 4 / PlayStation,
  Switch (Nintendo button layout over XInput, or raw Switch Pro `0x30` HID reports) and a
  GameSir G-Touch style touch emulator.
- **Mouse to stick**: a ledger of owed mouse counts plus a quantizer that inverts the game's
  deadzone and response curve (`GameCurve`) and picks the stick code the game turns closest
  to, so 8-bit DS4 sticks don't add rounding bias.
- `src/vigem.rs` (feature `vigem`, Windows only): ViGEmBus Xbox 360 and DS4 output.

Run `cargo test -p padbridge-core` and `cargo run -p padbridge-core --release --example accuracy`
for the simulator comparison. The "old" method in that table is a plain 16 ms velocity window,
not the app's current mouse code (which already carries owed motion with catch-up), so it
overstates the gain over today's padbridge. Real games add their own smoothing; calibrate per game.

ViGEmBus can't emulate a DualSense, a Switch Pro controller or touch input, so on Windows only
the XInput, DS4/PlayStation and Switch-over-XInput modes can reach a game.
