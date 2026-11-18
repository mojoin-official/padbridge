//! Decides, for every physical input event, whether to swallow it or let it through, and handles
//! hotkeys. Pure logic: the Windows hooks just ask it for a [`Verdict`].
//!
//! The central invariant is **the OS must never see an unbalanced key/button**:
//! * if it saw a press (we passed it), it must also see the release, even when capture was
//!   switched on in between;
//! * if it never saw the press (we swallowed it), it must never see the release either.
//!
//! Without that, toggling capture while holding a key would leave keys or buttons stuck down.

use crate::bindings::{Bindings, Combo, UnboundPolicy};
use crate::keys::Source;
use crate::mapper::Mapper;
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Edge {
    Down,
    Up,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Capture was switched; carries the new state.
    Toggled(bool),
    /// The exit hotkey fired.
    Exit,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// True: do not forward this event to the rest of the system.
    pub swallow: bool,
    pub action: Option<Action>,
}

impl Verdict {
    const PASS: Verdict = Verdict {
        swallow: false,
        action: None,
    };
    const SWALLOW: Verdict = Verdict {
        swallow: true,
        action: None,
    };
}

pub struct Capture {
    bindings: Arc<Bindings>,
    toggle: Combo,
    exit: Combo,
    unbound: UnboundPolicy,
    /// Whether mouse motion should be swallowed while capturing.
    motion_swallowed: bool,
    enabled: bool,
    /// Every physical key/button currently down (for hotkey modifiers).
    held: HashSet<Source>,
    /// Presses we swallowed; their releases must be swallowed too.
    swallowed: HashSet<Source>,
    /// Presses we let through; their releases must be let through too.
    passed: HashSet<Source>,
    /// Hotkey trigger presses we ate (and their repeats and release).
    eaten: HashSet<Source>,
}

