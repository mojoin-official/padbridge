//! The Windows runtime: low-level hooks (to swallow bound input), Raw Input (for true mouse
//! deltas), the message loop, and the fixed-rate output thread that feeds the virtual pad.
//!
//! [`Runtime`] can be started, stopped and started again in one process, so both the console
//! program and the window app drive it.
//!
//! Threads per running instance:
//! * **mapping** - owns the hidden window, both hooks and the message loop. Hook callbacks run
//!   here and must return quickly (Windows silently removes a hook that stalls), so they only
//!   take a short mutex and never print or block. A GUI thread being busy never affects it.
//! * **output** - ticks at `output_hz`, drains mouse deltas, asks the engine for a report and
//!   submits it. Creates its own ViGEm connection so nothing needs to be `Send`.
//! * **log** - turns notices into text lines and beeps, so slow consumers never stall a hook.

use crate::capture::{Action, Edge};
use crate::config::{Config, InputMethod};
use crate::driver::Driver;
use crate::engine::{describe, Engine};
use crate::keys::{hook_key_id, MouseButton, Source, WheelDir, KEY_PAUSE};
use crate::pad::Report;
use crate::sink::{self, NullSink, PadSink, PrintSink};
use crate::strokes::{KeyDecoder, KeyEvent, MouseStroke, StrokeBuf};
use crate::sys::*;
use std::mem::size_of;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Copy, Clone, Debug, Default)]
pub struct RunOptions {
    /// Do not create a virtual controller; reports are only computed (see `print_reports`).
    pub dry_run: bool,
    /// Only report the name of every key/button pressed; map and swallow nothing.
    pub sniff: bool,
    /// In a dry run, print every report to stdout (console program).
    pub print_reports: bool,
}

/// How input is being read by a running instance.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Backend {
    /// Interception driver: captured input never reaches any program.
    Driver,
    /// Low-level hooks: games that read the mouse directly can still see it.
    Hooks,
}

impl Backend {
    pub fn describe(self) -> &'static str {
        match self {
            Backend::Driver => "Interception driver",
            Backend::Hooks => "Windows hooks",
        }
    }
}

enum Notice {
    Toggled(bool),
    Exiting,
    Sniff(String),
    Warn(String),
    Info(String),
    Stop,
}

struct Shared {
    engine: Mutex<Engine>,
    /// Raw mouse counts accumulated by the mapping thread, drained by the output thread.
    dx: AtomicI64,
    dy: AtomicI64,
    /// Self-diagnostic: mouse-move events the hook swallowed vs. Raw Input packets received.
    /// Swallowing moves while Raw Input delivers nothing means mouse->stick can never work.
    moves_swallowed: AtomicU64,
    raw_packets: AtomicU64,
    /// Tells the output thread to finish.
    stop: AtomicBool,
    /// Tells the input loop to finish (driver mode; hook mode also gets WM_QUIT).
    quit: AtomicBool,
    /// Capture only while this program is in front (empty: always).
    game_exe: String,
    /// True from start until the mapping thread has fully cleaned up.
    running: AtomicBool,
    /// Most recent report, for live previews.
    last_report: Mutex<Report>,
    ignore_injected: bool,
    sniff: bool,
    notices: mpsc::Sender<Notice>,
}

/// The instance the hook callbacks talk to. At most one at a time.
static ACTIVE: RwLock<Option<Arc<Shared>>> = RwLock::new(None);
static CTRL_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static CLEANED_UP: AtomicBool = AtomicBool::new(true);

// Last absolute pointer position, for absolute devices (tablets, remote sessions).
static ABS_VALID: AtomicBool = AtomicBool::new(false);
static ABS_X: AtomicI32 = AtomicI32::new(0);
static ABS_Y: AtomicI32 = AtomicI32::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means another thread panicked; the state is still usable.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn active() -> Option<Arc<Shared>> {
    ACTIVE.read().unwrap_or_else(|e| e.into_inner()).clone()
}

