//! Where finished controller reports go.

use crate::pad::Report;
use std::time::{Duration, Instant};

pub trait PadSink {
    fn submit(&mut self, report: &Report) -> Result<(), String>;
}

/// `--dry-run`: prints what would be sent instead of talking to a driver. Button/trigger changes
/// print immediately; stick-only changes are throttled so fast mouse motion does not flood the
/// console.
pub struct PrintSink {
    last: Report,
    last_print: Option<Instant>,
}

impl PrintSink {
    pub fn new() -> PrintSink {
        PrintSink {
            last: Report::default(),
            last_print: None,
        }
    }
}

impl Default for PrintSink {
    fn default() -> Self {
        Self::new()
    }
}

pub fn format_report(r: &Report) -> String {
    format!(
        "buttons=0x{:04X} LT={:<3} RT={:<3} L=({:>6},{:>6}) R=({:>6},{:>6})",
        r.buttons, r.left_trigger, r.right_trigger, r.thumb_lx, r.thumb_ly, r.thumb_rx, r.thumb_ry
    )
}

impl PadSink for PrintSink {
    fn submit(&mut self, r: &Report) -> Result<(), String> {
        let discrete_changed = r.buttons != self.last.buttons
            || r.left_trigger != self.last.left_trigger
            || r.right_trigger != self.last.right_trigger;
        let due = self
            .last_print
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(100));
        if *r != self.last && (discrete_changed || due) {
            println!("{}", format_report(r));
            self.last_print = Some(Instant::now());
        }
        self.last = *r;
        Ok(())
    }
}

/// Connects to the ViGEmBus driver and plugs in a virtual wired Xbox 360 controller.
#[cfg(windows)]
pub fn connect_vigem() -> Result<Box<dyn PadSink>, String> {
    use vigem_client::{Client, TargetId, XButtons, XGamepad, Xbox360Wired};

    struct ViGEmSink {
        pad: Xbox360Wired<Client>,
    }

    impl PadSink for ViGEmSink {
        fn submit(&mut self, r: &Report) -> Result<(), String> {
            let gamepad = XGamepad {
                buttons: XButtons { raw: r.buttons },
                left_trigger: r.left_trigger,
                right_trigger: r.right_trigger,
                thumb_lx: r.thumb_lx,
                thumb_ly: r.thumb_ly,
                thumb_rx: r.thumb_rx,
                thumb_ry: r.thumb_ry,
            };
            self.pad
                .update(&gamepad)
                .map_err(|e| format!("virtual controller update failed: {e:?}"))
        }
    }

    impl Drop for ViGEmSink {
        fn drop(&mut self) {
            // Unplug explicitly so the controller disappears from games immediately.
            let _ = self.pad.unplug();
        }
    }

    let client = Client::connect().map_err(|e| {
        format!(
            "cannot open the ViGEmBus driver ({e:?}). Is ViGEmBus installed? \
             See the README, section \"Requirements\"."
        )
    })?;
    let mut pad = Xbox360Wired::new(client, TargetId::XBOX360_WIRED);
    pad.plugin()
        .map_err(|e| format!("could not plug in the virtual controller: {e:?}"))?;
    pad.wait_ready()
        .map_err(|e| format!("the virtual controller never became ready: {e:?}"))?;
    Ok(Box::new(ViGEmSink { pad }))
}

#[cfg(not(windows))]
pub fn connect_vigem() -> Result<Box<dyn PadSink>, String> {
    Err("the virtual controller driver (ViGEmBus) is only available on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_sink_accepts_everything() {
        let mut s = PrintSink::new();
        for i in 0..10 {
            let r = Report {
                thumb_rx: i,
                ..Report::default()
            };
            assert!(s.submit(&r).is_ok());
        }
    }

    #[test]
    fn report_formatting_is_stable() {
        let r = Report {
            buttons: 0x1000,
            right_trigger: 255,
            thumb_rx: -5,
            ..Report::default()
        };
        assert_eq!(
            format_report(&r),
            "buttons=0x1000 LT=0   RT=255 L=(     0,     0) R=(    -5,     0)"
        );
    }
}
