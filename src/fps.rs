//! Adaptive game frame detection.
//!
//! * **Measured** (padbridge running as administrator): a real-time Windows event trace counts
//!   the frames the program in front presents through DirectX (DXGI for DirectX 10/11/12, D3D9
//!   for DirectX 9) - the same signal frame counters such as PresentMon use.
//! * **Monitor refresh** (otherwise, or for games that present another way such as Vulkan or
//!   OpenGL): the refresh rate of the monitor the program in front is on.
//!
//! The output thread turns the frame rate into the mouse smoothing window (one frame), so every
//! mouse movement is spread over exactly one game frame: nothing is missed, nothing is delayed
//! more than necessary.

use crate::sys::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Event ids that mark one presented frame (from PresentMon's provider definitions).
const DXGI_PROVIDER: GUID = GUID {
    data1: 0xCA11_C036,
    data2: 0x0102,
    data3: 0x4A2D,
    data4: [0xA6, 0xAD, 0xF0, 0x3C, 0xFE, 0xD5, 0xD3, 0xC9],
};
const DXGI_PRESENT_START: u16 = 0x2a;
const DXGI_PRESENT_MPO_START: u16 = 0x37;
const D3D9_PROVIDER: GUID = GUID {
    data1: 0x783A_CA0A,
    data2: 0x790E,
    data3: 0x4D7F,
    data4: [0x84, 0x51, 0xAA, 0x85, 0x05, 0x11, 0xC6, 0xB9],
};
const D3D9_PRESENT_START: u16 = 0x01;

const SESSION_NAME: &str = "padbridge-frame-rate";

/// Lowest and highest frame rates the smoothing follows; outside this the value is clamped.
pub const MIN_FPS: f64 = 30.0;
pub const MAX_FPS: f64 = 500.0;

/// Where a frame-rate figure came from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Measured,
    Refresh,
}

impl Source {
    pub fn describe(self) -> &'static str {
        match self {
            Source::Measured => "measured from the game",
            Source::Refresh => "monitor refresh rate",
        }
    }
}

/// Smoothing window (ms) for a frame rate: exactly one frame, clamped to sensible limits.
pub fn smoothing_for_fps(fps: f64) -> f64 {
    1000.0 / fps.clamp(MIN_FPS, MAX_FPS)
}

/// Exponentially smoothed frame rate, so a single slow or fast second does not swing the
/// smoothing back and forth.
#[derive(Debug, Default)]
pub struct FpsFilter {
    value: Option<f64>,
}

impl FpsFilter {
    pub fn update(&mut self, sample: f64) -> f64 {
        let v = match self.value {
            None => sample,
            // big changes (switching game, menu to gameplay) are followed quickly, small
            // fluctuations slowly
            Some(prev) if (sample - prev).abs() / prev > 0.25 => sample,
            Some(prev) => prev + (sample - prev) * 0.3,
        };
        self.value = Some(v);
        v
    }

    pub fn reset(&mut self) {
        self.value = None;
    }
}

// ---------------------------------------------------------------------------------------------
// Event-trace meter (Windows)
// ---------------------------------------------------------------------------------------------

