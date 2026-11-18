//! The Windows runtime: low-level hooks (to swallow bound input), Raw Input (for true mouse
//! deltas), the message loop, and the fixed-rate output thread that feeds the virtual pad.
//!
//! Threads:
//! * **main** - owns the hidden window, both hooks and the message loop. Hook callbacks run here
//!   and must return quickly (Windows silently removes a hook that stalls), so they only take a
//!   short mutex and never print or block.
//! * **output** - ticks at `output_hz`, drains mouse deltas, asks the engine for a report and
//!   submits it. Creates its own ViGEm connection so nothing needs to be `Send`.
//! * **log** - prints notices and beeps, so slow console writes can never stall a hook.

use crate::capture::{Action, Edge};
use crate::config::Config;
use crate::engine::{describe, Engine};
use crate::keys::{hook_key_id, MouseButton, Source};
use crate::pad::Report;
use crate::sink::{self, PadSink, PrintSink};
use crate::sys::*;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Copy, Clone, Debug, Default)]
pub struct RunOptions {
    /// Print reports instead of creating a virtual controller.
    pub dry_run: bool,
    /// Only print the name of every key/button pressed; map and swallow nothing.
    pub sniff: bool,
}

enum Notice {
    Toggled(bool),
    Exiting,
    Sniff(String),
    Warn(String),
    Stop,
}

struct Shared {
    engine: Mutex<Engine>,
    /// Raw mouse counts accumulated by the message thread, drained by the output thread.
    dx: AtomicI64,
    dy: AtomicI64,
    /// Self-diagnostic: mouse-move events the hook swallowed vs. Raw Input packets received.
    /// Swallowing moves while Raw Input delivers nothing means mouse->stick can never work.
    moves_swallowed: AtomicU64,
    raw_packets: AtomicU64,
    stop: AtomicBool,
    ignore_injected: bool,
    sniff: bool,
    notices: mpsc::Sender<Notice>,
}

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static MAIN_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static CLEANED_UP: AtomicBool = AtomicBool::new(false);

// Last absolute pointer position, for absolute devices (tablets, remote sessions).
static ABS_VALID: AtomicBool = AtomicBool::new(false);
static ABS_X: AtomicI32 = AtomicI32::new(0);
static ABS_Y: AtomicI32 = AtomicI32::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means another thread panicked; the engine state is still usable.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn announce(sh: &Shared, action: Option<Action>) {
    match action {
        Some(Action::Toggled(on)) => {
            let _ = sh.notices.send(Notice::Toggled(on));
        }
        Some(Action::Exit) => {
            let _ = sh.notices.send(Notice::Exiting);
            unsafe { PostQuitMessage(0) };
        }
        None => {}
    }
}

/// Common path for key and button edges. Returns true to swallow.
fn dispatch(sh: &Shared, src: Source, edge: Edge) -> bool {
    if sh.sniff {
        if edge == Edge::Down {
            let _ = sh.notices.send(Notice::Sniff(src.to_string()));
        }
        return false;
    }
    let verdict = lock(&sh.engine).on_input(src, edge);
    announce(sh, verdict.action);
    verdict.swallow
}

// ---------------------------------------------------------------------------------------------
// Low-level hooks
// ---------------------------------------------------------------------------------------------

unsafe fn handle_key(wparam: WPARAM, lparam: LPARAM) -> bool {
    let Some(sh) = SHARED.get() else { return false };
    let info = &*(lparam as *const KBDLLHOOKSTRUCT);
    if sh.ignore_injected && info.flags & LLKHF_INJECTED != 0 {
        return false;
    }
    let edge = match wparam as u32 {
        WM_KEYDOWN | WM_SYSKEYDOWN => Edge::Down,
        WM_KEYUP | WM_SYSKEYUP => Edge::Up,
        _ => return false,
    };
    let Some(id) = hook_key_id(info.vkCode, info.scanCode, info.flags) else {
        return false;
    };
    dispatch(sh, Source::Key(id), edge)
}

