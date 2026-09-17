//! Pure policy: constructing or testing this state requires no async runtime.

#[derive(Clone, Debug, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardNavigationKey {
    pub key: String,
    pub code: String,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub repeat: bool,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForegroundTarget {
    pub window: isize,
    pub process: u32,
    pub thread: u32,
    pub focus: isize,
}

#[derive(Default, Debug)]
pub struct ClipboardSession {
    pub visible: bool,
    pub ready: bool,
    pub selecting: bool,
    pub editing: bool,
    pub generation: u64,
    pub target: Option<ForegroundTarget>,
}

impl ClipboardSession {
    pub fn show(&mut self, target: Option<ForegroundTarget>) {
        self.generation += 1;
        self.visible = true;
        self.ready = false;
        self.editing = false;
        self.selecting = true;
        self.target = target;
    }

    pub fn pause(&mut self) {
        self.generation += 1;
        self.selecting = false;
    }

    pub fn hide(&mut self) {
        self.pause();
        self.visible = false;
        self.editing = false;
        self.target = None;
    }

    pub fn captures(&self, foreground: Option<ForegroundTarget>) -> bool {
        self.visible
            && self.ready
            && self.selecting
            && !self.editing
            && self.target.is_some()
            && self.target == foreground
    }
}

/// Only keys with an actual list action are intercepted. Ctrl+V is deliberately
/// absent: the existing continuous-paste trigger owns it. Never intercept Alt/Win.
pub fn navigation_key(
    vk: u32,
    ctrl: bool,
    shift: bool,
    alt: bool,
    win: bool,
) -> Option<(&'static str, String)> {
    if alt || win {
        return None;
    }
    let (key, code) = if ctrl {
        if shift {
            return None;
        }
        match vk {
            0x41 => ("a", "KeyA".into()),
            0x46 => ("f", "KeyF".into()),
            0x4b => ("k", "KeyK".into()),
            0x31..=0x39 => ("", format!("Digit{}", vk - 0x30)),
            0x61..=0x69 => ("", format!("Numpad{}", vk - 0x60)),
            _ => return None,
        }
    } else {
        if shift && !matches!(vk, 0x09 | 0x0d) {
            return None;
        }
        match vk {
            0x09 => ("Tab", "Tab".into()),
            0x0d => ("Enter", "Enter".into()),
            0x1b => ("Escape", "Escape".into()),
            0x20 => (" ", "Space".into()),
            0x21 => ("PageUp", "PageUp".into()),
            0x22 => ("PageDown", "PageDown".into()),
            0x26 => ("ArrowUp", "ArrowUp".into()),
            0x25 => ("ArrowLeft", "ArrowLeft".into()),
            0x27 => ("ArrowRight", "ArrowRight".into()),
            0x28 => ("ArrowDown", "ArrowDown".into()),
            0x43 => ("c", "KeyC".into()),
            0x45 => ("e", "KeyE".into()),
            0x46 => ("f", "KeyF".into()),
            0x4a => ("j", "KeyJ".into()),
            0x4b => ("k", "KeyK".into()),
            0x4c => ("l", "KeyL".into()),
            0x5d => ("ContextMenu", "ContextMenu".into()),
            0xbf => ("/", "Slash".into()),
            _ => return None,
        }
    };
    Some((key, code))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target() -> Option<ForegroundTarget> {
        Some(ForegroundTarget {
            window: 42,
            process: 7,
            thread: 8,
            focus: 43,
        })
    }
    #[test]
    fn session_construction_without_tokio_and_readiness_gate() {
        let mut state = ClipboardSession::default();
        state.show(target());
        assert!(!state.captures(target()));
        state.ready = true;
        assert!(state.captures(target()));
        state.editing = true;
        assert!(!state.captures(target()));
    }
    #[test]
    fn pinned_idle_hidden_and_changed_target_never_capture() {
        let mut state = ClipboardSession {
            ready: true,
            ..Default::default()
        };
        state.show(target());
        state.ready = true;
        let mut changed = target().unwrap();
        changed.focus = 99;
        assert!(!state.captures(Some(changed)));
        changed = target().unwrap();
        changed.process = 99;
        assert!(!state.captures(Some(changed)));
        assert!(!state.captures(None));
        state.pause();
        assert!(!state.captures(target()));
        state.show(target());
        state.ready = true;
        assert!(state.captures(target()));
        state.hide();
        assert!(!state.captures(target()));
    }
    #[test]
    fn injected_paste_and_unmapped_combinations_are_not_navigation() {
        assert!(navigation_key(0x56, true, false, false, false).is_none());
        assert!(navigation_key(0x0d, false, false, true, false).is_none());
        assert!(navigation_key(0x46, true, false, false, true).is_none());
        assert!(navigation_key(0x43, true, false, false, false).is_none());
        assert!(navigation_key(0x45, false, true, false, false).is_none());
        assert_eq!(
            navigation_key(0x0d, false, true, false, false).unwrap().0,
            "Enter"
        );
        assert_eq!(
            navigation_key(0x62, true, false, false, false).unwrap().1,
            "Numpad2"
        );
    }
}
