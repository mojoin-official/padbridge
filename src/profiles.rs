//! Named profiles: one `.toml` file per profile in a `profiles` folder.

use crate::config::{Config, DEFAULT_CONFIG};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ProfileStore {
    dir: PathBuf,
}

const LAST_FILE: &str = ".last-profile";

impl ProfileStore {
    /// `profiles` next to the executable (falls back to the working directory). The
    /// `PADBRIDGE_PROFILE_DIR` environment variable overrides it.
    pub fn default_dir() -> PathBuf {
        if let Some(d) = std::env::var_os("PADBRIDGE_PROFILE_DIR") {
            return PathBuf::from(d);
        }
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("profiles")
    }

    /// Opens (creating if needed) a store and makes sure it holds at least one profile.
    pub fn open(dir: PathBuf) -> Result<ProfileStore, String> {
        fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create the profiles folder {}: {e}", dir.display()))?;
        let store = ProfileStore { dir };
        if store.list().is_empty() {
            store.write_text("Default", DEFAULT_CONFIG)?;
        }
        Ok(store)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Letters, digits, spaces, `-` and `_`; 1 to 40 characters; not starting with a space or dot.
    pub fn valid_name(name: &str) -> bool {
        let n = name.chars().count();
        (1..=40).contains(&n)
            && !name.starts_with([' ', '.'])
            && !name.ends_with(' ')
            && name
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.toml"))
    }

    pub fn list(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter_map(|e| {
                        let p = e.path();
                        if p.extension().and_then(|x| x.to_str()) != Some("toml") {
                            return None;
                        }
                        let stem = p.file_stem()?.to_str()?.to_string();
                        Self::valid_name(&stem).then_some(stem)
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by_key(|s| s.to_lowercase());
        v
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_file()
    }

    pub fn load(&self, name: &str) -> Result<Config, String> {
        let p = self.path(name);
        let text =
            fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        Config::parse(&text).map_err(|e| format!("profile '{name}': {e}"))
    }

    fn write_text(&self, name: &str, text: &str) -> Result<(), String> {
        if !Self::valid_name(name) {
            return Err(format!("'{name}' is not a valid profile name"));
        }
        let p = self.path(name);
        let tmp = self.dir.join(format!(".{name}.tmp"));
        fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &p).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            format!("cannot save {}: {e}", p.display())
        })
    }

    /// Saves atomically (write to a temp file, then rename), so a crash never leaves half a file.
    pub fn save(&self, name: &str, cfg: &Config) -> Result<(), String> {
        self.write_text(name, &cfg.to_toml())
    }

    pub fn delete(&self, name: &str) -> Result<(), String> {
        if self.list().len() <= 1 {
            return Err("the last profile cannot be deleted".to_string());
        }
        fs::remove_file(self.path(name)).map_err(|e| format!("cannot delete '{name}': {e}"))
    }

    pub fn last_used(&self) -> Option<String> {
        let name = fs::read_to_string(self.dir.join(LAST_FILE)).ok()?;
        let name = name.trim().to_string();
        self.exists(&name).then_some(name)
    }

    pub fn set_last_used(&self, name: &str) {
        let _ = fs::write(self.dir.join(LAST_FILE), name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> ProfileStore {
        let dir = std::env::temp_dir().join(format!(
            "padbridge-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        ProfileStore::open(dir).unwrap()
    }

    #[test]
    fn new_store_gets_a_default_profile() {
        let s = temp_store("default");
        assert_eq!(s.list(), vec!["Default".to_string()]);
        let c = s.load("Default").unwrap();
        assert!(c.bindings.source_count() > 0);
        fs::remove_dir_all(s.dir()).unwrap();
    }

    #[test]
    fn save_load_delete_and_last_used() {
        let s = temp_store("crud");
        let mut c = s.load("Default").unwrap();
        c.general.output_hz = 250;
        s.save("my game", &c).unwrap();
        assert_eq!(s.list(), vec!["Default".to_string(), "my game".to_string()]);
        assert_eq!(s.load("my game").unwrap().general.output_hz, 250);
        s.set_last_used("my game");
        assert_eq!(s.last_used().as_deref(), Some("my game"));
        s.delete("my game").unwrap();
        assert_eq!(s.last_used(), None, "a deleted profile is not 'last used'");
        assert!(s.delete("Default").is_err(), "cannot delete the last one");
        // no temp files left behind
        let leftovers: Vec<_> = fs::read_dir(s.dir())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        fs::remove_dir_all(s.dir()).unwrap();
    }

    #[test]
    fn names_are_validated() {
        for ok in ["Default", "my game", "CoD_2-mw"] {
            assert!(ProfileStore::valid_name(ok), "{ok}");
        }
        for bad in [
            "",
            " lead",
            "trail ",
            ".hidden",
            "a/b",
            "a\\b",
            "x:y",
            &"x".repeat(41),
        ] {
            assert!(!ProfileStore::valid_name(bad), "{bad}");
        }
        let s = temp_store("names");
        assert!(s.save("../escape", &Config::builtin()).is_err());
        fs::remove_dir_all(s.dir()).unwrap();
    }

    #[test]
    fn a_broken_profile_reports_an_error_instead_of_panicking() {
        let s = temp_store("broken");
        fs::write(s.dir().join("bad.toml"), "[general]\noutput_hz = 5\n").unwrap();
        let e = s.load("bad").unwrap_err();
        assert!(e.contains("bad") && e.contains("output_hz"), "{e}");
        fs::remove_dir_all(s.dir()).unwrap();
    }
}