unsafe fn handle_mouse(wparam: WPARAM, lparam: LPARAM) -> bool {
    let Some(sh) = SHARED.get() else { return false };
    let info = &*(lparam as *const MSLLHOOKSTRUCT);
    if sh.ignore_injected && info.flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) != 0 {
        return false;
    }
    let button = |b: MouseButton, edge: Edge| dispatch(sh, Source::Mouse(b), edge);
    let hi = info.mouseData >> 16;
    match wparam as u32 {
        WM_MOUSEMOVE => {
            let swallow = !sh.sniff && lock(&sh.engine).swallow_motion();
            if swallow {
                sh.moves_swallowed.fetch_add(1, Ordering::Relaxed);
            }
            swallow
        }
        WM_LBUTTONDOWN => button(MouseButton::Left, Edge::Down),
        WM_LBUTTONUP => button(MouseButton::Left, Edge::Up),
        WM_RBUTTONDOWN => button(MouseButton::Right, Edge::Down),
        WM_RBUTTONUP => button(MouseButton::Right, Edge::Up),
        WM_MBUTTONDOWN => button(MouseButton::Middle, Edge::Down),
        WM_MBUTTONUP => button(MouseButton::Middle, Edge::Up),
        w @ (WM_XBUTTONDOWN | WM_XBUTTONUP) => {
            let edge = if w == WM_XBUTTONDOWN {
                Edge::Down
            } else {
                Edge::Up
            };
            match hi {
                XBUTTON1 => button(MouseButton::X1, edge),
                XBUTTON2 => button(MouseButton::X2, edge),
                _ => false,
            }
        }
        w @ (WM_MOUSEWHEEL | WM_MOUSEHWHEEL) => {
            let vertical = w == WM_MOUSEWHEEL;
            let delta = i32::from(hi as u16 as i16);
            if sh.sniff {
                let name = match (vertical, delta > 0) {
                    (true, true) => "WheelUp",
                    (true, false) => "WheelDown",
                    (false, true) => "WheelRight",
                    (false, false) => "WheelLeft",
                };
                let _ = sh.notices.send(Notice::Sniff(name.to_string()));
                return false;
            }
            lock(&sh.engine).on_wheel(vertical, delta)
        }
        _ => false,
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // A panic must never unwind into Windows; if the callback fails, let the event through.
    let swallow = code == HC_ACTION
        && catch_unwind(AssertUnwindSafe(|| handle_key(wparam, lparam))).unwrap_or(false);
    if swallow {
        1
    } else {
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let swallow = code == HC_ACTION
        && catch_unwind(AssertUnwindSafe(|| handle_mouse(wparam, lparam))).unwrap_or(false);
    if swallow {
        1
    } else {
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}

// ---------------------------------------------------------------------------------------------
// Raw Input (true, unaccelerated mouse deltas)
// ---------------------------------------------------------------------------------------------

unsafe fn read_raw_mouse(lparam: LPARAM) {
    let Some(sh) = SHARED.get() else { return };
    // 8-byte aligned scratch space, comfortably larger than a mouse packet (48 bytes on x64).
    let mut buf = [0u64; 8];
    let mut size = size_of::<[u64; 8]>() as u32;
    let copied = GetRawInputData(
        lparam as HRAWINPUT,
        RID_INPUT,
        buf.as_mut_ptr().cast(),
        &mut size,
        size_of::<RAWINPUTHEADER>() as u32,
    );
    if copied == u32::MAX || (copied as usize) < size_of::<RAWINPUT>() {
        return;
    }
    let raw = &*(buf.as_ptr() as *const RAWINPUT);
    if raw.header.dwType != RIM_TYPEMOUSE {
        return;
    }
    sh.raw_packets.fetch_add(1, Ordering::Relaxed);
    let m = &raw.mouse;
    let (dx, dy) = if m.usFlags & MOUSE_MOVE_ABSOLUTE != 0 {
        // Absolute devices report a position; convert to a delta against the previous one.
        let (x, y) = (m.lLastX, m.lLastY);
        let delta = if ABS_VALID.swap(true, Ordering::Relaxed) {
            (
                x - ABS_X.load(Ordering::Relaxed),
                y - ABS_Y.load(Ordering::Relaxed),
            )
        } else {
            (0, 0)
        };
        ABS_X.store(x, Ordering::Relaxed);
        ABS_Y.store(y, Ordering::Relaxed);
        delta
    } else {
        (m.lLastX, m.lLastY)
    };
    if dx != 0 {
        sh.dx.fetch_add(i64::from(dx), Ordering::AcqRel);
    }
    if dy != 0 {
        sh.dy.fetch_add(i64::from(dy), Ordering::AcqRel);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_INPUT {
        let _ = catch_unwind(AssertUnwindSafe(|| read_raw_mouse(lparam)));
    }
    // Required for WM_INPUT as well: DefWindowProc performs the raw-input cleanup.
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ---------------------------------------------------------------------------------------------
// RAII guards (declared in the order they must be released, last declared dropped first)
// ---------------------------------------------------------------------------------------------

struct CleanedFlag;
impl Drop for CleanedFlag {
    fn drop(&mut self) {
        CLEANED_UP.store(true, Ordering::Release);
    }
}

struct SingleInstance(HANDLE);
impl SingleInstance {
    fn acquire() -> Result<SingleInstance, String> {
        let name = wide("Local\\padbridge.single-instance");
        let handle = unsafe { CreateMutexW(null_mut(), 0, name.as_ptr()) };
        if handle.is_null() {
            // Not fatal: running twice is only a convenience guard.
            return Ok(SingleInstance(handle));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe { CloseHandle(handle) };
            return Err("padbridge is already running in this session".to_string());
        }
        Ok(SingleInstance(handle))
    }
}
impl Drop for SingleInstance {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

struct LoggerGuard {
    tx: mpsc::Sender<Notice>,
    handle: Option<JoinHandle<()>>,
}
impl Drop for LoggerGuard {
    fn drop(&mut self) {
        let _ = self.tx.send(Notice::Stop);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

struct OutputGuard {
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}
impl Drop for OutputGuard {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join(); // the output thread sends a neutral report and unplugs the pad
        }
    }
}

struct WindowGuard(HWND);
impl Drop for WindowGuard {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

struct HookGuard(HHOOK);
impl Drop for HookGuard {
    fn drop(&mut self) {
        unsafe { UnhookWindowsHookEx(self.0) };
    }
}

// ---------------------------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------------------------

/// Sleeps until a deadline using a high-resolution waitable timer (Windows 10 1803+), falling
/// back to `timeBeginPeriod(1)` plus `thread::sleep` on older systems.
struct Ticker {
    timer: HANDLE,
    period_set: bool,
}

impl Ticker {
    fn new() -> Ticker {
        let timer = unsafe {
            CreateWaitableTimerExW(
                null_mut(),
                null(),
                CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                TIMER_ALL_ACCESS,
            )
        };
        if timer.is_null() {
            unsafe { timeBeginPeriod(1) };
            Ticker {
                timer,
                period_set: true,
            }
        } else {
            Ticker {
                timer,
                period_set: false,
            }
        }
    }

    fn sleep_until(&self, deadline: Instant) {
        let now = Instant::now();
        if deadline <= now {
            return;
        }
        let remaining = deadline - now;
        if !self.timer.is_null() {
            // Negative due time = relative, in 100 ns units.
            let due = -((remaining.as_nanos() / 100).max(1) as i64);
            let armed = unsafe { SetWaitableTimer(self.timer, &due, 0, null_mut(), null_mut(), 0) };
            if armed != 0 {
                let timeout_ms = (remaining.as_millis() as u32).saturating_add(50);
                unsafe { WaitForSingleObject(self.timer, timeout_ms) };
                return;
            }
        }
        thread::sleep(remaining);
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        unsafe {
            if !self.timer.is_null() {
                CloseHandle(self.timer);
            }
            if self.period_set {
                timeEndPeriod(1);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Threads
// ---------------------------------------------------------------------------------------------

fn logger_loop(rx: mpsc::Receiver<Notice>, beep: bool) {
    while let Ok(n) = rx.recv() {
        match n {
            Notice::Toggled(on) => {
                println!("[padbridge] {}", describe(Action::Toggled(on)));
                if beep {
                    unsafe { Beep(if on { 880 } else { 440 }, 60) };
                }
            }
            Notice::Exiting => println!("[padbridge] {}", describe(Action::Exit)),
            Notice::Sniff(name) => println!("{name}"),
            Notice::Warn(text) => eprintln!("[padbridge] warning: {text}"),
            Notice::Stop => break,
        }
    }
}

/// How long the pad may keep failing before capture is switched off so the user's keyboard and
/// mouse are never left swallowed with nothing on the other end.
const FAILSAFE_AFTER: Duration = Duration::from_millis(1500);

fn output_thread(
    shared: Arc<Shared>,
    hz: u32,
    dry_run: bool,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let mut sink: Box<dyn PadSink> = if dry_run {
        Box::new(PrintSink::new())
    } else {
        match sink::connect_vigem() {
            Ok(s) => s,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        }
    };
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL) };

    let mut sent = Report::default();
    if let Err(e) = sink.submit(&sent) {
        let _ = ready.send(Err(format!(
            "the virtual controller rejected its first report: {e}"
        )));
        return;
    }
    let _ = ready.send(Ok(()));

    let ticker = Ticker::new();
    let period = Duration::from_secs_f64(1.0 / f64::from(hz));
    let mut last_tick = Instant::now();
    let mut next = last_tick + period;
    let mut failing_since: Option<Instant> = None;
    let mut last_warn: Option<Instant> = None;
    let mut raw_check_due = Instant::now() + Duration::from_secs(2);
    let mut raw_warned = false;

    while !shared.stop.load(Ordering::Acquire) {
        ticker.sleep_until(next);
        let now = Instant::now();

        if !raw_warned && now >= raw_check_due {
            raw_check_due = now + Duration::from_secs(2);
            if shared.moves_swallowed.load(Ordering::Relaxed) >= 100
                && shared.raw_packets.load(Ordering::Relaxed) == 0
            {
                raw_warned = true;
                let _ = shared.notices.send(Notice::Warn(
                    "the mouse is being captured but Raw Input is delivering no motion, so the \
                     mouse cannot drive the stick on this system. Set swallow_motion = false in \
                     [mouse] and report this."
                        .to_string(),
                ));
            }
        }
        let elapsed = now.duration_since(last_tick).as_secs_f64();
        last_tick = now;
        next += period;
        if next <= now {
            // We fell behind (the engine replays missed ticks); do not try to catch up in a burst.
            next = now + period;
        }

        let dx = shared.dx.swap(0, Ordering::AcqRel);
        let dy = shared.dy.swap(0, Ordering::AcqRel);
        let report = lock(&shared.engine).step(elapsed, dx, dy);

        if report == sent && failing_since.is_none() {
            continue;
        }
        match sink.submit(&report) {
            Ok(()) => {
                sent = report;
                failing_since = None;
            }
            Err(e) => {
                let since = *failing_since.get_or_insert(now);
                if last_warn.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
                    let _ = shared.notices.send(Notice::Warn(e));
                    last_warn = Some(Instant::now());
                }
                if now.duration_since(since) >= FAILSAFE_AFTER {
                    if lock(&shared.engine).enabled() {
                        lock(&shared.engine).set_enabled(false);
                        let _ = shared.notices.send(Notice::Warn(
                            "the virtual controller is not responding; mapping has been disabled \
                             so your keyboard and mouse work normally"
                                .to_string(),
                        ));
                        let _ = shared.notices.send(Notice::Toggled(false));
                    }
                    next = now + Duration::from_millis(100); // back off while it is down
                }
            }
        }
    }
    let _ = sink.submit(&Report::default());
}

// ---------------------------------------------------------------------------------------------
// Process plumbing
// ---------------------------------------------------------------------------------------------

unsafe extern "system" fn ctrl_handler(event: u32) -> BOOL {
    match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            let tid = MAIN_THREAD_ID.load(Ordering::Acquire);
            if tid != 0 {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
            if event != CTRL_C_EVENT && event != CTRL_BREAK_EVENT {
                // Windows ends the process as soon as this handler returns for these events, so
                // give the main thread a moment to unhook and unplug first.
                for _ in 0..80 {
                    if CLEANED_UP.load(Ordering::Acquire) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
            1
        }
        _ => 0,
    }
}

unsafe fn create_window() -> Result<HWND, String> {
    let instance = GetModuleHandleW(null());
    let class = wide("padbridge.rawinput");
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(wnd_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: instance,
        hIcon: null_mut(),
        hCursor: null_mut(),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: class.as_ptr(),
        hIconSm: null_mut(),
    };
    if RegisterClassExW(&wc) == 0 {
        return Err(format!(
            "RegisterClassExW failed (error {})",
            GetLastError()
        ));
    }
    // A never-shown top-level window: it receives WM_INPUT (with RIDEV_INPUTSINK) even while
    // another application has focus.
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        class.as_ptr(),
        WS_OVERLAPPED,
        0,
        0,
        0,
        0,
        null_mut(),
        null_mut(),
        instance,
        null_mut(),
    );
    if hwnd.is_null() {
        return Err(format!("CreateWindowExW failed (error {})", GetLastError()));
    }
    let device = RAWINPUTDEVICE {
        usUsagePage: HID_USAGE_PAGE_GENERIC,
        usUsage: HID_USAGE_GENERIC_MOUSE,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: hwnd,
    };
    if RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) == 0 {
        let e = GetLastError();
        DestroyWindow(hwnd);
        return Err(format!("RegisterRawInputDevices failed (error {e})"));
    }
    Ok(hwnd)
}

/// Runs padbridge until the exit hotkey, Ctrl+C or console close. Windows only.
pub fn run(cfg: Config, opts: RunOptions) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("padbridge only runs on Windows".to_string());
    }

    let _cleaned = CleanedFlag;
    let _instance = SingleInstance::acquire()?;

    let (notice_tx, notice_rx) = mpsc::channel::<Notice>();
    let beep = cfg.general.beep_on_toggle;
    let logger = thread::Builder::new()
        .name("padbridge-log".into())
        .spawn(move || logger_loop(notice_rx, beep))
        .map_err(|e| format!("could not start the log thread: {e}"))?;
    let _logger = LoggerGuard {
        tx: notice_tx.clone(),
        handle: Some(logger),
    };

    let shared = Arc::new(Shared {
        engine: Mutex::new(Engine::new(&cfg)),
        dx: AtomicI64::new(0),
        dy: AtomicI64::new(0),
        moves_swallowed: AtomicU64::new(0),
        raw_packets: AtomicU64::new(0),
        stop: AtomicBool::new(false),
        ignore_injected: cfg.general.ignore_injected,
        sniff: opts.sniff,
        notices: notice_tx,
    });
    SHARED
        .set(Arc::clone(&shared))
        .map_err(|_| "padbridge was already started in this process".to_string())?;

    // The virtual controller (not needed to sniff key names).
    let mut _output = None;
    if !opts.sniff {
        let (ready_tx, ready_rx) = mpsc::channel();
        let out_shared = Arc::clone(&shared);
        let hz = cfg.general.output_hz;
        let dry = opts.dry_run;
        let handle = thread::Builder::new()
            .name("padbridge-output".into())
            .spawn(move || output_thread(out_shared, hz, dry, ready_tx))
            .map_err(|e| format!("could not start the output thread: {e}"))?;
        let guard = OutputGuard {
            shared: Arc::clone(&shared),
            handle: Some(handle),
        };
        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("timed out waiting for the virtual controller".to_string()),
        }
        _output = Some(guard);
    }

    MAIN_THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::Release);
    unsafe {
        SetConsoleCtrlHandler(Some(ctrl_handler), 1);
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }

    let hwnd = unsafe { create_window() }?;
    let _window = WindowGuard(hwnd);

    let module = unsafe { GetModuleHandleW(null()) };
    let kb = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0) };
    if kb.is_null() {
        return Err(format!(
            "could not install the keyboard hook (error {})",
            unsafe { GetLastError() }
        ));
    }
    let _kb = HookGuard(kb);
    let ms = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) };
    if ms.is_null() {
        return Err(format!(
            "could not install the mouse hook (error {})",
            unsafe { GetLastError() }
        ));
    }
    let _ms = HookGuard(ms);

    if opts.sniff {
        println!("[padbridge] sniff mode: press keys and mouse buttons to see their names. Ctrl+C quits.");
    } else {
        println!(
            "[padbridge] running{}. mapping is {}. toggle: {}   exit: {}",
            if opts.dry_run {
                " (dry run, no controller)"
            } else {
                ""
            },
            if lock(&shared.engine).enabled() {
                "ON"
            } else {
                "off"
            },
            cfg.general.toggle_hotkey,
            cfg.general.exit_hotkey
        );
    }

    let mut msg = MSG::zeroed();
    loop {
        let r = unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) };
        if r == 0 {
            break; // WM_QUIT
        }
        if r < 0 {
            return Err(format!("GetMessageW failed (error {})", unsafe {
                GetLastError()
            }));
        }
        unsafe {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    // Guards now release in reverse order: hooks, window, output (neutral report + unplug),
    // logger, instance mutex.
    Ok(())
}
