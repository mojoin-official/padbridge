//! Minimal hand-written Win32 FFI: exactly the calls padbridge makes, nothing else.
//!
//! On non-Windows targets every function exists as a stub with the same signature, so the whole
//! crate (including the runtime in `app.rs`) type-checks and its portable parts can be unit
//! tested anywhere. The stubs are never reached at runtime because `app::run` refuses to start
//! off Windows.
//!
//! Struct layouts are asserted against their documented 64-bit sizes at compile time.

#![allow(
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    dead_code,
    clippy::upper_case_acronyms
)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HWND = HANDLE;
pub type HHOOK = HANDLE;
pub type HINSTANCE = HANDLE;
pub type HMODULE = HANDLE;
pub type HRAWINPUT = HANDLE;
pub type BOOL = i32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;

pub type WNDPROC = Option<unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>;
pub type HOOKPROC = Option<unsafe extern "system" fn(i32, WPARAM, LPARAM) -> LRESULT>;
pub type PHANDLER_ROUTINE = Option<unsafe extern "system" fn(u32) -> BOOL>;

// ---- messages -------------------------------------------------------------------------------
pub const WM_QUIT: u32 = 0x0012;
pub const WM_INPUT: u32 = 0x00FF;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONDOWN: u32 = 0x0204;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_MBUTTONDOWN: u32 = 0x0207;
pub const WM_MBUTTONUP: u32 = 0x0208;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_XBUTTONDOWN: u32 = 0x020B;
pub const WM_XBUTTONUP: u32 = 0x020C;
pub const WM_MOUSEHWHEEL: u32 = 0x020E;

// ---- hooks ----------------------------------------------------------------------------------
pub const WH_KEYBOARD_LL: i32 = 13;
pub const WH_MOUSE_LL: i32 = 14;
pub const HC_ACTION: i32 = 0;
pub const LLKHF_EXTENDED: u32 = 0x01;
pub const LLKHF_INJECTED: u32 = 0x10;
pub const LLMHF_INJECTED: u32 = 0x01;
pub const LLMHF_LOWER_IL_INJECTED: u32 = 0x02;
pub const XBUTTON1: u32 = 0x0001;
pub const XBUTTON2: u32 = 0x0002;

// ---- raw input ------------------------------------------------------------------------------
pub const RIDEV_INPUTSINK: u32 = 0x0000_0100;
pub const RID_INPUT: u32 = 0x1000_0003;
pub const RIM_TYPEMOUSE: u32 = 0;
pub const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
pub const HID_USAGE_PAGE_GENERIC: u16 = 0x01;
pub const HID_USAGE_GENERIC_MOUSE: u16 = 0x02;

// ---- misc -----------------------------------------------------------------------------------
pub const WS_OVERLAPPED: u32 = 0;
pub const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x0000_0002;
pub const TIMER_ALL_ACCESS: u32 = 0x001F_0003;
pub const THREAD_PRIORITY_HIGHEST: i32 = 2;
pub const THREAD_PRIORITY_TIME_CRITICAL: i32 = 15;
pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const CTRL_C_EVENT: u32 = 0;
pub const CTRL_BREAK_EVENT: u32 = 1;
pub const CTRL_CLOSE_EVENT: u32 = 2;
pub const CTRL_LOGOFF_EVENT: u32 = 5;
pub const CTRL_SHUTDOWN_EVENT: u32 = 6;

// ---- structs --------------------------------------------------------------------------------
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
}

impl MSG {
    pub fn zeroed() -> MSG {
        MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT::default(),
        }
    }
}

