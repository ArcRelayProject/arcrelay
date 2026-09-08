use super::*;
use objc2_foundation::{ns_string, NSNumber, NSUserDefaults};

/// A standalone Fn release has a system action; Fn used in a chord does not.
/// Cleanup drops this state without running the action.
#[derive(Default)]
pub(super) struct FunctionTap {
    held: bool,
    alone: bool,
}

impl FunctionTap {
    pub(super) fn key(&mut self, key: u16, down: bool, otherwise_empty: bool) -> bool {
        if key != HID_KEY_FUNCTION {
            self.cancel();
            return false;
        }
        if down {
            if !self.held {
                self.held = true;
                self.alone = otherwise_empty;
            }
            false
        } else {
            let activate = self.held && self.alone && otherwise_empty;
            *self = Self::default();
            activate
        }
    }

    pub(super) fn cancel(&mut self) {
        self.alone = false;
    }
}

fn emoji_configured() -> bool {
    objc2::rc::autoreleasepool(|_| {
        NSUserDefaults::standardUserDefaults()
            .persistentDomainForName(ns_string!("com.apple.HIToolbox"))
            .and_then(|domain| domain.objectForKey(ns_string!("AppleFnUsageType")))
            .and_then(|value| {
                value
                    .downcast_ref::<NSNumber>()
                    .map(|value| value.integerValue())
            })
            // The macOS default when no explicit preference has been saved.
            .unwrap_or(2)
            == 2
    })
}

fn emoji_events(platform: &NativePlatform) -> Result<[CGEvent; 2], PlatformError> {
    // CGEvent Fn edges do not invoke the hardware-only Globe action. Use the
    // standard Character Viewer shortcut with explicit flags on both edges;
    // keep ArcRelay in the background and preserve the external text target.
    let down = platform.key_event(0x2c, true, false)?;
    let up = platform.key_event(0x2c, false, false)?;
    let flags = CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagCommand;
    down.set_flags(flags);
    up.set_flags(flags);
    Ok([down, up])
}

pub(super) fn perform_function_tap(platform: &NativePlatform) -> Result<(), PlatformError> {
    if emoji_configured() {
        for event in emoji_events(platform)? {
            event.post(CGEventTapLocation::HID);
        }
        tracing::debug!(
            event = "input.function.emoji",
            "requested target macOS Character Viewer"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_tap_activates_once_and_chords_or_cleanup_cancel_it() {
        let mut tap = FunctionTap::default();
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
        assert!(!tap.key(HID_KEY_FUNCTION, true, true));
        assert!(!tap.key(HID_KEY_FUNCTION, true, true));
        assert!(tap.key(HID_KEY_FUNCTION, false, true));
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
        tap.key(HID_KEY_FUNCTION, true, true);
        tap.key(0x3a, true, false);
        tap.key(0x3a, false, true);
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
        tap.key(HID_KEY_FUNCTION, true, false);
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
        tap.key(HID_KEY_FUNCTION, true, true);
        tap.cancel();
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
        tap.key(HID_KEY_FUNCTION, true, true);
        tap = FunctionTap::default();
        assert!(!tap.key(HID_KEY_FUNCTION, false, true));
    }

    #[test]
    fn emoji_shortcut_events_preserve_external_target_without_posting() {
        let platform = NativePlatform::new(ServiceInstanceId::parse("test-device").unwrap());
        let events = emoji_events(&platform).unwrap();
        for (event, kind) in events
            .iter()
            .zip([CGEventType::KeyDown, CGEventType::KeyUp])
        {
            assert_eq!(event.get_type() as u32, kind as u32);
            assert_eq!(
                event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE),
                0x31
            );
            assert_eq!(
                event.get_flags(),
                CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagCommand
            );
            assert_eq!(
                event.get_integer_value_field(FIELD_EVENT_SOURCE_USER_DATA),
                ARC_INPUT_EVENT_TAG
            );
        }
    }
}
