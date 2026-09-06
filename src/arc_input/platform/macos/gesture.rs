//! Legacy DockSwipe adapter, verified by the local gesture laboratory on macOS 26.
//! Not Universal Control. Unknown event types are never decoded or posted here.

use super::*;
use arcrelay_core::infrastructure::input_control::macos_system_gesture;
use arcrelay_input::SystemGestureEvent;

pub(super) const MASK: u64 = (1 << 29) | (1 << 30);

pub(super) fn supported() -> bool {
    macos_system_gesture::supported()
}

pub(super) unsafe fn decode(event: CGEventRef) -> Option<SystemGestureEvent> {
    if CGEventGetIntegerValueField(event, 55) != 30 || CGEventGetIntegerValueField(event, 110) != 23
    {
        return None;
    }
    let axis = u32::try_from(CGEventGetIntegerValueField(event, 123)).ok()?;
    let mut decoded = SystemGestureEvent {
        axis,
        phase: u32::try_from(CGEventGetIntegerValueField(event, 132)).ok()?,
        progress: CGEventGetDoubleValueField(event, 124),
        velocity_x: CGEventGetDoubleValueField(event, 129),
        velocity_y: CGEventGetDoubleValueField(event, 130),
        // Pinch's native direction flag affects how the Dock interprets it.
        // Swipes retain the v1 injection convention used by mobile and old peers.
        inverted_from_device: axis == 3 && CGEventGetIntegerValueField(event, 136) != 0,
    }
    .validate()
    .ok()?;
    if axis == 2 {
        // The macOS source trace reports up as negative/down as positive, but
        // replay with our legacy injector reverses those actions (dual-Mac test).
        // Convert only captured vertical gestures, including terminal velocity;
        // do not invert horizontal swipes or mobile-generated input a second time.
        decoded.progress = -decoded.progress;
        decoded.velocity_x = -decoded.velocity_x;
        decoded.velocity_y = -decoded.velocity_y;
    }
    Some(decoded)
}

#[derive(Default)]
pub(super) struct CaptureGate {
    generation: u64,
    format_version: u32,
    gesture: Option<(u32, bool)>,
    companion_pending: bool,
}

impl CaptureGate {
    pub(super) fn route(
        &mut self,
        event: Option<SystemGestureEvent>,
        marker: bool,
        generation: u64,
        format_version: u32,
    ) -> bool {
        if self.generation != generation || self.format_version != format_version || generation == 0
        {
            *self = Self {
                generation,
                format_version,
                ..Self::default()
            };
        }
        if generation == 0 {
            return false;
        }
        if let Some(event) = event {
            self.companion_pending = false;
            if event.validate_format(format_version).is_err() {
                return false;
            }
            if event.phase == 1 {
                self.gesture = Some((event.axis, event.inverted_from_device));
            }
            if self.gesture != Some((event.axis, event.inverted_from_device)) {
                return false;
            }
            self.companion_pending = true;
            if matches!(event.phase, 4 | 8) {
                self.gesture = None;
            }
            true
        } else if marker && self.companion_pending {
            self.companion_pending = false;
            true
        } else {
            self.companion_pending = false;
            false
        }
    }
}