/// Process whose frames are being counted, and the count since the last read. Global because the
/// trace callback is a plain function.
static TARGET_PID: AtomicU32 = AtomicU32::new(0);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_event(rec: *mut EVENT_RECORD) {
    let Some(r) = rec.as_ref() else { return };
    let frame = (r.ProviderId == DXGI_PROVIDER
        && (r.Id == DXGI_PRESENT_START || r.Id == DXGI_PRESENT_MPO_START))
        || (r.ProviderId == D3D9_PROVIDER && r.Id == D3D9_PRESENT_START);
    if frame && r.ProcessId != 0 && r.ProcessId == TARGET_PID.load(Ordering::Relaxed) {
        FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

#[repr(C)]
struct Props {
    p: EVENT_TRACE_PROPERTIES,
    name: [u16; 64],
}

fn props() -> Box<Props> {
    let mut b = Box::new(Props {
        p: EVENT_TRACE_PROPERTIES {
            Wnode: WNODE_HEADER {
                BufferSize: std::mem::size_of::<Props>() as u32,
                ProviderId: 0,
                HistoricalContext: 0,
                TimeStamp: 0,
                Guid: GUID::default(),
                ClientContext: 1, // QueryPerformanceCounter timestamps
                Flags: WNODE_FLAG_TRACED_GUID,
            },
            BufferSize: 64,
            MinimumBuffers: 4,
            MaximumBuffers: 32,
            MaximumFileSize: 0,
            LogFileMode: EVENT_TRACE_REAL_TIME_MODE,
            FlushTimer: 1,
            EnableFlags: 0,
            AgeLimit: 0,
            NumberOfBuffers: 0,
            FreeBuffers: 0,
            EventsLost: 0,
            BuffersWritten: 0,
            LogBuffersLost: 0,
            RealTimeBuffersLost: 0,
            LoggerThreadId: std::ptr::null_mut(),
            LogFileNameOffset: 0,
            LoggerNameOffset: std::mem::size_of::<EVENT_TRACE_PROPERTIES>() as u32,
        },
        name: [0; 64],
    });
    b.name[0] = 0;
    b
}

fn stop_session() {
    let name = wide(SESSION_NAME);
    let mut p = props();
    unsafe {
        ControlTraceW(0, name.as_ptr(), &mut p.p, EVENT_TRACE_CONTROL_STOP);
    }
}

/// A running frame-rate trace. Dropping it stops the trace.
pub struct FrameMeter {
    reader: Option<JoinHandle<()>>,
    last_read: Instant,
}

impl FrameMeter {
    /// Starts the trace. Fails without administrator rights (Windows error 5) or when Windows
    /// refuses the graphics providers; callers then fall back to the monitor refresh rate.
    pub fn start() -> Result<FrameMeter, String> {
        if !cfg!(windows) {
            return Err("only on Windows".into());
        }
        if SESSION_ACTIVE.swap(true, Ordering::AcqRel) {
            return Err("a frame-rate trace is already running".into());
        }
        let fail = |msg: String| {
            SESSION_ACTIVE.store(false, Ordering::Release);
            Err(msg)
        };
        let name = wide(SESSION_NAME);
        let mut handle = 0u64;
        let mut p = props();
        let mut rc = unsafe { StartTraceW(&mut handle, name.as_ptr(), &mut p.p) };
        if rc == ERROR_ALREADY_EXISTS {
            // left over from a previous run that did not shut down cleanly
            stop_session();
            let mut p2 = props();
            rc = unsafe { StartTraceW(&mut handle, name.as_ptr(), &mut p2.p) };
        }
        if rc != 0 {
            return fail(if rc == 5 {
                "needs administrator rights".to_string()
            } else {
                format!("Windows refused the trace (error {rc})")
            });
        }
        let mut enabled = 0;
        for g in [DXGI_PROVIDER, D3D9_PROVIDER] {
            let r = unsafe {
                EnableTraceEx2(
                    handle,
                    &g,
                    EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                    TRACE_LEVEL_INFORMATION,
                    0,
                    0,
                    0,
                    std::ptr::null(),
                )
            };
            if r == 0 {
                enabled += 1;
            }
        }
        if enabled == 0 {
            stop_session();
            return fail("Windows refused the graphics event providers".to_string());
        }
        // The reader: OpenTrace + ProcessTrace, which blocks until the session is stopped.
        let reader = std::thread::Builder::new()
            .name("padbridge-fps".into())
            .spawn(move || {
                let mut logger = wide(SESSION_NAME);
                let mut lf = EVENT_TRACE_LOGFILEW {
                    LogFileName: std::ptr::null_mut(),
                    LoggerName: logger.as_mut_ptr(),
                    CurrentTime: 0,
                    BuffersRead: 0,
                    ProcessTraceMode: PROCESS_TRACE_MODE_REAL_TIME
                        | PROCESS_TRACE_MODE_EVENT_RECORD,
                    CurrentEvent: [0; 88],
                    LogfileHeader: [0; 35],
                    BufferCallback: std::ptr::null_mut(),
                    BufferSize: 0,
                    Filled: 0,
                    EventsLost: 0,
                    EventRecordCallback: Some(on_event),
                    IsKernelTrace: 0,
                    Context: std::ptr::null_mut(),
                };
                let h = unsafe { OpenTraceW(&mut lf) };
                if h == INVALID_PROCESSTRACE_HANDLE {
                    return;
                }
                unsafe {
                    ProcessTrace(&h, 1, std::ptr::null(), std::ptr::null());
                    CloseTrace(h);
                }
            });
        match reader {
            Ok(r) => Ok(FrameMeter {
                reader: Some(r),
                last_read: Instant::now(),
            }),
            Err(e) => {
                stop_session();
                fail(format!("could not start the trace reader: {e}"))
            }
        }
    }

    /// Frames per second presented by process `pid` since the previous call. Returns `None`
    /// until a meaningful number of frames has been seen (non-DirectX games never qualify).
    pub fn sample(&mut self, pid: u32) -> Option<f64> {
        let now = Instant::now();
        let dt = now.duration_since(self.last_read).as_secs_f64();
        self.last_read = now;
        let frames = FRAMES.swap(0, Ordering::Relaxed);
        let previous = TARGET_PID.swap(pid, Ordering::Relaxed);
        if previous != pid || dt <= 0.0 {
            return None; // counts belonged to another program
        }
        let fps = f64::from(frames) / dt;
        (frames >= 10 && fps >= 15.0).then_some(fps)
    }
}

impl Drop for FrameMeter {
    fn drop(&mut self) {
        stop_session();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        SESSION_ACTIVE.store(false, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------------------------
// Foreground program and monitor refresh (Windows)
// ---------------------------------------------------------------------------------------------

/// Process id of the window in front.
pub fn foreground_pid() -> u32 {
    if !cfg!(windows) {
        return 0;
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return 0;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        pid
    }
}

/// Refresh rate (Hz) of the monitor showing the window in front.
pub fn foreground_refresh_hz() -> Option<f64> {
    if !cfg!(windows) {
        return None;
    }
    unsafe {
        let hwnd = GetForegroundWindow();
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            rcMonitor: [0; 4],
            rcWork: [0; 4],
            dwFlags: 0,
            szDevice: [0; 32],
        };
        let device = if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            info.szDevice.as_ptr()
        } else {
            std::ptr::null()
        };
        let mut mode = DEVMODEW([0; 55]);
        // dmSize (u16 at byte 68) = sizeof(DEVMODEW)
        mode.0[17] = 220;
        if EnumDisplaySettingsW(device, ENUM_CURRENT_SETTINGS, &mut mode) == 0 {
            return None;
        }
        // dmDisplayFrequency: u32 at byte 184
        let hz = mode.0[46];
        (hz > 1).then_some(f64::from(hz))
    }
}

/// Duration between frame-rate checks.
pub const SAMPLE_EVERY: Duration = Duration::from_millis(1000);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_is_one_frame_within_limits() {
        assert!((smoothing_for_fps(60.0) - 16.6667).abs() < 0.001);
        assert!((smoothing_for_fps(144.0) - 6.9444).abs() < 0.001);
        assert_eq!(smoothing_for_fps(5.0), 1000.0 / MIN_FPS);
        assert_eq!(smoothing_for_fps(5000.0), 1000.0 / MAX_FPS);
    }

    #[test]
    fn filter_follows_big_changes_fast_and_small_ones_slowly() {
        let mut f = FpsFilter::default();
        assert_eq!(f.update(144.0), 144.0);
        let v = f.update(140.0);
        assert!(v > 140.0 && v < 144.0, "{v}");
        assert_eq!(f.update(60.0), 60.0, "a jump is taken at once");
        f.reset();
        assert_eq!(f.update(240.0), 240.0);
    }

    #[test]
    fn devmode_word_offsets() {
        // dmSize is the u16 at byte 68 => low half of u32 word 17 (little-endian);
        // dmDisplayFrequency is the u32 at byte 184 => word 46.
        assert_eq!(68 / 4, 17);
        assert_eq!(184 / 4, 46);
    }

    #[test]
    fn provider_guids_match_the_documented_strings() {
        assert_eq!(DXGI_PROVIDER.data1, 0xCA11C036);
        assert_eq!(D3D9_PROVIDER.data4[7], 0xB9);
        assert_eq!(
            (
                DXGI_PRESENT_START,
                DXGI_PRESENT_MPO_START,
                D3D9_PRESENT_START
            ),
            (42, 55, 1)
        );
    }
}
