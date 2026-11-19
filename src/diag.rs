//! Interception driver diagnostics: gathers every fact that decides whether the driver can work
//! (files, services, device filters, Windows security features, blocked-driver events) and turns
//! them into one plain verdict with what to do. Also runs the driver's installer elevated.
//!
//! Facts come from standard Windows tools (`sc`, `reg`, `wevtutil`) whose output is parsed by the
//! pure functions below, which are unit-tested on any platform.

use crate::driver::{self, DriverError};
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Service {
    Running,
    Stopped,
    Missing,
    Unknown,
}

impl Service {
    fn text(self) -> &'static str {
        match self {
            Service::Running => "running",
            Service::Stopped => "installed but not running",
            Service::Missing => "not installed",
            Service::Unknown => "unknown",
        }
    }
}

/// Everything the verdict is based on.
#[derive(Clone, Debug)]
pub struct Facts {
    pub status: Result<(), DriverError>,
    pub files_present: bool,
    pub kb_service: Service,
    pub mouse_service: Service,
    /// Whether the keyboard / mouse device classes list Interception's filter (None: unreadable).
    pub kb_filter: Option<bool>,
    pub mouse_filter: Option<bool>,
    pub memory_integrity: Option<bool>,
    pub blocklist: Option<bool>,
    /// A Windows Code Integrity event about Interception's driver files, if any.
    pub ci_event: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub ok: bool,
    pub title: String,
    pub advice: String,
}

// ---------------------------------------------------------------------------------------------
// Parsing (pure)
// ---------------------------------------------------------------------------------------------

/// Reads the state out of `sc query <name>` output.
pub fn parse_sc(out: &str) -> Service {
    let lower = out.to_ascii_lowercase();
    if lower.contains("1060") || lower.contains("does not exist") {
        return Service::Missing;
    }
    for line in lower.lines() {
        if line.trim_start().starts_with("state") {
            if line.contains("running") {
                return Service::Running;
            }
            if line.contains("stopped") || line.contains("pending") {
                return Service::Stopped;
            }
        }
    }
    Service::Unknown
}

/// Whether `name` is in the `UpperFilters` list printed by `reg query ... /v UpperFilters`.
pub fn parse_upper_filters(out: &str, name: &str) -> Option<bool> {
    let line = out.lines().find(|l| l.contains("REG_MULTI_SZ"))?;
    let values = line.split("REG_MULTI_SZ").nth(1)?;
    Some(
        values
            .split(|c: char| c == '\0' || c.is_whitespace())
            .flat_map(|t| t.split("\\0"))
            .any(|t| t.trim().eq_ignore_ascii_case(name)),
    )
}

/// Reads a `REG_DWORD` value printed by `reg query ... /v <value>`.
pub fn parse_reg_dword(out: &str, value: &str) -> Option<u32> {
    let line = out
        .lines()
        .find(|l| l.contains("REG_DWORD") && l.trim_start().starts_with(value))?;
    let hex = line.split_whitespace().last()?;
    u32::from_str_radix(hex.trim_start_matches("0x"), 16).ok()
}

