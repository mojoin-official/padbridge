//! Access to the Interception keyboard/mouse filter driver, loaded at run time from
//! `interception.dll` (next to the executable). With it, padbridge sees input *below* Windows:
//! anything it does not send back never reaches any program, including games that read the mouse
//! through Raw Input or DirectInput.
//!
//! Without the driver (or the DLL) [`Driver::open`] fails and padbridge falls back to hooks.

use crate::strokes::{StrokeBuf, FILTER_ALL};
use crate::sys::*;
use std::ffi::c_void;
use std::fmt;

type Ctx = *mut c_void;
type FnCreate = unsafe extern "C" fn() -> Ctx;
type FnDestroy = unsafe extern "C" fn(Ctx);
type FnPred = unsafe extern "C" fn(i32) -> i32;
type FnSetFilter = unsafe extern "C" fn(Ctx, Option<FnPred>, u16);
type FnWait = unsafe extern "C" fn(Ctx, u32) -> i32;
type FnIo = unsafe extern "C" fn(Ctx, i32, *mut StrokeBuf, u32) -> i32;

/// Why the driver cannot be used.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DriverError {
    /// `interception.dll` is not next to padbridge (or is not a valid Interception library).
    DllMissing,
    /// The DLL loaded but the driver is not installed (or the PC was not restarted after
    /// installing it).
    NotInstalled,
    /// Not on Windows.
    Unsupported,
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DriverError::DllMissing => "interception.dll was not found next to padbridge.exe",
            DriverError::NotInstalled => {
                "the Interception driver is not installed (or the PC has not been restarted since)"
            }
            DriverError::Unsupported => "the Interception driver only exists on Windows",
        })
    }
}

pub struct Driver {
    lib: HMODULE,
    ctx: Ctx,
    destroy: FnDestroy,
    set_filter: FnSetFilter,
    wait: FnWait,
    send: FnIo,
    receive: FnIo,
    is_keyboard: FnPred,
    is_mouse: FnPred,
}

impl Driver {
    /// Loads the library and opens a context. Input is not intercepted until
    /// [`capture_all`](Self::capture_all).
    pub fn open() -> Result<Driver, DriverError> {
        if !cfg!(windows) {
            return Err(DriverError::Unsupported);
        }
        unsafe {
            let name = wide("interception.dll");
            let lib = LoadLibraryW(name.as_ptr());
            if lib.is_null() {
                return Err(DriverError::DllMissing);
            }
            macro_rules! sym {
                ($name:literal, $t:ty) => {{
                    let p = GetProcAddress(lib, concat!($name, "\0").as_ptr());
                    if p.is_null() {
                        FreeLibrary(lib);
                        return Err(DriverError::DllMissing);
                    }
                    std::mem::transmute::<*const c_void, $t>(p)
                }};
            }
            let create = sym!("interception_create_context", FnCreate);
            let destroy = sym!("interception_destroy_context", FnDestroy);
            let set_filter = sym!("interception_set_filter", FnSetFilter);
            let wait = sym!("interception_wait_with_timeout", FnWait);
            let send = sym!("interception_send", FnIo);
            let receive = sym!("interception_receive", FnIo);
            let is_keyboard = sym!("interception_is_keyboard", FnPred);
            let is_mouse = sym!("interception_is_mouse", FnPred);
            let ctx = create();
            if ctx.is_null() {
                FreeLibrary(lib);
                return Err(DriverError::NotInstalled);
            }
            Ok(Driver {
                lib,
                ctx,
                destroy,
                set_filter,
                wait,
                send,
                receive,
                is_keyboard,
                is_mouse,
            })
        }
    }

    /// Starts intercepting every keyboard and mouse event. From here on, an event not passed to
    /// [`send`](Self::send) is gone.
    pub fn capture_all(&self) {
        unsafe {
            (self.set_filter)(self.ctx, Some(self.is_keyboard), FILTER_ALL);
            (self.set_filter)(self.ctx, Some(self.is_mouse), FILTER_ALL);
        }
    }

    /// Waits up to `ms` for an event; returns its device number, or 0 on timeout.
    pub fn wait(&self, ms: u32) -> i32 {
        unsafe { (self.wait)(self.ctx, ms) }
    }

    pub fn receive(&self, device: i32, buf: &mut StrokeBuf) -> bool {
        unsafe { (self.receive)(self.ctx, device, buf, 1) > 0 }
    }

    /// Passes an event on to Windows.
    pub fn send(&self, device: i32, buf: &StrokeBuf) {
        let mut copy = *buf;
        unsafe {
            (self.send)(self.ctx, device, &mut copy, 1);
        }
    }

    pub fn is_keyboard(&self, device: i32) -> bool {
        unsafe { (self.is_keyboard)(device) != 0 }
    }

    pub fn is_mouse(&self, device: i32) -> bool {
        unsafe { (self.is_mouse)(device) != 0 }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // Destroying the context stops interception, so input flows normally again even if we
        // are unwinding from a bug.
        unsafe {
            (self.destroy)(self.ctx);
            FreeLibrary(self.lib);
        }
    }
}

/// Whether the driver can be used right now (for the app's status display).
pub fn status() -> Result<(), DriverError> {
    Driver::open().map(drop)
}