impl Capture {
    pub fn new(
        bindings: Arc<Bindings>,
        toggle: Combo,
        exit: Combo,
        unbound: UnboundPolicy,
        swallow_motion: bool,
        has_mouse_axes: bool,
        enabled: bool,
    ) -> Capture {
        Capture {
            bindings,
            toggle,
            exit,
            unbound,
            motion_swallowed: swallow_motion && (has_mouse_axes || unbound == UnboundPolicy::Block),
            enabled,
            held: HashSet::new(),
            swallowed: HashSet::new(),
            passed: HashSet::new(),
            eaten: HashSet::new(),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Switches capture on or off. Always resets the mapper so no output is left held.
    pub fn set_enabled(&mut self, on: bool, mapper: &mut Mapper) {
        self.enabled = on;
        mapper.reset();
    }

    /// Whether the input is ours to consume while capturing.
    fn owned(&self, src: Source) -> bool {
        match self.unbound {
            UnboundPolicy::Block => true,
            UnboundPolicy::Pass => self.bindings.contains(src),
        }
    }

    /// Should mouse *motion* (cursor movement) be swallowed right now?
    pub fn swallow_motion(&self) -> bool {
        self.enabled && self.motion_swallowed
    }

    /// Should a wheel notch in this direction be swallowed right now?
    pub fn wheel_swallowed(&self, dir: crate::keys::WheelDir) -> bool {
        self.enabled && self.owned(Source::Wheel(dir))
    }

    /// Handles one key / mouse-button edge. (Wheel notches go through the engine instead.)
    pub fn on_event(&mut self, src: Source, edge: Edge, mapper: &mut Mapper) -> Verdict {
        match edge {
            Edge::Down => self.on_down(src, mapper),
            Edge::Up => self.on_up(src, mapper),
        }
    }

    fn on_down(&mut self, src: Source, mapper: &mut Mapper) -> Verdict {
        let first = self.held.insert(src);

        if first {
            let action = if self.toggle.fires_on(src, &self.held) {
                let on = !self.enabled;
                self.set_enabled(on, mapper);
                Some(Action::Toggled(on))
            } else if self.exit.fires_on(src, &self.held) {
                Some(Action::Exit)
            } else {
                None
            };
            if let Some(a) = action {
                self.eaten.insert(src);
                return Verdict {
                    swallow: true,
                    action: Some(a),
                };
            }
        }
        if self.eaten.contains(&src) {
            return Verdict::SWALLOW; // auto-repeat of an eaten hotkey
        }
        if self.passed.contains(&src) {
            return Verdict::PASS; // continuing a press the system already saw
        }
        if self.swallowed.contains(&src) {
            return Verdict::SWALLOW; // auto-repeat of a press we own
        }
        if self.enabled && self.owned(src) {
            self.swallowed.insert(src);
            mapper.press(src);
            Verdict::SWALLOW
        } else {
            self.passed.insert(src);
            Verdict::PASS
        }
    }

    fn on_up(&mut self, src: Source, mapper: &mut Mapper) -> Verdict {
        self.held.remove(&src);
        if self.eaten.remove(&src) {
            return Verdict::SWALLOW;
        }
        if self.swallowed.remove(&src) {
            mapper.release(src);
            return Verdict::SWALLOW;
        }
        // Either a press we passed, or one that began before we were running: the system saw the
        // press, so it must see the release.
        self.passed.remove(&src);
        Verdict::PASS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::pad::BTN_A;

    fn mk(general: &str, bindings: &str) -> (Capture, Mapper) {
        let text = format!(
            "[general]\n{general}\n[axis.mouse_x]\ntarget = \"RightStickX\"\n[bindings]\n{bindings}\n"
        );
        let cfg = Config::parse(&text).unwrap();
        let b = Arc::new(cfg.bindings.clone());
        let mapper = Mapper::new(&cfg, Arc::clone(&b));
        let cap = Capture::new(
            b,
            cfg.general.toggle_hotkey.clone(),
            cfg.general.exit_hotkey.clone(),
            cfg.general.unbound,
            cfg.mouse.swallow_motion,
            mapper.has_mouse_axes(),
            cfg.general.start_enabled,
        );
        (cap, mapper)
    }

    fn s(n: &str) -> Source {
        Source::parse(n).unwrap()
    }

    fn down(c: &mut Capture, m: &mut Mapper, n: &str) -> Verdict {
        c.on_event(s(n), Edge::Down, m)
    }

    fn up(c: &mut Capture, m: &mut Mapper, n: &str) -> Verdict {
        c.on_event(s(n), Edge::Up, m)
    }

    #[test]
    fn bound_input_is_swallowed_and_drives_the_mapper() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        assert!(down(&mut c, &mut m, "Space").swallow);
        assert_eq!(m.tick().buttons, BTN_A);
        assert!(up(&mut c, &mut m, "Space").swallow);
        assert_eq!(m.tick().buttons, 0);
    }

    #[test]
    fn unbound_input_passes_by_default() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        assert!(!down(&mut c, &mut m, "X").swallow);
        assert!(!up(&mut c, &mut m, "X").swallow);
    }

    #[test]
    fn block_policy_swallows_everything_but_hotkeys() {
        let (mut c, mut m) = mk("unbound = \"block\"", "A = \"Space\"");
        assert!(down(&mut c, &mut m, "X").swallow);
        assert!(up(&mut c, &mut m, "X").swallow);
        assert_eq!(m.tick().buttons, 0, "unbound key must not drive outputs");
        // hotkey still works and is eaten
        down(&mut c, &mut m, "LCtrl");
        down(&mut c, &mut m, "LAlt");
        let v = down(&mut c, &mut m, "F10");
        assert_eq!(v.action, Some(Action::Toggled(false)));
    }

    #[test]
    fn disabled_capture_passes_everything() {
        let (mut c, mut m) = mk("start_enabled = false", "A = \"Space\"");
        assert!(!down(&mut c, &mut m, "Space").swallow);
        assert!(!up(&mut c, &mut m, "Space").swallow);
        assert_eq!(m.tick().buttons, 0);
        assert!(!c.swallow_motion());
    }

    #[test]
    fn key_held_when_capture_turns_on_is_passed_through_until_released() {
        let (mut c, mut m) = mk("start_enabled = false", "A = \"Space\"");
        assert!(!down(&mut c, &mut m, "Space").swallow); // system sees the press
        down(&mut c, &mut m, "LCtrl");
        down(&mut c, &mut m, "LAlt");
        assert_eq!(
            down(&mut c, &mut m, "F10").action,
            Some(Action::Toggled(true))
        );
        // auto-repeat continues to reach the system; it is not mapped
        assert!(!down(&mut c, &mut m, "Space").swallow);
        assert_eq!(m.tick().buttons, 0);
        // the release MUST pass so the system key is not stuck
        assert!(!up(&mut c, &mut m, "Space").swallow);
        // pressing it again now is captured
        assert!(down(&mut c, &mut m, "Space").swallow);
        assert_eq!(m.tick().buttons, BTN_A);
    }

    #[test]
    fn key_held_when_capture_turns_off_never_leaks_a_release() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        assert!(down(&mut c, &mut m, "Space").swallow);
        down(&mut c, &mut m, "LCtrl");
        down(&mut c, &mut m, "LAlt");
        assert_eq!(
            down(&mut c, &mut m, "F10").action,
            Some(Action::Toggled(false))
        );
        assert_eq!(m.tick().buttons, 0, "toggle resets the pad");
        // the system never saw the press, so it must not see the release
        assert!(up(&mut c, &mut m, "Space").swallow);
        assert!(!down(&mut c, &mut m, "Space").swallow, "now passes");
    }