/// Finds the first Code Integrity event (text output of `wevtutil qe ... /f:text`) that mentions
/// Interception's driver files, and returns its description.
pub fn find_ci_event(out: &str) -> Option<String> {
    for block in out.split("Event[") {
        let lower = block.to_ascii_lowercase();
        if !(lower.contains("\\keyboard.sys") || lower.contains("\\mouse.sys")) {
            continue;
        }
        let mut desc = String::new();
        let mut in_desc = false;
        for line in block.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("Description:") {
                in_desc = true;
                desc.push_str(rest.trim());
            } else if in_desc && !t.is_empty() {
                desc.push(' ');
                desc.push_str(t);
            }
        }
        let desc = if desc.is_empty() {
            "Windows Code Integrity reported a problem with keyboard.sys / mouse.sys".to_string()
        } else {
            desc
        };
        return Some(desc.chars().take(400).collect());
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Verdict (pure)
// ---------------------------------------------------------------------------------------------

const INSTALL_STEPS: &str = "Use 'Install driver' below (or run install-interception.exe /install \
     from an administrator Command Prompt), then restart the PC.";

pub fn conclude(f: &Facts) -> Verdict {
    let v = |ok: bool, title: &str, advice: String| Verdict {
        ok,
        title: title.to_string(),
        advice,
    };
    match &f.status {
        Ok(()) => {
            return v(
                true,
                "The Interception driver is working",
                "padbridge will use it automatically (Input method: Driver if installed). \
                 Captured input never reaches the game."
                    .to_string(),
            )
        }
        Err(DriverError::Unsupported) => {
            return v(
                false,
                "Not on Windows",
                "The driver only exists on Windows.".into(),
            )
        }
        Err(
            e @ (DriverError::DllMissing { .. }
            | DriverError::DllWrongArch { .. }
            | DriverError::DllInvalid { .. }),
        ) => return v(false, "padbridge cannot load interception.dll", e.advice()),
        Err(_) => {}
    }
    let services_missing = f.kb_service == Service::Missing && f.mouse_service == Service::Missing;
    if !f.files_present && services_missing {
        return v(
            false,
            "The driver is not installed",
            INSTALL_STEPS.to_string(),
        );
    }
    let filters_missing = f.kb_filter == Some(false) || f.mouse_filter == Some(false);
    if !f.files_present || services_missing || filters_missing {
        return v(
            false,
            "The driver installation is incomplete",
            format!(
                "Some parts are missing (files present: {}, filters registered: {}). The \
                 installer probably ran without administrator rights. Uninstall, restart, then \
                 install again as administrator and restart. {INSTALL_STEPS}",
                yes_no(f.files_present),
                yes_no(!filters_missing),
            ),
        );
    }
    let running = f.kb_service == Service::Running && f.mouse_service == Service::Running;
    if !running {
        if let Some(ev) = &f.ci_event {
            return v(
                false,
                "Windows blocked the driver from loading",
                format!(
                    "Windows reported: \"{ev}\". This usually comes from Memory integrity \
                     (Windows Security > Device security > Core isolation) or the vulnerable \
                     driver blocklist. Turning those off lets the driver load but lowers your \
                     PC's protection against malicious drivers; that is your call. Otherwise \
                     padbridge keeps working with Windows hooks."
                ),
            );
        }
        if f.memory_integrity == Some(true) {
            return v(
                false,
                "The driver is installed but not running (Memory integrity is on)",
                "If you have already restarted since installing, Memory integrity is the likely \
                 reason: it refuses drivers that don't meet its rules. You can turn it off in \
                 Windows Security > Device security > Core isolation (this lowers protection \
                 against malicious drivers), or keep using Windows hooks."
                    .to_string(),
            );
        }
        return v(
            false,
            "The driver is installed but not running yet",
            "Restart the PC. Windows only starts keyboard and mouse filter drivers at boot."
                .to_string(),
        );
    }
    v(
        false,
        "The driver is running but padbridge cannot open it",
        format!(
            "{} Try running padbridge as administrator, and close other programs that use \
             Interception.",
            f.status.as_ref().err().map(|e| e.short()).unwrap_or("")
        ),
    )
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// The full report, for showing and copying.
pub fn report(f: &Facts) -> String {
    let opt = |o: Option<bool>| match o {
        Some(true) => "yes",
        Some(false) => "no",
        None => "unknown",
    };
    let v = conclude(f);
    let mut s = String::new();
    s.push_str(&format!("Verdict: {}\n{}\n\n", v.title, v.advice));
    s.push_str(&format!(
        "padbridge status: {}\n",
        match &f.status {
            Ok(()) => "ready".to_string(),
            Err(e) => e.short().to_string(),
        }
    ));
    s.push_str(&format!(
        "Driver files (keyboard.sys, mouse.sys): {}\n",
        yes_no(f.files_present)
    ));
    s.push_str(&format!(
        "Keyboard filter service: {}\n",
        f.kb_service.text()
    ));
    s.push_str(&format!(
        "Mouse filter service: {}\n",
        f.mouse_service.text()
    ));
    s.push_str(&format!(
        "Keyboard class filter registered: {}\n",
        opt(f.kb_filter)
    ));
    s.push_str(&format!(
        "Mouse class filter registered: {}\n",
        opt(f.mouse_filter)
    ));
    s.push_str(&format!(
        "Memory integrity on: {}\n",
        opt(f.memory_integrity)
    ));
    s.push_str(&format!(
        "Vulnerable driver blocklist on: {}\n",
        opt(f.blocklist)
    ));
    s.push_str(&format!(
        "Windows blocked-driver event: {}\n",
        f.ci_event.as_deref().unwrap_or("none found")
    ));
    s
}

// ---------------------------------------------------------------------------------------------
// Gathering (Windows)
// ---------------------------------------------------------------------------------------------

fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let mut s = String::from_utf8_lossy(&out.stdout).into_owned();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(s)
}

const KEYBOARD_CLASS: &str =
    "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e96b-e325-11ce-bfc1-08002be10318}";
const MOUSE_CLASS: &str =
    "HKLM\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e96f-e325-11ce-bfc1-08002be10318}";

/// Collects all facts. Takes about a second (it runs a few Windows tools), so call it off the UI
/// thread.
pub fn gather() -> Facts {
    let status = driver::status();
    if !cfg!(windows) {
        return Facts {
            status,
            files_present: false,
            kb_service: Service::Unknown,
            mouse_service: Service::Unknown,
            kb_filter: None,
            mouse_filter: None,
            memory_integrity: None,
            blocklist: None,
            ci_event: None,
        };
    }
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let drivers = PathBuf::from(root).join("System32").join("drivers");
    let files_present =
        drivers.join("keyboard.sys").is_file() && drivers.join("mouse.sys").is_file();
    let sc =
        |name: &str| run("sc.exe", &["query", name]).map_or(Service::Unknown, |o| parse_sc(&o));
    let filters = |key: &str, name: &str| {
        run("reg.exe", &["query", key, "/v", "UpperFilters"])
            .and_then(|o| parse_upper_filters(&o, name))
    };
    let dword = |key: &str, value: &str| {
        run("reg.exe", &["query", key, "/v", value]).and_then(|o| parse_reg_dword(&o, value))
    };
    let ci_event = run(
        "wevtutil.exe",
        &[
            "qe",
            "Microsoft-Windows-CodeIntegrity/Operational",
            "/c:300",
            "/rd:true",
            "/f:text",
        ],
    )
    .and_then(|o| find_ci_event(&o));
    Facts {
        status,
        files_present,
        kb_service: sc("keyboard"),
        mouse_service: sc("mouse"),
        kb_filter: filters(KEYBOARD_CLASS, "keyboard"),
        mouse_filter: filters(MOUSE_CLASS, "mouse"),
        memory_integrity: dword(
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\DeviceGuard\\Scenarios\\HypervisorEnforcedCodeIntegrity",
            "Enabled",
        )
        .map(|v| v != 0),
        blocklist: dword(
            "HKLM\\SYSTEM\\CurrentControlSet\\Control\\CI\\Config",
            "VulnerableDriverBlocklistEnable",
        )
        .map(|v| v != 0),
        ci_event,
    }
}

// ---------------------------------------------------------------------------------------------
// Installer
// ---------------------------------------------------------------------------------------------

const INSTALLER: &str = "install-interception.exe";

fn search(dir: &Path, depth: u32) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut dirs = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_file()
            && p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(INSTALLER))
        {
            return Some(p);
        }
        if depth > 0 && p.is_dir() {
            dirs.push(p);
        }
    }
    dirs.into_iter().find_map(|d| search(&d, depth - 1))
}