fn announce(sh: &Shared, action: Option<Action>) {
    match action {
        Some(Action::Toggled(on)) => {
            let _ = sh.notices.send(Notice::Toggled(on));
        }
        Some(Action::Exit) => {
            let _ = sh.notices.send(Notice::Exiting);
            sh.quit.store(true, Ordering::Release);
            // Hook callbacks run on the mapping thread, so this ends its message loop.
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
    let Some(sh) = active() else { return false };
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
    dispatch(&sh, Source::Key(id), edge)
}

unsafe fn handle_mouse(wparam: WPARAM, lparam: LPARAM) -> bool {
    let Some(sh) = active() else { return false };
    let info = &*(lparam as *const MSLLHOOKSTRUCT);
    if sh.ignore_injected && info.flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) != 0 {
        return false;
    }
    let button = |b: MouseButton, edge: Edge| dispatch(&sh, Source::Mouse(b), edge);
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
    let Some(sh) = active() else { return };
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
// Driver input (Interception)
// ---------------------------------------------------------------------------------------------

fn wheel_name(vertical: bool, positive: bool) -> WheelDir {
    match (vertical, positive) {
        (true, true) => WheelDir::Up,
        (true, false) => WheelDir::Down,
        (false, true) => WheelDir::Right,
        (false, false) => WheelDir::Left,
    }
}

/// Applies one mouse packet to the engine and removes everything that was captured. Returns
/// whether anything is left to pass on to Windows.
fn filter_mouse(sh: &Shared, m: &mut MouseStroke) -> bool {
    use crate::strokes::*;
    let was_empty = m.state == 0 && m.x == 0 && m.y == 0;
    for (bit, button, edge) in MOUSE_BUTTON_BITS {
        if m.state & bit != 0 && dispatch(sh, Source::Mouse(button), edge) {
            m.state &= !bit;
        }
    }
    for (bit, vertical) in [(MOUSE_WHEEL, true), (MOUSE_HWHEEL, false)] {
        if m.state & bit == 0 {
            continue;
        }
        let delta = i32::from(m.rolling);
        let swallow = if sh.sniff {
            let _ = sh.notices.send(Notice::Sniff(
                Source::Wheel(wheel_name(vertical, delta > 0)).to_string(),
            ));
            false
        } else {
            lock(&sh.engine).on_wheel(vertical, delta)
        };
        if swallow {
            m.state &= !bit;
        }
    }
    if m.state & (MOUSE_WHEEL | MOUSE_HWHEEL) == 0 {
        m.rolling = 0;
    }
    if m.flags & MOUSE_MOVE_ABSOLUTE == 0 && (m.x != 0 || m.y != 0) && !sh.sniff {
        sh.raw_packets.fetch_add(1, Ordering::Relaxed);
        sh.dx.fetch_add(i64::from(m.x), Ordering::AcqRel);
        sh.dy.fetch_add(i64::from(m.y), Ordering::AcqRel);
        if lock(&sh.engine).swallow_motion() {
            m.x = 0;
            m.y = 0;
        }
    }
    was_empty || m.state != 0 || m.x != 0 || m.y != 0
}

/// Reads every keyboard and mouse event from the driver until told to stop, passing on only
/// what is not captured.
fn driver_loop(sh: &Shared, drv: &Driver) {
    let mut keys = KeyDecoder::default();
    let mut pause_swallowed = false;
    let mut buf = StrokeBuf::default();
    while !sh.quit.load(Ordering::Acquire) {
        let device = drv.wait(50);
        if device <= 0 || !drv.receive(device, &mut buf) {
            continue;
        }
        let forward = if drv.is_keyboard(device) {
            match keys.decode(&buf.key()) {
                KeyEvent::Ignore => true,
                KeyEvent::PauseTail(_) => !pause_swallowed,
                KeyEvent::Key(id, edge) => {
                    let swallow = dispatch(sh, Source::Key(id), edge);
                    if id == KEY_PAUSE {
                        pause_swallowed = swallow;
                    }
                    !swallow
                }
            }
        } else if drv.is_mouse(device) {
            let mut m = buf.mouse();
            let keep = filter_mouse(sh, &mut m);
            buf.set_mouse(&m);
            keep
        } else {
            true
        };
        if forward {
            drv.send(device, &buf);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Foreground program
// ---------------------------------------------------------------------------------------------

/// File name of the program whose window is in front (e.g. `game.exe`), if it can be read.
pub fn foreground_program() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(crate::util::file_name(&path).to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// RAII guards (dropped in reverse declaration order)
// ---------------------------------------------------------------------------------------------

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
            return Err(
                "padbridge mapping is already running (another window or console?)".to_string(),
            );
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

/// Publishes the instance to the hook callbacks, and withdraws it on drop.
struct ActiveGuard;
impl ActiveGuard {
    fn publish(shared: &Arc<Shared>) -> Result<ActiveGuard, String> {
        let mut slot = ACTIVE.write().unwrap_or_else(|e| e.into_inner());
        if slot.is_some() {
            return Err("mapping is already running".to_string());
        }
        *slot = Some(Arc::clone(shared));
        Ok(ActiveGuard)
    }
}
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = None;
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

fn logger_loop(rx: mpsc::Receiver<Notice>, out: mpsc::Sender<String>, beep: bool) {
    while let Ok(n) = rx.recv() {
        let line = match n {
            Notice::Toggled(on) => {
                if beep {
                    unsafe { Beep(if on { 880 } else { 440 }, 60) };
                }
                describe(Action::Toggled(on)).to_string()
            }
            Notice::Exiting => describe(Action::Exit).to_string(),
            Notice::Sniff(name) => name,
            Notice::Warn(text) => format!("warning: {text}"),
            Notice::Info(text) => text,
            Notice::Stop => break,
        };
        let _ = out.send(line);
    }
}

/// How long the pad may keep failing before capture is switched off so the user's keyboard and
/// mouse are never left swallowed with nothing on the other end.
const FAILSAFE_AFTER: Duration = Duration::from_millis(1500);

fn output_thread(
    shared: Arc<Shared>,
    hz: u32,
    opts: RunOptions,
    ready: mpsc::Sender<Result<(), String>>,
) {
    let mut sink: Box<dyn PadSink> = if opts.dry_run {
        if opts.print_reports {
            Box::new(PrintSink::new())
        } else {
            Box::new(NullSink)
        }
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
    let mut focus_due = Instant::now();
    let mut focus_last: Option<bool> = None;

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
                     mouse cannot drive the stick on this system. Turn off 'Freeze the cursor' \
                     on the Mouse to stick page and report this."
                        .to_string(),
                ));
            }
        }
        if !shared.game_exe.is_empty() && now >= focus_due {
            focus_due = now + Duration::from_millis(200);
            let in_game = foreground_program()
                .is_some_and(|exe| crate::util::exe_matches(&exe, &shared.game_exe));
            if focus_last != Some(in_game) {
                focus_last = Some(in_game);
                let changed = {
                    let mut engine = lock(&shared.engine);
                    let changed = engine.enabled() != in_game;
                    if changed {
                        engine.set_enabled(in_game);
                    }
                    changed
                };
                if changed {
                    let _ = shared.notices.send(Notice::Info(
                        if in_game {
                            "game in front: capturing"
                        } else {
                            "game not in front: keyboard and mouse released"
                        }
                        .to_string(),
                    ));
                }
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
        *lock(&shared.last_report) = report;

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
    *lock(&shared.last_report) = Report::default();
    let _ = sink.submit(&Report::default());
}

unsafe fn create_window() -> Result<HWND, String> {
    const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;
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
    // A second start in the same process finds the class already registered; that is fine.
    if RegisterClassExW(&wc) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
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

/// Body of the mapping thread. Sends `Ok(thread id)` on `ready` once input is being handled, or
/// `Err` if setup fails. Returns when the message loop ends.
fn run_mapping(
    shared: &Arc<Shared>,
    opts: RunOptions,
    hz: u32,
    method: InputMethod,
    ready: &mut Option<mpsc::Sender<Result<(u32, Backend), String>>>,
) -> Result<(), String> {
    let _instance = SingleInstance::acquire()?;
    let _active = ActiveGuard::publish(shared)?;

    // The virtual controller (not needed to sniff key names).
    let mut _output = None;
    if !opts.sniff {
        let (ready_tx, ready_rx) = mpsc::channel();
        let out_shared = Arc::clone(shared);
        let handle = thread::Builder::new()
            .name("padbridge-output".into())
            .spawn(move || output_thread(out_shared, hz, opts, ready_tx))
            .map_err(|e| format!("could not start the output thread: {e}"))?;
        let guard = OutputGuard {
            shared: Arc::clone(shared),
            handle: Some(handle),
        };
        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("timed out waiting for the virtual controller".to_string()),
        }
        _output = Some(guard);
    }

    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };

    // Preferred: the driver, so captured input never reaches any program.
    let driver = match method {
        InputMethod::Auto => Driver::open().ok(),
        InputMethod::Hooks => None,
    };
    if let Some(drv) = driver {
        drv.capture_all();
        if let Some(r) = ready.take() {
            let _ = r.send(Ok((unsafe { GetCurrentThreadId() }, Backend::Driver)));
        }
        driver_loop(shared, &drv);
        return Ok(()); // dropping `drv` ends interception
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

    if let Some(r) = ready.take() {
        let _ = r.send(Ok((unsafe { GetCurrentThreadId() }, Backend::Hooks)));
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
    // published instance, instance mutex.
    Ok(())
}

/// A running mapping instance. Dropping it stops mapping and unplugs the controller.
pub struct Runtime {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    thread_id: u32,
    backend: Backend,
    logger: Option<JoinHandle<()>>,
    log_rx: mpsc::Receiver<String>,
}

impl Runtime {
    /// Starts mapping with `cfg`. Returns once input is being captured, or with the reason it
    /// could not start (for example, the ViGEmBus driver is missing).
    pub fn start(cfg: &Config, opts: RunOptions) -> Result<Runtime, String> {
        if !cfg!(windows) {
            return Err("padbridge mapping only runs on Windows".to_string());
        }

        let (notice_tx, notice_rx) = mpsc::channel::<Notice>();
        let (log_tx, log_rx) = mpsc::channel::<String>();
        let beep = cfg.general.beep_on_toggle;
        let logger = thread::Builder::new()
            .name("padbridge-log".into())
            .spawn(move || logger_loop(notice_rx, log_tx, beep))
            .map_err(|e| format!("could not start the log thread: {e}"))?;

        let shared = Arc::new(Shared {
            engine: Mutex::new(Engine::new(cfg)),
            dx: AtomicI64::new(0),
            dy: AtomicI64::new(0),
            moves_swallowed: AtomicU64::new(0),
            raw_packets: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            game_exe: cfg.general.game_exe.trim().to_string(),
            running: AtomicBool::new(true),
            last_report: Mutex::new(Report::default()),
            ignore_injected: cfg.general.ignore_injected,
            sniff: opts.sniff,
            notices: notice_tx,
        });

        let stop_logger = |shared: &Shared, logger: JoinHandle<()>| {
            let _ = shared.notices.send(Notice::Stop);
            let _ = logger.join();
        };

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(u32, Backend), String>>();
        let th_shared = Arc::clone(&shared);
        let hz = cfg.general.output_hz;
        let method = cfg.general.input_method;
        CLEANED_UP.store(false, Ordering::Release);
        let thread = match thread::Builder::new()
            .name("padbridge-mapping".into())
            .spawn(move || {
                let mut ready = Some(ready_tx);
                let result = run_mapping(&th_shared, opts, hz, method, &mut ready);
                match (result, ready.take()) {
                    (Err(e), Some(r)) => {
                        let _ = r.send(Err(e));
                    }
                    (Err(e), None) => {
                        let _ = th_shared.notices.send(Notice::Warn(e));
                    }
                    _ => {}
                }
                th_shared.running.store(false, Ordering::Release);
                CLEANED_UP.store(true, Ordering::Release);
            }) {
            Ok(t) => t,
            Err(e) => {
                CLEANED_UP.store(true, Ordering::Release);
                stop_logger(&shared, logger);
                return Err(format!("could not start the mapping thread: {e}"));
            }
        };

        let failure = match ready_rx.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok((tid, backend))) => {
                let rt = Runtime {
                    shared,
                    thread: Some(thread),
                    thread_id: tid,
                    backend,
                    logger: Some(logger),
                    log_rx,
                };
                let what = if opts.sniff {
                    "sniff mode: press keys and mouse buttons to see their names".to_string()
                } else {
                    format!(
                        "running{} using the {}. mapping is {}. toggle: {}   exit: {}",
                        if opts.dry_run {
                            " (test mode, no controller)"
                        } else {
                            ""
                        },
                        backend.describe(),
                        if !cfg.general.game_exe.trim().is_empty() {
                            "on while the game is in front"
                        } else if rt.enabled() {
                            "ON"
                        } else {
                            "off"
                        },
                        cfg.general.toggle_hotkey,
                        cfg.general.exit_hotkey
                    )
                };
                let _ = rt.shared.notices.send(Notice::Info(what));
                return Ok(rt);
            }
            Ok(Err(e)) => e,
            Err(_) => "timed out starting the mapping thread".to_string(),
        };
        let _ = thread.join();
        stop_logger(&shared, logger);
        Err(failure)
    }

    /// False once mapping has ended (exit hotkey, error, or [`stop`](Self::stop)).
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Acquire)
    }

    /// Whether input is currently being captured and mapped.
    pub fn enabled(&self) -> bool {
        lock(&self.shared.engine).enabled()
    }

    /// Turns capture on or off (same as the toggle hotkey).
    pub fn set_enabled(&self, on: bool) {
        lock(&self.shared.engine).set_enabled(on);
        let _ = self.shared.notices.send(Notice::Toggled(on));
    }

    /// The report most recently produced (neutral when stopped or in sniff mode).
    pub fn last_report(&self) -> Report {
        *lock(&self.shared.last_report)
    }

    /// Log lines produced since the previous call.
    pub fn drain_log(&self) -> Vec<String> {
        self.log_rx.try_iter().collect()
    }

    pub fn thread_id(&self) -> u32 {
        self.thread_id
    }

    /// How input is being read.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    /// Stops mapping, releases every key and unplugs the controller. Idempotent.
    pub fn stop(&mut self) {
        if let Some(t) = self.thread.take() {
            self.shared.quit.store(true, Ordering::Release);
            if self.thread_id != 0 {
                unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0) };
            }
            let _ = t.join();
        }
        if let Some(l) = self.logger.take() {
            let _ = self.shared.notices.send(Notice::Stop);
            let _ = l.join();
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------------------------
// Console integration
// ---------------------------------------------------------------------------------------------

unsafe extern "system" fn ctrl_handler(event: u32) -> BOOL {
    match event {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            if let Some(sh) = active() {
                sh.quit.store(true, Ordering::Release);
            }
            let tid = CTRL_THREAD_ID.load(Ordering::Acquire);
            if tid != 0 {
                PostThreadMessageW(tid, WM_QUIT, 0, 0);
            }
            if event != CTRL_C_EVENT && event != CTRL_BREAK_EVENT {
                // Windows ends the process as soon as this handler returns for these events, so
                // give the mapping thread a moment to unhook and unplug first.
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

/// Makes Ctrl+C and closing the console window stop `rt` cleanly (console program only).
pub fn install_console_ctrl_handler(rt: &Runtime) {
    CTRL_THREAD_ID.store(rt.thread_id(), Ordering::Release);
    unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1) };
}
