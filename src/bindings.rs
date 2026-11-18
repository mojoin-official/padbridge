//! Compiled binding table, hotkey combos and the unbound-input policy.

use crate::keys::{KeyId, Source};
use crate::pad::Output;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// `source -> outputs`. One source may drive several outputs, and several sources may drive the
/// same output (the mapper counts holders, so releasing one of two keys keeps the output down).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bindings {
    map: HashMap<Source, Vec<Output>>,
}

impl Bindings {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, src: Source, out: Output) {
        let v = self.map.entry(src).or_default();
        if !v.contains(&out) {
            v.push(out);
        }
    }

    pub fn outputs(&self, src: Source) -> &[Output] {
        self.map.get(&src).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn contains(&self, src: Source) -> bool {
        self.map.contains_key(&src)
    }

    /// Removes one mapping. A source left with no outputs is removed entirely, so it stops being
    /// captured.
    pub fn remove(&mut self, src: Source, out: Output) {
        if let Some(v) = self.map.get_mut(&src) {
            v.retain(|o| *o != out);
            if v.is_empty() {
                self.map.remove(&src);
            }
        }
    }

    /// Adds the mapping if absent, removes it if present.
    pub fn toggle(&mut self, src: Source, out: Output) {
        if self.outputs(src).contains(&out) {
            self.remove(src, out);
        } else {
            self.add(src, out);
        }
    }

    /// Removes every mapping of one source.
    pub fn clear_source(&mut self, src: Source) {
        self.map.remove(&src);
    }

    /// Every source that drives `out`, in a stable order.
    pub fn sources_for(&self, out: Output) -> Vec<Source> {
        let mut v: Vec<Source> = self
            .map
            .iter()
            .filter(|(_, outs)| outs.contains(&out))
            .map(|(s, _)| *s)
            .collect();
        v.sort();
        v
    }

    /// Number of distinct bound sources.
    pub fn source_count(&self) -> usize {
        self.map.len()
    }

    /// All `(source, output)` pairs in a stable order (for `--check` output).
    pub fn pairs_sorted(&self) -> Vec<(Source, Output)> {
        let mut v: Vec<(Source, Output)> = self
            .map
            .iter()
            .flat_map(|(s, outs)| outs.iter().map(move |o| (*s, *o)))
            .collect();
        v.sort_by_key(|(s, o)| (*s, o.index()));
        v
    }
}

/// What to do with physical input that has no binding while capture is on.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnboundPolicy {
    /// Let it through to the foreground application (default: the Windows key, Alt+Tab etc. keep
    /// working).
    Pass,
    /// Swallow it, so the game sees *only* the virtual controller.
    Block,
}

/// A hotkey such as `Ctrl+Alt+F10`: zero or more modifier groups that must be held, then a
/// trigger key whose press fires the hotkey. A group holds alternatives (`Ctrl` is either
/// control key).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    groups: Vec<Vec<Source>>,
    text: String,
}

fn modifier_group(token: &str) -> Option<Vec<Source>> {
    let k = |id: KeyId| Source::Key(id);
    match crate::util::norm(token).as_str() {
        "ctrl" | "control" => Some(vec![k(0x1D), k(0xE01D)]),
        "shift" => Some(vec![k(0x2A), k(0x36)]),
        "alt" => Some(vec![k(0x38), k(0xE038)]),
        "win" | "super" | "meta" => Some(vec![k(0xE05B), k(0xE05C)]),
        _ => None,
    }
}

impl Combo {
    pub fn parse(text: &str) -> Result<Combo, String> {
        let tokens: Vec<&str> = text.split('+').map(str::trim).collect();
        if tokens.iter().any(|t| t.is_empty()) {
            return Err(format!(
                "hotkey '{text}' has an empty part (write keys joined by '+', e.g. \"Ctrl+Alt+F10\")"
            ));
        }
        let mut groups = Vec::new();
        for t in &tokens {
            if let Some(g) = modifier_group(t) {
                groups.push(g);
            } else {
                let s = Source::parse(t)?;
                if s.is_wheel() {
                    return Err(format!(
                        "hotkey '{text}': wheel notches cannot be used in hotkeys"
                    ));
                }
                groups.push(vec![s]);
            }
        }
        Ok(Combo {
            groups,
            text: text.trim().to_string(),
        })
    }