/// Looks for Interception's installer next to padbridge and in the Downloads folder.
pub fn find_installer() -> Option<PathBuf> {
    let mut roots: Vec<(PathBuf, u32)> = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        roots.push((dir, 3));
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        roots.push((PathBuf::from(home).join("Downloads"), 4));
    }
    roots.into_iter().find_map(|(d, depth)| search(&d, depth))
}

/// Runs the installer as administrator in a Command Prompt window that stays open, so the user
/// sees its result. Windows asks for permission first.
pub fn run_installer(installer: &Path, uninstall: bool) -> Result<(), String> {
    if !cfg!(windows) {
        return Err("only on Windows".to_string());
    }
    if !installer.is_file() {
        return Err(format!("{} does not exist", installer.display()));
    }
    let arg = if uninstall { "/uninstall" } else { "/install" };
    let params = format!(
        "/k \"\"{}\" {arg} & echo. & echo Restart the PC to finish.\"",
        installer.display()
    );
    let dir = installer
        .parent()
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let (op, file, params, dir) = (
        crate::sys::wide("runas"),
        crate::sys::wide("cmd.exe"),
        crate::sys::wide(&params),
        crate::sys::wide(&dir),
    );
    let r = unsafe {
        crate::sys::ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            params.as_ptr(),
            dir.as_ptr(),
            1, // SW_SHOWNORMAL
        )
    };
    if r as isize > 32 {
        Ok(())
    } else {
        Err("Windows did not start the installer (permission declined?)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            status: Err(DriverError::NotInstalled),
            files_present: false,
            kb_service: Service::Missing,
            mouse_service: Service::Missing,
            kb_filter: Some(false),
            mouse_filter: Some(false),
            memory_integrity: Some(false),
            blocklist: Some(true),
            ci_event: None,
        }
    }

    #[test]
    fn sc_output() {
        let running = "SERVICE_NAME: keyboard\r\n        TYPE               : 1  KERNEL_DRIVER\r\n        STATE              : 4  RUNNING\r\n";
        assert_eq!(parse_sc(running), Service::Running);
        let stopped = "SERVICE_NAME: keyboard\n        STATE              : 1  STOPPED\n        WIN32_EXIT_CODE    : 183  (0xb7)\n";
        assert_eq!(parse_sc(stopped), Service::Stopped);
        let missing = "[SC] EnumQueryServicesStatus:OpenService FAILED 1060:\n\nThe specified service does not exist as an installed service.\n";
        assert_eq!(parse_sc(missing), Service::Missing);
        assert_eq!(parse_sc(""), Service::Unknown);
    }

    #[test]
    fn upper_filters_output() {
        let with = "\r\nHKEY_LOCAL_MACHINE\\...\\{4d36e96b-...}\r\n    UpperFilters    REG_MULTI_SZ    keyboard\\0kbdclass\r\n";
        assert_eq!(parse_upper_filters(with, "keyboard"), Some(true));
        let without = "    UpperFilters    REG_MULTI_SZ    kbdclass\r\n";
        assert_eq!(parse_upper_filters(without, "keyboard"), Some(false));
        assert_eq!(parse_upper_filters("ERROR: not found", "keyboard"), None);
        // "keyboard" must match a whole entry, not part of one
        let partial = "    UpperFilters    REG_MULTI_SZ    mykeyboardfilter\\0kbdclass\r\n";
        assert_eq!(parse_upper_filters(partial, "keyboard"), Some(false));
    }

    #[test]
    fn dword_output() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\...\r\n    Enabled    REG_DWORD    0x1\r\n";
        assert_eq!(parse_reg_dword(out, "Enabled"), Some(1));
        assert_eq!(
            parse_reg_dword("    Enabled    REG_DWORD    0x0", "Enabled"),
            Some(0)
        );
        assert_eq!(
            parse_reg_dword("ERROR: The system was unable to find", "Enabled"),
            None
        );
    }

    #[test]
    fn code_integrity_events() {
        let log = "Event[0]:\n  Log Name: Microsoft-Windows-CodeIntegrity/Operational\n  Event ID: 3001\nDescription: \nSomething about other.sys\n\nEvent[1]:\n  Event ID: 3077\nDescription: \nCode Integrity determined that a process (System) attempted to load \\Driver\\keyboard.sys that did not meet the security requirements.\n";
        let ev = find_ci_event(log).unwrap();
        assert!(
            ev.contains("did not meet the security requirements"),
            "{ev}"
        );
        assert_eq!(find_ci_event("Event[0]:\nDescription: nothing\n"), None);
    }

    #[test]
    fn verdicts_cover_each_failure() {
        let mut f = facts();
        assert_eq!(conclude(&f).title, "The driver is not installed");

        f.files_present = true;
        assert!(conclude(&f).title.contains("incomplete"));

        f.kb_service = Service::Stopped;
        f.mouse_service = Service::Stopped;
        f.kb_filter = Some(true);
        f.mouse_filter = Some(true);
        assert!(conclude(&f).title.contains("not running yet"));

        f.memory_integrity = Some(true);
        assert!(conclude(&f).title.contains("Memory integrity"));

        f.ci_event = Some("blocked".into());
        assert!(conclude(&f).title.contains("blocked"));

        f.kb_service = Service::Running;
        f.mouse_service = Service::Running;
        assert!(conclude(&f).title.contains("cannot open"));

        f.status = Ok(());
        assert!(conclude(&f).ok);

        let dll = Facts {
            status: Err(DriverError::DllMissing {
                path: "x".into(),
                code: 126,
            }),
            ..facts()
        };
        assert!(conclude(&dll).title.contains("interception.dll"));
    }

    #[test]
    fn report_lists_every_fact() {
        let r = report(&facts());
        for needle in [
            "Verdict:",
            "Driver files",
            "Memory integrity",
            "blocklist",
            "event",
        ] {
            assert!(r.contains(needle), "{needle}");
        }
    }

    #[test]
    fn installer_search_finds_nested_file() {
        let dir = std::env::temp_dir().join(format!("pb-diag-{}", std::process::id()));
        let nested = dir.join("Interception").join("command line installer");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("install-interception.exe"), b"x").unwrap();
        assert_eq!(
            search(&dir, 3),
            Some(nested.join("install-interception.exe"))
        );
        assert_eq!(search(&dir, 1), None, "depth limit respected");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