pub(super) fn post(event: SystemGestureEvent) -> Result<(), PlatformError> {
    macos_system_gesture::post(event).map_err(|error| PlatformError::Operation(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> CGEventRef;
        fn CGEventSetDoubleValueField(event: CGEventRef, field: u32, value: f64);
    }
    struct TestEvent(CGEventRef);
    impl TestEvent {
        fn new(sample: SystemGestureEvent) -> Self {
            unsafe {
                let raw = CGEventCreate(std::ptr::null());
                assert!(!raw.is_null());
                for (field, value) in [
                    (55, 30),
                    (110, 23),
                    (123, sample.axis as i64),
                    (132, sample.phase as i64),
                    (136, sample.inverted_from_device as i64),
                ] {
                    CGEventSetIntegerValueField(raw, field, value);
                }
                for (field, value) in [
                    (124, sample.progress),
                    (129, sample.velocity_x),
                    (130, sample.velocity_y),
                ] {
                    CGEventSetDoubleValueField(raw, field, value);
                }
                Self(raw)
            }
        }
        fn decode(&self) -> Option<SystemGestureEvent> {
            unsafe { decode(self.0) }
        }
    }
    impl Drop for TestEvent {
        fn drop(&mut self) {
            unsafe {
                CFRelease(self.0);
            }
        }
    }
    fn event(phase: u32) -> SystemGestureEvent {
        SystemGestureEvent {
            axis: 2,
            phase,
            progress: -0.375,
            velocity_x: -4.5,
            velocity_y: -4.5,
            inverted_from_device: false,
        }
    }

    #[test]
    fn gesture_gate_requires_a_begin_and_resets_on_target_change() {
        let mut gate = CaptureGate::default();
        assert!(!gate.route(Some(event(1)), false, 0, 1));
        assert!(!gate.route(Some(event(2)), false, 1, 1));
        assert!(gate.route(Some(event(1)), false, 1, 1));
        assert!(gate.route(None, true, 1, 1));
        assert!(!gate.route(None, false, 1, 1)); // other gestures, including two-finger scroll
        assert!(gate.route(Some(event(2)), false, 1, 1));
        assert!(!gate.route(Some(event(2)), false, 2, 1));
        assert!(!gate.route(Some(event(4)), false, 2, 1));
        assert!(gate.route(Some(event(1)), false, 2, 1));
        assert!(gate.route(Some(event(4)), false, 2, 1));
        assert!(gate.route(None, true, 2, 1));
        assert!(!gate.route(None, true, 2, 1));
    }

    #[test]
    fn captured_vertical_direction_and_exit_velocity_are_corrected_once() {
        // Signs from the user's labelled upward/downward macOS 26 trace.
        for sign in [-1.0, 1.0] {
            for phase in [1, 2, 4, 8] {
                let sample = SystemGestureEvent {
                    progress: sign * 0.375,
                    velocity_x: sign * 4.5,
                    velocity_y: sign * 4.5,
                    ..event(phase)
                };
                let decoded = TestEvent::new(sample).decode().unwrap();
                assert_eq!(
                    decoded,
                    SystemGestureEvent {
                        progress: -sample.progress,
                        velocity_x: -sample.velocity_x,
                        velocity_y: -sample.velocity_y,
                        ..sample
                    }
                );
                assert_eq!(
                    SystemGestureEvent::try_from(arcrelay_input::proto::SystemGesture::from(
                        decoded
                    )),
                    Ok(decoded)
                );
            }
        }
    }

    #[test]
    fn capture_preserves_horizontal_and_pinch_values_and_rejects_other_gestures() {
        for axis in [1, 3] {
            for sign in [-1.0, 1.0] {
                for inverted_from_device in [false, true] {
                    let sample = SystemGestureEvent {
                        axis,
                        progress: sign * 0.5,
                        inverted_from_device,
                        ..event(1)
                    };
                    let decoded = TestEvent::new(sample).decode().unwrap();
                    assert_eq!(
                        decoded,
                        SystemGestureEvent {
                            inverted_from_device: axis == 3 && inverted_from_device,
                            ..sample
                        }
                    );
                }
            }
        }
        for axis in [0, 4, u32::MAX] {
            assert!(TestEvent::new(SystemGestureEvent { axis, ..event(1) })
                .decode()
                .is_none());
        }
        assert!(TestEvent::new(SystemGestureEvent {
            progress: f64::NAN,
            ..event(1)
        })
        .decode()
        .is_none());
        let raw = TestEvent::new(event(1));
        unsafe {
            CGEventSetIntegerValueField(raw.0, 110, 8);
        } // ordinary magnification
        assert!(raw.decode().is_none());
        unsafe {
            CGEventSetIntegerValueField(raw.0, 110, 23);
            CGEventSetIntegerValueField(raw.0, 55, 29);
        }
        assert!(raw.decode().is_none());
    }

    #[test]
    fn pinch_capture_requires_v2_and_fails_open_after_route_or_version_change() {
        let pinch = |phase| SystemGestureEvent {
            axis: 3,
            inverted_from_device: true,
            ..event(phase)
        };
        let mut gate = CaptureGate::default();
        assert!(gate.route(Some(event(1)), false, 1, 1));
        assert!(!gate.route(Some(pinch(1)), false, 1, 1));
        assert!(!gate.route(None, true, 1, 1));
        assert!(!gate.route(Some(pinch(2)), false, 1, 2));
        assert!(gate.route(Some(pinch(1)), false, 1, 2));
        assert!(gate.route(None, true, 1, 2));
        assert!(gate.route(Some(pinch(2)), false, 1, 2));
        assert!(!gate.route(Some(pinch(4)), false, 2, 2));
        assert!(!gate.route(None, true, 2, 2));
        assert!(gate.route(Some(pinch(1)), false, 2, 2));
        assert!(!gate.route(Some(pinch(2)), false, 2, 1));
        assert!(!gate.route(None, true, 2, 1));
        for terminal in [4, 8] {
            assert!(gate.route(Some(pinch(1)), false, 3, 2));
            assert!(gate.route(Some(pinch(terminal)), false, 3, 2));
            assert!(gate.route(None, true, 3, 2));
            assert!(!gate.route(Some(pinch(2)), false, 3, 2));
        }
        assert!(!gate.route(Some(pinch(1)), false, 0, 2));
    }

    #[test]
    fn native_gesture_adapter_constructs_and_maintains_idle_without_tokio() {
        let platform = NativePlatform::new(ServiceInstanceId::parse("gesture-test").unwrap());
        assert!(lock(&platform.capture).is_none());
        assert_eq!(
            platform.system_gesture_generation.load(Ordering::Acquire),
            0
        );
        assert!(!platform.maintain_system_gesture().unwrap());
        assert!(!lock(&platform.injection).system_gesture.is_active());
    }
}