    /// True when `src` is a press of the trigger key and every modifier group has a member held.
    pub fn fires_on(&self, src: Source, held: &HashSet<Source>) -> bool {
        let Some((trigger, mods)) = self.groups.split_last() else {
            return false;
        };
        trigger.contains(&src) && mods.iter().all(|g| g.iter().any(|s| held.contains(s)))
    }

    /// True if two combos can fire on the same press (used to reject toggle == exit).
    pub fn overlaps(&self, other: &Combo) -> bool {
        self.groups == other.groups
    }
}

impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(names: &[&str]) -> HashSet<Source> {
        names.iter().map(|n| Source::parse(n).unwrap()).collect()
    }

    #[test]
    fn bindings_dedupe_and_lookup() {
        let mut b = Bindings::new();
        let w = Source::parse("W").unwrap();
        b.add(w, Output::LsUp);
        b.add(w, Output::LsUp);
        b.add(w, Output::A);
        assert_eq!(b.outputs(w), &[Output::LsUp, Output::A]);
        assert!(b.contains(w));
        assert!(!b.contains(Source::parse("S").unwrap()));
        assert_eq!(b.source_count(), 1);
        assert_eq!(b.pairs_sorted().len(), 2);
        assert!(b.outputs(Source::parse("S").unwrap()).is_empty());
    }

    #[test]
    fn editing_operations() {
        let mut b = Bindings::new();
        let w = Source::parse("W").unwrap();
        let up = Source::parse("Up").unwrap();
        b.toggle(w, Output::A);
        b.add(up, Output::A);
        assert_eq!(b.sources_for(Output::A), vec![w, up]);
        b.toggle(w, Output::A);
        assert!(
            !b.contains(w),
            "a source with no outputs is no longer captured"
        );
        b.add(up, Output::B);
        b.remove(up, Output::A);
        assert_eq!(b.outputs(up), &[Output::B]);
        b.clear_source(up);
        assert_eq!(b.source_count(), 0);
        b.remove(up, Output::A); // removing something absent is harmless
    }

    #[test]
    fn combo_fires_with_either_side_modifier() {
        let c = Combo::parse("Ctrl+Alt+F10").unwrap();
        let f10 = Source::parse("F10").unwrap();
        assert!(c.fires_on(f10, &held(&["LCtrl", "LAlt", "F10"])));
        assert!(c.fires_on(f10, &held(&["RCtrl", "RAlt", "F10"])));
        assert!(c.fires_on(f10, &held(&["LCtrl", "RAlt", "F10"])));
        assert!(!c.fires_on(f10, &held(&["LCtrl", "F10"])));
        assert!(!c.fires_on(f10, &held(&["F10"])));
        // a modifier going down is not the trigger
        assert!(!c.fires_on(
            Source::parse("LAlt").unwrap(),
            &held(&["LCtrl", "LAlt", "F10"])
        ));
    }

    #[test]
    fn single_key_combo() {
        let c = Combo::parse("Pause").unwrap();
        assert!(c.fires_on(Source::parse("Pause").unwrap(), &held(&["Pause"])));
        assert!(!c.fires_on(Source::parse("F1").unwrap(), &held(&["F1"])));
    }

    #[test]
    fn combo_errors() {
        assert!(Combo::parse("").is_err());
        assert!(Combo::parse("Ctrl+").is_err());
        assert!(Combo::parse("Ctrl+Nope").is_err());
        assert!(Combo::parse("Ctrl+WheelUp").is_err());
    }

    #[test]
    fn combo_can_use_mouse_buttons() {
        let c = Combo::parse("Ctrl+Mouse5").unwrap();
        let m5 = Source::parse("Mouse5").unwrap();
        assert!(c.fires_on(m5, &held(&["LCtrl", "Mouse5"])));
    }

    #[test]
    fn overlap_detection() {
        let a = Combo::parse("Ctrl+F1").unwrap();
        let b = Combo::parse("ctrl+f1").unwrap();
        // the spelling differs but the groups are identical
        assert!(a.overlaps(&b));
        assert!(!a.overlaps(&Combo::parse("Ctrl+F2").unwrap()));
    }
}