    #[test]
    fn release_of_a_press_from_before_startup_passes() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        assert!(!up(&mut c, &mut m, "Space").swallow);
        assert!(!up(&mut c, &mut m, "X").swallow);
    }

    #[test]
    fn hotkey_trigger_is_eaten_with_repeats_and_release() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        down(&mut c, &mut m, "LCtrl");
        down(&mut c, &mut m, "LAlt");
        let v = down(&mut c, &mut m, "F10");
        assert!(v.swallow);
        assert_eq!(v.action, Some(Action::Toggled(false)));
        // key repeat must not toggle again
        let again = down(&mut c, &mut m, "F10");
        assert!(again.swallow);
        assert_eq!(again.action, None);
        assert!(up(&mut c, &mut m, "F10").swallow);
        assert!(!c.enabled());
        // modifiers are not eaten: they reach the system
        assert!(!up(&mut c, &mut m, "LAlt").swallow);
    }

    #[test]
    fn hotkey_toggles_back_and_forth() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        for expect in [false, true, false] {
            down(&mut c, &mut m, "LCtrl");
            down(&mut c, &mut m, "LAlt");
            assert_eq!(
                down(&mut c, &mut m, "F10").action,
                Some(Action::Toggled(expect))
            );
            up(&mut c, &mut m, "F10");
            up(&mut c, &mut m, "LAlt");
            up(&mut c, &mut m, "LCtrl");
        }
    }

    #[test]
    fn exit_hotkey_fires_exit() {
        let (mut c, mut m) = mk("", "A = \"Space\"");
        down(&mut c, &mut m, "RCtrl");
        down(&mut c, &mut m, "RAlt");
        let v = down(&mut c, &mut m, "F12");
        assert_eq!(v.action, Some(Action::Exit));
        assert!(v.swallow);
    }

    #[test]
    fn hotkey_works_even_when_its_modifiers_are_bound_and_swallowed() {
        let (mut c, mut m) = mk("", "B = \"LCtrl\"\nX = \"LAlt\"");
        assert!(down(&mut c, &mut m, "LCtrl").swallow);
        assert!(down(&mut c, &mut m, "LAlt").swallow);
        assert_eq!(
            down(&mut c, &mut m, "F10").action,
            Some(Action::Toggled(false))
        );
        // their releases are still swallowed (the system never saw the presses)
        assert!(up(&mut c, &mut m, "LCtrl").swallow);
        assert!(up(&mut c, &mut m, "LAlt").swallow);
    }

    #[test]
    fn hotkey_without_its_modifiers_is_an_ordinary_key() {
        let (mut c, mut m) = mk("", "A = \"F10\"");
        let v = down(&mut c, &mut m, "F10");
        assert_eq!(v.action, None);
        assert!(v.swallow, "bound F10 is captured as A");
        assert_eq!(m.tick().buttons, BTN_A);
    }

    #[test]
    fn mouse_buttons_follow_the_same_rules() {
        let (mut c, mut m) = mk("", "A = \"MouseLeft\"");
        assert!(down(&mut c, &mut m, "MouseLeft").swallow);
        assert_eq!(m.tick().buttons, BTN_A);
        assert!(!down(&mut c, &mut m, "MouseMiddle").swallow);
        assert!(up(&mut c, &mut m, "MouseLeft").swallow);
    }

    #[test]
    fn motion_swallow_follows_config() {
        let (c, _) = mk("", "A = \"Space\"");
        assert!(
            c.swallow_motion(),
            "mouse axis bound and swallow_motion default true"
        );
        let text = "[bindings]\nA = \"Space\"\n";
        let cfg = Config::parse(text).unwrap();
        let b = Arc::new(cfg.bindings.clone());
        let m = Mapper::new(&cfg, Arc::clone(&b));
        let c = Capture::new(
            b,
            cfg.general.toggle_hotkey.clone(),
            cfg.general.exit_hotkey.clone(),
            cfg.general.unbound,
            true,
            m.has_mouse_axes(),
            true,
        );
        assert!(
            !c.swallow_motion(),
            "no mouse axes bound: the cursor keeps working"
        );
    }

    #[test]
    fn wheel_swallow_follows_bindings() {
        let (c, _) = mk("", "A = \"WheelUp\"");
        assert!(c.wheel_swallowed(crate::keys::WheelDir::Up));
        assert!(!c.wheel_swallowed(crate::keys::WheelDir::Down));
    }
}
