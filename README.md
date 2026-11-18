# padbridge

Remaps keyboard and mouse input to a **virtual Xbox 360 controller** on Windows, so any game that
reads a gamepad sees one. Every key, mouse button and wheel notch is bindable, and mouse X and Y
are bound **independently** to any stick axis, converted so the camera moves the same distance the
mouse would have moved it.

```
keyboard + mouse --(low-level hooks swallow bound input; Raw Input supplies true deltas)--> engine
engine --(500 Hz reports)--> ViGEmBus virtual Xbox 360 pad --> XInput / GameInput games
```

## Requirements

* Windows 10 1803+ or Windows 11 (the high-resolution timer needs 1803; older systems fall back).
* **ViGEmBus** driver installed. Note: the ViGEm project was retired in 2023 and no longer receives
  updates ([notice](https://docs.nefarius.at/projects/ViGEm/End-of-Life/)); existing releases still
  exist on its GitHub page. I could not confirm behaviour on current Windows 11 builds, so check it.
* To build: Rust **1.82+** with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`).
* Run padbridge **as administrator** if the game runs elevated, otherwise Windows hides that
  window's input from hooks (UIPI).

## Build and run

```
cargo build --release
target\release\padbridge.exe --write-default      # writes bindings.toml next to you
```

First-run checklist, in this order:

1. `padbridge --check` validates `bindings.toml` and prints what it does. Typos are errors with
   a "did you mean" hint; nothing is silently ignored.
2. `padbridge --sniff` prints the name of every key or button you press, to use in the config.
3. `padbridge --dry-run` runs the whole pipeline but prints reports instead of creating a
   controller. **Move the mouse while mapping is on and confirm `R=(...)` values change.** This
   also proves Raw Input works on your machine (see "Not verified" below).
4. `padbridge` for real. Check it in `joy.cpl` or any gamepad tester, then in the game.

Hotkeys (configurable, never reach the game, work even when everything is captured):
**Ctrl+Alt+F10** toggles mapping, **Ctrl+Alt+F12** exits. Ctrl+C in the console also exits.
Toggling or exiting releases everything and unplugs the pad.

## Same movement: how mouse becomes stick

A mouse reports displacement, a stick sets velocity, and the game integrates it. padbridge makes
the integral match: `sum(stick speed x dt) == sum(mouse counts)`.

* Motion is spread over `smoothing_ms` (a boxcar window) so a game sampling once per frame never
  misses a mouse packet. The stick returns to exactly zero afterwards.
* Motion faster than a fully held stick can express is **queued, not dropped**, up to
  `max_backlog_ms`. Direction is preserved.
* The game's inner **deadzone** and **response curve** are inverted (`deadzone`, `curve_exponent`),
  so slow, precise movement is not swallowed and speed is linear.
* Raw Input deltas are used, not cursor positions, so Windows pointer acceleration and screen
  edges do not distort it.
* `[axis.mouse_x]` and `[axis.mouse_y]` each pick a target (`LeftStickX/Y`, `RightStickX/Y`),
  sensitivity, inversion and calibration. Delete a section to leave that axis unbound.

**Calibrate** (per axis if yaw and pitch differ): `full_deflection_cps` is the mouse speed in
counts/second that should equal a fully held stick. Measure two things and let padbridge divide:

* `stick_seconds_per_360`: hold the stick fully over and time a 360 degree turn in the game.
* `counts_per_360`: your normal mouse sensitivity; `DPI x cm_per_360 / 2.54`.

Then set `deadzone` to the game's value (XInput default: 0.265 right stick, 0.240 left) and
`smoothing_ms` to about one game frame (16 at 60 fps, 7 at 144 fps).

## Known limits

* **Games that read the physical mouse/keyboard directly** (Raw Input or DirectInput) still see
  the real devices; a hook cannot hide input from them, which would double the camera movement.
  Hide the physical devices from that game with a tool such as HidHide, whitelisting padbridge.
* Anti-cheat systems may treat hooks and virtual controllers as tampering. Do not use this with
  online games that forbid it.
* Games that apply their own stick **acceleration over time** cannot be matched exactly.
* Opposite keys (A+D) follow `socd`; keyboard diagonals are normalised to a circle by default.
* Wheel notches become short presses (`wheel_pulse_ms`), one press per notch, queued.

## Not verified

Honest status of what was and was not exercised before you got this:

* **Verified (Linux, rustc 1.97):** all portable logic: 94 unit tests and 9 end-to-end tests,
  with clippy and rustfmt clean on that build (the `cfg(windows)` code is not covered by either).
  The end-to-end tests drive the real engine against a model game
  with a deadzone, response curve and 60/144 fps sampling: displacement error is under 0.5 percent
  for steady motion and under 0.3 percent for varied human-like motion, and flicks faster than the
  stick can express are preserved. The Win32 FFI layer also type-checks against stubs, and its
  struct sizes and offsets are asserted at compile time.
* **Type-checked only:** the ViGEm glue, against a mock built from the vigem-client 0.1.x
  source signatures.
* **Never run:** the Windows build itself, the hooks, Raw Input, and the actual virtual device.
  The `.github/workflows/ci.yml` builds and tests on `windows-latest` so you get a real compile
  immediately.
* **Assumed, not confirmed:** that Raw Input keeps delivering mouse deltas while the hook swallows
  cursor movement (consistent with reports that hooks do not affect Raw Input readers, but I found
  no authoritative statement). padbridge warns at runtime if it sees the opposite. And that
  GameInput enumerates the virtual pad: Microsoft documents XUSB as an XInput protocol that
  GameInput exposes as a gamepad, but virtual devices are not mentioned.

## Layout

```
src/keys.rs      physical inputs (scan-code based) and names
src/pad.rs       controller outputs, XUSB report
src/config.rs    strict TOML-subset parser and schema
src/mouse.rs     mouse -> stick stage (window, backlog, deadzone/curve inversion)
src/mapper.rs    digital inputs, SOCD, ramps, wheel notches -> report
src/capture.rs   swallow-or-pass decisions and hotkeys (no stuck keys)
src/engine.rs    ties them together behind one lock
src/sys.rs       hand-written Win32 FFI (+ stubs elsewhere)
src/sink.rs      ViGEm output and dry-run output
src/app.rs       hooks, Raw Input, message loop, output thread
tests/           end-to-end same-movement tests
```

Fail-safes: a hook callback can never panic into Windows (`catch_unwind`); if the virtual pad stops
responding for 1.5 s, mapping switches itself off so your keyboard and mouse are never left
swallowed; key and button presses/releases are always kept balanced across toggles.