#[repr(C)]
pub struct KBDLLHOOKSTRUCT {
    pub vkCode: u32,
    pub scanCode: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct MSLLHOOKSTRUCT {
    pub pt: POINT,
    pub mouseData: u32,
    pub flags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
pub struct RAWINPUTDEVICE {
    pub usUsagePage: u16,
    pub usUsage: u16,
    pub dwFlags: u32,
    pub hwndTarget: HWND,
}

#[repr(C)]
pub struct RAWINPUTHEADER {
    pub dwType: u32,
    pub dwSize: u32,
    pub hDevice: HANDLE,
    pub wParam: WPARAM,
}

/// `RAWMOUSE`. The C union `{ ULONG ulButtons; struct { USHORT usButtonFlags, usButtonData; } }`
/// is flattened to a `u32` because padbridge reads buttons from the low-level hook instead.
#[repr(C)]
pub struct RAWMOUSE {
    pub usFlags: u16,
    pub _pad: u16,
    pub ulButtons: u32,
    pub ulRawButtons: u32,
    pub lLastX: i32,
    pub lLastY: i32,
    pub ulExtraInformation: u32,
}

/// `RAWINPUT` as seen for mouse packets (header followed by the mouse member of the union; the
/// union's largest member is `RAWMOUSE`, so this is the full size of a mouse packet).
#[repr(C)]
pub struct RAWINPUT {
    pub header: RAWINPUTHEADER,
    pub mouse: RAWMOUSE,
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: WNDPROC,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HANDLE,
    pub hCursor: HANDLE,
    pub hbrBackground: HANDLE,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HANDLE,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    use std::mem::{offset_of, size_of};
    assert!(size_of::<POINT>() == 8);
    assert!(size_of::<MSG>() == 48);
    assert!(size_of::<KBDLLHOOKSTRUCT>() == 24);
    assert!(size_of::<MSLLHOOKSTRUCT>() == 32);
    assert!(size_of::<RAWINPUTDEVICE>() == 16);
    assert!(size_of::<RAWINPUTHEADER>() == 24);
    assert!(size_of::<RAWMOUSE>() == 24);
    assert!(size_of::<RAWINPUT>() == 48);
    assert!(size_of::<WNDCLASSEXW>() == 80);
    assert!(offset_of!(RAWMOUSE, lLastX) == 12);
    assert!(offset_of!(RAWMOUSE, lLastY) == 16);
    assert!(offset_of!(RAWINPUT, mouse) == 24);
    assert!(offset_of!(MSLLHOOKSTRUCT, mouseData) == 8);
    assert!(offset_of!(KBDLLHOOKSTRUCT, dwExtraInfo) == 16);
};

// ---- functions ------------------------------------------------------------------------------
macro_rules! api {
    ($(#[$meta:meta])* { $( fn $name:ident($($arg:ident : $ty:ty),* $(,)?) $(-> $ret:ty)? ; )* }) => {
        #[cfg(windows)]
        $(#[$meta])*
        extern "system" {
            $( pub fn $name($($arg: $ty),*) $(-> $ret)?; )*
        }
        $(
            #[cfg(not(windows))]
            #[allow(clippy::missing_safety_doc, clippy::too_many_arguments)]
            pub unsafe fn $name($(_: $ty),*) $(-> $ret)? {
                unimplemented!(concat!(stringify!($name), " is only available on Windows"))
            }
        )*
    };
}

api! {
    #[link(name = "user32")]
    {
        fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
        fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: HWND,
            menu: HANDLE,
            instance: HINSTANCE,
            param: *mut c_void,
        ) -> HWND;
        fn DefWindowProcW(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
        fn DestroyWindow(hwnd: HWND) -> BOOL;
        fn GetMessageW(msg: *mut MSG, hwnd: HWND, filter_min: u32, filter_max: u32) -> BOOL;
        fn TranslateMessage(msg: *const MSG) -> BOOL;
        fn DispatchMessageW(msg: *const MSG) -> LRESULT;
        fn PostQuitMessage(exit_code: i32);
        fn PostThreadMessageW(thread_id: u32, msg: u32, wparam: WPARAM, lparam: LPARAM) -> BOOL;
        fn SetWindowsHookExW(id: i32, proc_: HOOKPROC, module: HINSTANCE, thread_id: u32) -> HHOOK;
        fn UnhookWindowsHookEx(hook: HHOOK) -> BOOL;
        fn CallNextHookEx(hook: HHOOK, code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
        fn RegisterRawInputDevices(devices: *const RAWINPUTDEVICE, count: u32, size: u32) -> BOOL;
        fn GetRawInputData(
            raw: HRAWINPUT,
            command: u32,
            data: *mut c_void,
            size: *mut u32,
            header_size: u32,
        ) -> u32;
        fn GetForegroundWindow() -> HWND;
        fn SetTimer(hwnd: HWND, id: usize, elapse_ms: u32, func: *const c_void) -> usize;
        fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut u32) -> u32;
    }
}

api! {
    #[link(name = "kernel32")]
    {
        fn GetModuleHandleW(name: *const u16) -> HMODULE;
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentThread() -> HANDLE;
        fn SetThreadPriority(thread: HANDLE, priority: i32) -> BOOL;
        fn CreateWaitableTimerExW(
            attributes: *mut c_void,
            name: *const u16,
            flags: u32,
            access: u32,
        ) -> HANDLE;
        fn SetWaitableTimer(
            timer: HANDLE,
            due_time: *const i64,
            period: i32,
            completion: *mut c_void,
            completion_arg: *mut c_void,
            resume: BOOL,
        ) -> BOOL;
        fn WaitForSingleObject(handle: HANDLE, millis: u32) -> u32;
        fn CloseHandle(handle: HANDLE) -> BOOL;
        fn CreateMutexW(attributes: *mut c_void, initial_owner: BOOL, name: *const u16) -> HANDLE;
        fn GetLastError() -> u32;
        fn SetConsoleCtrlHandler(handler: PHANDLER_ROUTINE, add: BOOL) -> BOOL;
        fn Beep(frequency: u32, duration: u32) -> BOOL;
        fn LoadLibraryW(name: *const u16) -> HMODULE;
        fn GetProcAddress(module: HMODULE, name: *const u8) -> *const c_void;
        fn FreeLibrary(module: HMODULE) -> BOOL;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            security: *mut c_void,
            disposition: u32,
            flags: u32,
            template: HANDLE,
        ) -> HANDLE;
        fn OpenProcess(access: u32, inherit: BOOL, process_id: u32) -> HANDLE;
        fn QueryFullProcessImageNameW(
            process: HANDLE,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> BOOL;
    }
}

pub const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

api! {
    #[link(name = "winmm")]
    {
        fn timeBeginPeriod(period: u32) -> u32;
        fn timeEndPeriod(period: u32) -> u32;
        fn PlaySoundW(sound: *const u16, module: HMODULE, flags: u32) -> BOOL;
    }
}

pub const SND_ASYNC: u32 = 0x0001;
pub const SND_NODEFAULT: u32 = 0x0002;
pub const SND_MEMORY: u32 = 0x0004;

/// UTF-16, NUL-terminated.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_is_nul_terminated_utf16() {
        assert_eq!(wide("a"), vec![0x61, 0]);
        assert_eq!(wide(""), vec![0]);
        assert_eq!(wide("é"), vec![0xE9, 0]);
    }
}
