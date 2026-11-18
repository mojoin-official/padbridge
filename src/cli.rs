use padbridge::app::{self, RunOptions, Runtime};
use padbridge::config::{Config, DEFAULT_CONFIG};
use padbridge::keys;
use padbridge::pad::{AxisTarget, Output};
use std::path::{Path, PathBuf};

const HELP: &str = "\
padbridge-cli - keyboard and mouse to a virtual Xbox 360 (XInput) controller (console version;
padbridge.exe is the window app)

USAGE:
    padbridge-cli [OPTIONS]

OPTIONS:
    -c, --config <FILE>      Use this configuration (default: ./bindings.toml, then the file
                             next to the executable, then the built-in defaults)
        --check              Validate the configuration, print what it does, and exit
        --dry-run            Run the full pipeline but print controller reports instead of
                             creating a virtual controller (no driver needed)
        --sniff              Print the name of every key / mouse button you press (to find the
                             name to use in the configuration); maps nothing
        --list-names         List every valid input, output and axis name
        --write-default [F]  Write the default configuration to F (default: bindings.toml)
        --force              Allow --write-default to overwrite an existing file
    -h, --help               Show this help
    -V, --version            Show the version

Default hotkeys (configurable): Ctrl+Alt+P toggles mapping, Ctrl+Alt+Q exits.
";

#[derive(Default)]
struct Cli {
    config: Option<PathBuf>,
    check: bool,
    dry_run: bool,
    sniff: bool,
    list_names: bool,
    write_default: Option<PathBuf>,
    force: bool,
    help: bool,
    version: bool,
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-c" | "--config" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| format!("{a} needs a file name"))?;
                cli.config = Some(PathBuf::from(v));
            }
            "--check" => cli.check = true,
            "--dry-run" => cli.dry_run = true,
            "--sniff" => cli.sniff = true,
            "--list-names" => cli.list_names = true,
            "--force" => cli.force = true,
            "--write-default" => {
                let path = match args.get(i + 1) {
                    Some(next) if !next.starts_with('-') => {
                        i += 1;
                        PathBuf::from(next)
                    }
                    _ => PathBuf::from("bindings.toml"),
                };
                cli.write_default = Some(path);
            }
            "-h" | "--help" => cli.help = true,
            "-V" | "--version" => cli.version = true,
            other => return Err(format!("unknown option '{other}' (try --help)")),
        }
        i += 1;
    }
    Ok(cli)
}

fn find_config(explicit: Option<&Path>) -> Result<(Config, String), String> {
    let load = |path: &Path| -> Result<(Config, String), String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let cfg = Config::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok((cfg, path.display().to_string()))
    };
    if let Some(p) = explicit {
        return load(p);
    }
    let local = PathBuf::from("bindings.toml");
    if local.is_file() {
        return load(&local);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let beside = dir.join("bindings.toml");
        if beside.is_file() {
            return load(&beside);
        }
    }
    Ok((Config::builtin(), "built-in defaults".to_string()))
}

/// Writes informational output, ignoring a closed pipe (`padbridge --list-names | head` must not
/// panic the way `println!` does on EPIPE).
fn emit(text: &str) {
    use std::io::Write;
    let _ = std::io::stdout().write_all(text.as_bytes());
}

fn names_text() -> String {
    let mut s = String::new();
    s.push_str("INPUTS (use in [bindings] values and hotkeys; case-insensitive):\n");
    for chunk in keys::all_names().chunks(8) {
        s.push_str(&format!("  {}\n", chunk.join(", ")));
    }
    s.push_str("  sc:0xNNNN  (raw scan code, e.g. sc:0xE05B, for keys not listed)\n");
    s.push_str("  Hotkey modifiers: Ctrl, Shift, Alt, Win (either side)\n\n");
    s.push_str("OUTPUTS (use as [bindings] keys):\n");
    let names: Vec<&str> = Output::ALL.iter().map(|o| o.name()).collect();
    for chunk in names.chunks(6) {
        s.push_str(&format!("  {}\n", chunk.join(", ")));
    }
    s.push_str("\nMOUSE AXIS TARGETS (use as 'target' in [axis.mouse_x] / [axis.mouse_y]):\n");
    let axes: Vec<&str> = AxisTarget::ALL.iter().map(|a| a.name()).collect();
    s.push_str(&format!("  {}\n", axes.join(", ")));
    s
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return 2;
        }
    };

    if cli.help {
        emit(HELP);
        return 0;
    }
    if cli.version {
        emit(&format!("padbridge {}\n", env!("CARGO_PKG_VERSION")));
        return 0;
    }
    if cli.list_names {
        emit(&names_text());
        return 0;
    }
    if let Some(path) = &cli.write_default {
        if path.exists() && !cli.force {
            eprintln!(
                "error: {} already exists (use --force to overwrite)",
                path.display()
            );
            return 1;
        }
        return match std::fs::write(path, DEFAULT_CONFIG) {
            Ok(()) => {
                println!("wrote {}", path.display());
                0
            }
            Err(e) => {
                eprintln!("error: cannot write {}: {e}", path.display());
                1
            }
        };
    }

    let (cfg, origin) = match find_config(cli.config.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };

    if cli.check {
        emit(&format!("configuration OK ({origin})\n{}", cfg.summary()));
        return 0;
    }

    println!("[padbridge] configuration: {origin}");
    let opts = RunOptions {
        dry_run: cli.dry_run,
        sniff: cli.sniff,
        print_reports: true,
    };
    let mut rt = match Runtime::start(&cfg, opts) {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            return 1;
        }
    };
    app::install_console_ctrl_handler(&rt);
    let print = |lines: Vec<String>| {
        for l in lines {
            emit(&format!("[padbridge] {l}\n"));
        }
    };
    while rt.is_running() {
        print(rt.drain_log());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    rt.stop();
    print(rt.drain_log());
    0
}

fn main() {
    std::process::exit(real_main());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags() {
        let c = parse_args(&args(&["--check", "-c", "x.toml", "--dry-run"])).unwrap();
        assert!(c.check && c.dry_run);
        assert_eq!(c.config.unwrap(), PathBuf::from("x.toml"));
    }

    #[test]
    fn write_default_path_is_optional() {
        let c = parse_args(&args(&["--write-default"])).unwrap();
        assert_eq!(c.write_default.unwrap(), PathBuf::from("bindings.toml"));
        let c = parse_args(&args(&["--write-default", "a.toml", "--force"])).unwrap();
        assert_eq!(c.write_default.unwrap(), PathBuf::from("a.toml"));
        assert!(c.force);
        let c = parse_args(&args(&["--write-default", "--force"])).unwrap();
        assert_eq!(c.write_default.unwrap(), PathBuf::from("bindings.toml"));
        assert!(c.force);
    }

    #[test]
    fn name_listing_covers_every_table() {
        let t = names_text();
        for needle in [
            "Space",
            "MouseLeft",
            "WheelUp",
            "LeftStickUp",
            "LT",
            "RightStickY",
            "sc:0x",
        ] {
            assert!(t.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn rejects_unknown_and_incomplete_options() {
        assert!(parse_args(&args(&["--nope"])).is_err());
        assert!(parse_args(&args(&["--config"])).is_err());
    }
}
