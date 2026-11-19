//! padbridge-core: the platform-independent engine behind padbridge.
//!
//! Pipeline, per output tick:
//!
//! ```text
//!  raw mouse counts ──► MouseToStick ──► effective look rate (what the GAME should do)
//!                                             │
//!                                   GameCurve::inverse + StickQuantizer (sigma-delta,
//!                                   in the game's effective space, at the target's
//!                                   stick resolution)
//!                                             │
//!  keys ──► DigitalStick / buttons ──────► PadState ──► OutputMode encoder ──► backend
//!                                                         (XInput, DS4/PlayStation,
//!                                                          Switch Pro, G-Touch)
//! ```
//!
//! Accuracy model: the mouse produces a *displacement* (counts). A stick produces a
//! *rate* (the game turns at `effective * max_turn_speed`). The engine keeps a ledger of
//! counts owed to the game and releases them as a rate, so the total camera rotation
//! matches the mouse even when a flick exceeds the game's top stick speed, when the
//! stick resolution is coarse (DS4 = 8 bit), or when the game has a deadzone and curve.

pub mod curve;
pub mod engine;
pub mod keyboard;
pub mod modes;
pub mod mouse;
pub mod pad;
pub mod quantize;
pub mod sim;
pub mod touch;

#[cfg(all(windows, feature = "vigem"))]
pub mod vigem;

pub use curve::{DeadzoneShape, GameCurve};
pub use engine::{Engine, EngineConfig, InputFrame};
pub use keyboard::{DigitalStick, SocdMode};
pub use modes::{OutputMode, Report};
pub use mouse::{Calibration, MouseToStick};
pub use pad::{Buttons, PadState};
pub use quantize::{Resolution, StickQuantizer};
pub use touch::{TouchContact, TouchEmulator};
