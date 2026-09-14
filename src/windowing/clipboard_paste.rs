use std::time::Duration;

const FOCUS_SETTLE_TIME: Duration = Duration::from_millis(150);

/// Tracks a single paste while AppKit returns keyboard focus to its recipient.
/// `None` means ArcRelay (or no application) is still frontmost, not failure.
pub struct ClipboardPasteTarget {
    pub pid: Option<i32>,
    clipboard_version: isize,
    observed_target: bool,
    ready_since: Option<Duration>,
    restore_pinned_panel: bool,
}

impl ClipboardPasteTarget {
    pub fn new(pid: Option<i32>, clipboard_version: isize) -> Self {
        Self {
            pid,
            clipboard_version,
            observed_target: false,
            ready_since: None,
            restore_pinned_panel: false,
        }
    }

    pub fn with_pinned_panel_restore(mut self, restore: bool) -> Self {
        self.restore_pinned_panel = restore;
        self
    }

    pub fn restore_pinned_panel(&self) -> bool {
        self.restore_pinned_panel
    }

    pub fn observe(
        &mut self,
        external_pid: Option<i32>,
        clipboard_version: isize,
        panel_focused: bool,
        elapsed: Duration,
    ) -> Result<bool, &'static str> {
        if clipboard_version != self.clipboard_version {
            return Err("clipboard changed during focus handoff; paste cancelled");
        }
        match (self.pid, external_pid) {
            (Some(expected), Some(current)) if expected != current => {
                return Err("frontmost paste target changed; paste cancelled");
            }
            (_, Some(current)) => {
                self.pid = Some(current);
                self.observed_target = true;
            }
            (_, None) if self.observed_target => {
                return Err("paste target lost frontmost status; paste cancelled");
            }
            (_, None) => {
                self.ready_since = None;
                return Ok(false);
            }
        }
        if panel_focused {
            self.ready_since = None;
            return Ok(false);
        }
        let ready_since = *self.ready_since.get_or_insert(elapsed);
        Ok(elapsed.saturating_sub(ready_since) >= FOCUS_SETTLE_TIME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: u64) -> Duration {
        Duration::from_millis(value)
    }

    #[test]
    fn activated_clipboard_panel_waits_for_original_application() {
        let mut target = ClipboardPasteTarget::new(Some(42), 7);
        assert_eq!(target.observe(None, 7, true, ms(20)), Ok(false));
        assert_eq!(target.observe(None, 7, false, ms(40)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(60)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(100)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(200)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(220)), Ok(true));
    }

    #[test]
    fn missing_target_is_resolved_after_panel_orders_out() {
        let mut target = ClipboardPasteTarget::new(None, 7);
        assert_eq!(target.observe(None, 7, false, ms(20)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(40)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(180)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(200)), Ok(true));
        assert_eq!(target.pid, Some(42));
    }

    #[test]
    fn no_external_application_never_becomes_ready() {
        let mut target = ClipboardPasteTarget::new(None, 7);
        for elapsed in [20, 100, 500] {
            assert_eq!(target.observe(None, 7, false, ms(elapsed)), Ok(false));
        }
    }

    #[test]
    fn switching_to_another_application_cancels_restoration() {
        let mut target = ClipboardPasteTarget::new(Some(42), 7);
        assert_eq!(
            target.observe(Some(99), 7, false, ms(20)),
            Err("frontmost paste target changed; paste cancelled")
        );
    }

    #[test]
    fn losing_the_restored_target_cancels_paste() {
        for next_pid in [None, Some(99)] {
            let mut target = ClipboardPasteTarget::new(Some(42), 7);
            assert_eq!(target.observe(Some(42), 7, false, ms(20)), Ok(false));
            let expected = if next_pid.is_none() {
                "paste target lost frontmost status; paste cancelled"
            } else {
                "frontmost paste target changed; paste cancelled"
            };
            assert_eq!(target.observe(next_pid, 7, false, ms(40)), Err(expected));
        }
    }

    #[test]
    fn clipboard_replacement_cancels_before_or_after_focus_returns() {
        for pid in [None, Some(42)] {
            let mut target = ClipboardPasteTarget::new(Some(42), 7);
            assert_eq!(
                target.observe(pid, 8, false, ms(20)),
                Err("clipboard changed during focus handoff; paste cancelled")
            );
        }
    }

    #[test]
    fn pinned_panel_restore_is_explicit() {
        let target = ClipboardPasteTarget::new(Some(42), 7).with_pinned_panel_restore(true);
        assert!(target.restore_pinned_panel());
    }

    #[test]
    fn pinned_panel_must_relinquish_focus_for_full_settle_interval() {
        let mut target = ClipboardPasteTarget::new(Some(42), 7);
        assert_eq!(target.observe(Some(42), 7, true, ms(20)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(40)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, true, ms(60)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(80)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(120)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(220)), Ok(false));
        assert_eq!(target.observe(Some(42), 7, false, ms(240)), Ok(true));
    }
}
