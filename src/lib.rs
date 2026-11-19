//! padbridge: remaps keyboard and mouse input to a virtual Xbox 360 (XInput) controller.
//!
//! Layout:
//! * Portable core (no OS calls, fully unit-tested on any platform):
//!   [`keys`], [`pad`], [`bindings`], [`config`], [`mouse`], [`mapper`], [`capture`], [`engine`].
//! * Windows layer: [`sys`] (hand-written Win32 FFI), [`sink`] (ViGEm output), [`app`]
//!   (hooks, Raw Input, message loop, output thread).

pub mod app;
pub mod bindings;
pub mod capture;
pub mod config;
pub mod diag;
pub mod driver;
pub mod engine;
pub mod keys;
pub mod layout;
pub mod mapper;
pub mod mouse;
pub mod pad;
pub mod profiles;
pub mod sink;
pub mod sound;
pub mod strokes;
pub mod sys;
pub mod util;
