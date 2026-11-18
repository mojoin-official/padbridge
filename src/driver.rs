//! Access to the Interception keyboard/mouse filter driver, loaded at run time from
//! `interception.dll` (next to the executable). With it, padbridge sees input *below* Windows:
//! anything it does not send back never reaches any program, including games that read the mouse
//! through Raw Input or DirectInput.
//!
//! Without the driver (or the DLL) [`Driver::open`] fails with a precise reason and padbridge
//! falls back to hooks.

use crate::strokes::{StrokeBuf, FILTER_ALL};
use crate::sys::*;
use std::ffi::c_void;
use std::fmt;
use std::path::PathBuf;

type Ctx = *mut c_void;
type FnCreate = unsafe extern "C" fn() -> Ctx;
type FnDestroy = unsafe extern "C" fn(Ctx);
type FnPred = unsafe extern "C" fn(i32) -> i32;
type FnSetFilter = unsafe extern "C" fn(Ctx, Option<FnPred>, u16);
type FnWait = unsafe extern "C" fn(Ctx, u32) -> i32;
type FnIo = unsafe extern "C" fn(Ctx, i32, *mut StrokeBuf, u32) -> i32;

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_MOD_NOT_FOUND: u32 = 126;
const ERROR_BAD_EXE_FORMAT: u32 = 193;

/// Why the driver cannot be used, precise enough to tell the user what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverError {
    /// `interception.dll` could not be loaded. `path` is where padbridge looked first.
    DllMissing { path: String, code: u32 },
    /// The DLL is there but is the 32-bit (x86) build; padbridge needs the x64 one.
    DllWrongArch { path: String },
    /// The DLL loaded but is not the Interception library.
    DllInvalid { path: String },
    /// The driver's files are not in Windows' drivers folder: it was never installed.
    NotInstalled,
    /// The driver files are installed but the driver is not running: the PC has not been
    /// restarted since installing, or Windows refused to load it.
    NotRunning { code: u32 },
    /// Not on Windows.
    Unsupported,
}

impl DriverError {
    /// One short status line.
    pub fn short(&self) -> &'static str {
        match self {
            DriverError::DllMissing { .. } => "interception.dll not found",
            DriverError::DllWrongArch { .. } => "wrong interception.dll (32-bit)",
            DriverError::DllInvalid { .. } => "interception.dll is not valid",
            DriverError::NotInstalled => "driver not installed",
            DriverError::NotRunning { .. } => "driver installed but not running",
            DriverError::Unsupported => "Windows only",
        }
    }

    /// What to do about it.
    pub fn advice(&self) -> String {
        match self {
            DriverError::DllMissing { path, code } => format!(
                "Put interception.dll in the same folder as padbridge.exe. Looked for: {path} \
                 (Windows error {code}). It is in the padbridge download, and in Interception.zip \
                 under library\\x64."
            ),
            DriverError::DllWrongArch { path } => format!(
                "{path} is the 32-bit version. Replace it with the one from Interception.zip \
                 under library\\x64."
            ),
            DriverError::DllInvalid { path } => format!(
                "{path} loaded but does not look like Interception's library. Replace it with \
                 the one from Interception.zip under library\\x64."
            ),
            DriverError::NotInstalled => "Install it: download Interception.zip from \
                 github.com/oblitum/Interception/releases, open Command Prompt as \
                 administrator in its 'command line installer' folder, run \
                 install-interception.exe /install, then restart the PC."
                .to_string(),
            DriverError::NotRunning { code } => format!(
                "The driver is installed but Windows has not started it (error {code}). \
                 Restart the PC if you have not since installing. If it still shows this after \
                 a restart, Windows may be refusing to load it: look for a 'A driver can't load \
                 on this device' notification, or check Windows Security > Device security > \
                 Core isolation."
            ),
            DriverError::Unsupported => "The Interception driver only exists on Windows.".into(),
        }
    }
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.short(), self.advice())
    }
}

fn dll_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("interception.dll")))
        .unwrap_or_else(|| PathBuf::from("interception.dll"))
}

fn driver_files_present() -> bool {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let dir = PathBuf::from(root).join("System32").join("drivers");
    // Interception installs these two filter drivers (Windows' own are kbdclass/mouclass).
    dir.join("keyboard.sys").is_file() && dir.join("mouse.sys").is_file()
}

/// Opens the first Interception device directly, for its error code.
fn device_open_error() -> u32 {
    let name = wide("\\\\.\\interception00");
    unsafe {
        let h = CreateFileW(
            name.as_ptr(),
            0x8000_0000,
            0,
            std::ptr::null_mut(),
            3,
            0,
            std::ptr::null_mut(),
        );
        if h.is_null() || h as isize == -1 {
            GetLastError()
        } else {
            CloseHandle(h);
            0
        }
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
        let path = dll_path();
        let shown = path.display().to_string();
        unsafe {
            // Load from padbridge's own folder explicitly; fall back to the normal search.
            let full = wide(&shown);
            let mut lib = LoadLibraryW(full.as_ptr());
            let mut code = if lib.is_null() { GetLastError() } else { 0 };
            if lib.is_null() && code == ERROR_BAD_EXE_FORMAT {
                return Err(DriverError::DllWrongArch { path: shown });
            }
            if lib.is_null() {
                lib = LoadLibraryW(wide("interception.dll").as_ptr());
                if lib.is_null() {
                    let c = GetLastError();
                    if c == ERROR_BAD_EXE_FORMAT {
                        return Err(DriverError::DllWrongArch { path: shown });
                    }
                    if code == 0 || code == ERROR_MOD_NOT_FOUND {
                        code = c;
                    }
                    return Err(DriverError::DllMissing { path: shown, code });
                }
            }
            macro_rules! sym {
                ($name:literal, $t:ty) => {{
                    let p = GetProcAddress(lib, concat!($name, "\0").as_ptr());
                    if p.is_null() {
                        FreeLibrary(lib);
                        return Err(DriverError::DllInvalid { path: shown });
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
                return Err(if driver_files_present() {
                    let c = device_open_error();
                    DriverError::NotRunning {
                        code: if c == 0 { ERROR_FILE_NOT_FOUND } else { c },
                    }
                } else {
                    DriverError::NotInstalled
                });
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

    /// Reads up to `buf.len()` pending events of one device; returns how many were read.
    pub fn receive(&self, device: i32, buf: &mut [StrokeBuf]) -> usize {
        let n = unsafe { (self.receive)(self.ctx, device, buf.as_mut_ptr(), buf.len() as u32) };
        n.clamp(0, buf.len() as i32) as usize
    }

    /// Passes events on to Windows.
    pub fn send(&self, device: i32, strokes: &mut [StrokeBuf]) {
        if strokes.is_empty() {
            return;
        }
        unsafe {
            (self.send)(self.ctx, device, strokes.as_mut_ptr(), strokes.len() as u32);
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

/// Whether the driver can be used right now (for the app's status display). Safe to call while
/// mapping runs: a second context that sets no filter intercepts nothing.
pub fn status() -> Result<(), DriverError> {
    Driver::open().map(drop)
}
