use super::*;
use arcrelay_input::{ConsumerKey, ConsumerKeyEvent, PlatformError};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::NSPoint;

// Private Quartz compound-event fields, checked against AppKit-created events
// in the tests below. Read data1 only for NX_SYSDEFINED/AUX_CONTROL_BUTTONS;
// other subtypes need not carry compound data.
const FIELD_EVENT_SUBTYPE: u32 = 0x53;
const FIELD_EVENT_DATA1: u32 = 0x95;
const SYSTEM_DEFINED: u32 = 14;
const AUX_CONTROL_BUTTONS: i64 = 8;

fn key_from_native(value: u16) -> Option<ConsumerKey> {
    Some(match value {
        0 => ConsumerKey::VolumeUp,
        1 => ConsumerKey::VolumeDown,
        2 => ConsumerKey::BrightnessUp,
        3 => ConsumerKey::BrightnessDown,
        7 => ConsumerKey::Mute,
        16 => ConsumerKey::PlayPause,
        17 | 19 => ConsumerKey::NextTrack,
        18 | 20 => ConsumerKey::PreviousTrack,
        _ => return None,
    })
}
fn native_key(key: ConsumerKey) -> u16 {
    match key {
        ConsumerKey::VolumeUp => 0,
        ConsumerKey::VolumeDown => 1,
        ConsumerKey::BrightnessUp => 2,
        ConsumerKey::BrightnessDown => 3,
        ConsumerKey::Mute => 7,
        ConsumerKey::PlayPause => 16,
        ConsumerKey::NextTrack => 17,
        ConsumerKey::PreviousTrack => 18,
    }
}
pub(super) unsafe fn decode(raw: CGEventRef) -> Option<ConsumerKeyEvent> {
    if raw.is_null() || CGEventGetType(raw) != SYSTEM_DEFINED {
        return None;
    }
    // Do not convert captured events to NSEvent on the event-tap thread.
    // eventWithCGEvent processes Caps Lock/input-source transitions in HIToolbox,
    // whose main-queue assertion terminates the process before we can filter it.
    if CGEventGetIntegerValueField(raw, FIELD_EVENT_SUBTYPE) != AUX_CONTROL_BUTTONS {
        return None;
    }
    let data = CGEventGetIntegerValueField(raw, FIELD_EVENT_DATA1) as u32;
    let state = (data >> 8) & 0xff;
    if !matches!(state, 10 | 11) {
        return None;
    }
    Some(ConsumerKeyEvent {
        key: key_from_native((data >> 16) as u16)?,
        down: state == 10,
        repeat: state == 10 && data & 1 != 0,
    })
}
fn make_event(key: ConsumerKey, down: bool) -> Result<objc2::rc::Retained<NSEvent>, PlatformError> {
    let state = if down { 10 } else { 11 };
    NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::SystemDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::from_bits_retain(state),
        0.0, 0, None, 8, ((native_key(key) as isize) << 16) | ((state as isize) << 8), -1,
    ).ok_or_else(|| PlatformError::Operation("cannot construct system key event".into()))
}
pub(super) fn pulse(key: ConsumerKey) -> Result<(), PlatformError> {
    objc2::rc::autoreleasepool(|_| {
        // Construct BOTH halves before posting, so a conversion failure cannot
        // leave a down without its corresponding up.
        let down = make_event(key, true)?.CGEvent().ok_or_else(|| {
            PlatformError::Operation("system key down has no Quartz event".into())
        })?;
        let up = make_event(key, false)?
            .CGEvent()
            .ok_or_else(|| PlatformError::Operation("system key up has no Quartz event".into()))?;
        for event in [down, up] {
            let raw = (&*event as *const _ as *const c_void).cast_mut();
            unsafe {
                CGEventSetIntegerValueField(raw, FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
                CGEventPost(1, raw);
            }
        }
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    fn decode_system_event(subtype: i16, data: isize) -> Option<ConsumerKeyEvent> {
        // Construct fixtures through AppKit so the test independently checks the
        // private Quartz field mapping. Never post these events to the OS.
        let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
            NSEventType::SystemDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(),
            0.0, 0, None, subtype, data, -1,
        ).unwrap().CGEvent().unwrap();
        unsafe { decode((&*event as *const _ as *const c_void).cast_mut()) }
    }

    #[test]
    fn system_event_decode_on_background_thread_ignores_caps_lock_and_other_subtypes() {
        std::thread::spawn(|| {
            objc2::rc::autoreleasepool(|_| {
                for subtype in [0, 1, 6, 7, 9, 10, 11, 12, 13] {
                    assert_eq!(decode_system_event(subtype, 0x0a00), None);
                }
                for key in [4, 6] {
                    // Caps Lock and Power are local-only.
                    for state in [10, 11] {
                        assert_eq!(decode_system_event(8, (key << 16) | (state << 8)), None);
                    }
                }
                assert_eq!(unsafe { decode(std::ptr::null_mut()) }, None);
                for down in [true, false] {
                    let raw = unsafe { CGEventCreateKeyboardEvent(std::ptr::null(), 0x39, down) };
                    assert!(!raw.is_null());
                    let decoded = unsafe { decode(raw) };
                    unsafe { CFRelease(raw) };
                    assert_eq!(decoded, None);
                }
            });
        })
        .join()
        .unwrap();
    }

    #[test]
    fn system_event_decode_preserves_repeat_and_rejects_invalid_key_states() {
        objc2::rc::autoreleasepool(|_| {
            for (state, repeat, expected) in [
                (10, 0, Some((true, false))),
                (10, 1, Some((true, true))),
                (11, 0, Some((false, false))),
                (11, 1, Some((false, false))),
                (0, 0, None),
                (12, 0, None),
            ] {
                assert_eq!(
                    decode_system_event(8, (16 << 16) | (state << 8) | repeat),
                    expected.map(|(down, repeat)| ConsumerKeyEvent {
                        key: ConsumerKey::PlayPause,
                        down,
                        repeat,
                    }),
                );
            }
        });
    }

    #[test]
    fn capture_suppresses_only_negotiated_keys_and_never_replays_a_held_key_on_handoff() {
        let (events, receiver) = mpsc::sync_channel(8);
        let mut context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: true,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: Arc::new(AtomicBool::new(true)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            native_quartz_capture: Arc::new(AtomicBool::new(false)),
            system_gesture_generation: Arc::new(AtomicU64::new(0)),
            system_gesture_format_version: Arc::new(AtomicU32::new(0)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        // Only construct events and call our callback. Never post to the OS.
        let pass = |context: &mut CaptureContext, key, down, tag| {
            let event = make_event(key, down).unwrap().CGEvent().unwrap();
            let raw = (&*event as *const _ as *const c_void).cast_mut();
            unsafe {
                CGEventSetIntegerValueField(raw, FIELD_EVENT_SOURCE_USER_DATA, tag);
                capture_callback(
                    std::ptr::null(),
                    14,
                    raw,
                    (context as *mut CaptureContext).cast(),
                ) == raw
            }
        };
        lock(&context.consumer).set_route(7, ConsumerKey::VolumeUp.mask());
        assert!(pass(&mut context, ConsumerKey::BrightnessUp, true, 0));
        assert!(pass(&mut context, ConsumerKey::BrightnessUp, false, 0));
        assert!(pass(
            &mut context,
            ConsumerKey::VolumeUp,
            true,
            ARC_INPUT_EVENT_TAG
        ));
        assert!(receiver.try_recv().is_err());
        assert!(!pass(&mut context, ConsumerKey::VolumeUp, true, 0));
        assert!(matches!(
            receiver.try_recv(),
            Ok(CapturedInputEvent::ConsumerKey {
                generation: 7,
                event: ConsumerKeyEvent {
                    down: true,
                    repeat: false,
                    ..
                }
            })
        ));
        lock(&context.consumer).set_route(8, ConsumerKey::ALL_MASK);
        assert!(!pass(&mut context, ConsumerKey::VolumeUp, true, 0));
        assert!(!pass(&mut context, ConsumerKey::VolumeUp, false, 0));
        assert!(receiver.try_recv().is_err());
        assert!(!pass(&mut context, ConsumerKey::VolumeUp, true, 0));
        assert!(matches!(
            receiver.try_recv(),
            Ok(CapturedInputEvent::ConsumerKey { generation: 8, .. })
        ));
        assert!(!pass(&mut context, ConsumerKey::VolumeUp, false, 0));
        assert!(matches!(
            receiver.try_recv(),
            Ok(CapturedInputEvent::ConsumerKey {
                generation: 8,
                event: ConsumerKeyEvent { down: false, .. }
            })
        ));
        lock(&context.consumer).set_route(0, 0);
        assert!(pass(&mut context, ConsumerKey::VolumeUp, true, 0));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn system_key_events_roundtrip_without_posting() {
        for key in ConsumerKey::ALL {
            for down in [true, false] {
                let event = make_event(key, down).unwrap().CGEvent().unwrap();
                let raw = (&*event as *const _ as *const c_void).cast_mut();
                assert_eq!(
                    unsafe { decode(raw) }.unwrap(),
                    ConsumerKeyEvent {
                        key,
                        down,
                        repeat: false
                    }
                );
            }
        }
        assert!(key_from_native(6).is_none()); // Power must never be forwarded.
        assert_eq!(key_from_native(19), Some(ConsumerKey::NextTrack));
        assert_eq!(key_from_native(20), Some(ConsumerKey::PreviousTrack));
    }
}
