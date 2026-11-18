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

## The app (padbridge.exe)

Double-click `padbridge.exe`. Pages:

* **Keyboard / Mouse buttons**: click a key or button, then click the controller outputs it
  should drive on the right. Mapped keys are green and show what they do.
* **Controller**: click a controller button to see which keys drive it, remove them, or add one.
* **Mouse to stick**: which stick axis each mouse direction drives, sensitivity, inversion,
  calibration, and the motion-feel settings, with a response graph and a live stick preview.
* **Settings**: hotkeys, unmapped-key behaviour, opposite-direction handling, ramps and more.

Profiles (one per game, say) are saved in a `profiles` folder next to the .exe. **Start** begins
mapping; **Test mode** runs everything except the virtual controller, so you can watch the live
preview without the driver. Edits made while running take effect with **Apply changes**.

Hotkeys (configurable, never reach the game, work even when everything is captured):
**Ctrl+Alt+F10** pauses/resumes capture, **Ctrl+Alt+F12** stops mapping. While capturing, your
mouse drives the stick, so use the hotkey to get the cursor back. Closing the window always
releases everything and unplugs the pad.

## Console version (padbridge-cli.exe)

Same engine, no window, configured by a `bindings.toml` file:

1. `padbridge-cli --write-default` writes `bindings.toml`; `--check` validates it.
2. `padbridge-cli --sniff` prints the name of every key or button you press.
3. `padbridge-cli --dry-run` prints reports instead of creating a controller.
4. `padbridge-cli` for real. Ctrl+C also exits.

## Building

`cargo build --release` produces both programs in `target\release`. Every push to GitHub also
builds them (Actions tab, artifact `padbridge-windows`) and photographs each page of the app to
the `screenshots` branch.

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

* **Games that read the physical mouse/keyboard directly** (Raw Input or DirectInput) see the real
  devices when padbridge uses Windows hooks, and keep flipping their prompts to keyboard and mouse.
  The fix is the **Interception driver**: padbridge then reads input below Windows and anything
  it captures never reaches the game. Install it once:
  1. Download `Interception.zip` from https://github.com/oblitum/Interception/releases.
  2. In its `command line installer` folder, run `install-interception.exe /install` from an
     administrator Command Prompt, then restart the PC.
  3. Keep `interception.dll` next to `padbridge.exe` (the GitHub build includes it).
  Settings > Game and input shows whether it was found. Interception is free for non-commercial
  use. Known driver issue: after many USB unplug/replug cycles in one session, newly plugged
  devices may stop responding until a restart.
* **Capture only in the game**: set the game's program name in Settings > Game and input (or click
  Pick game window). Outside the game the keyboard and mouse work normally; switching to the game
  turns capture on. For the game to see *only* the controller, also set unmapped keys to Block.
* Anti-cheat systems may treat hooks, input drivers and virtual controllers as tampering, and
  some block the Interception driver outright. Do not use this with online games that forbid it.
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
