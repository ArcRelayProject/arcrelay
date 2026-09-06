//! Native two-finger scrolling. Keep Quartz's deltas, phases and momentum, but
//! replace the source device's location/clock with the receiving Mac's context.

use core_foundation::base::{CFType, TCFType};
use core_foundation::data::CFData;

use super::*;

thread_local! {
    // CF/Quartz objects stay on their owning thread. Reusing this source avoids
    // creating a new WindowServer event source for every trackpad sample.
    static SCROLL_SOURCE: RefCell<Option<CFType>> = const { RefCell::new(None) };
}

pub(super) struct NativeScroll(CFType);

impl NativeScroll {
    fn raw(&self) -> CGEventRef {
        self.0.as_concrete_TypeRef().cast_mut()
    }

    pub(super) fn scroll_event(&self) -> ScrollEvent {
        unsafe { portable_scroll_event(self.raw()) }
    }

    pub(super) fn post(&self) {
        unsafe { CGEventPost(0, self.raw()) }
    }
}

// Only call for an owned Core Foundation object returned by a Create function.
unsafe fn owned(raw: *const c_void, operation: &str) -> Result<CFType, PlatformError> {
    if raw.is_null() {
        Err(PlatformError::Operation(operation.into()))
    } else {
        Ok(CFType::wrap_under_create_rule(raw))
    }
}

fn local_timestamp() -> Result<u64, PlatformError> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_UPTIME_RAW is mach_absolute_time expressed in nanoseconds, not
    // Unix time or the sleep-inclusive continuous clock.
    if unsafe { libc::clock_gettime(libc::CLOCK_UPTIME_RAW, &mut time) } != 0 {
        return Err(PlatformError::Operation(
            "cannot read local Quartz clock".into(),
        ));
    }
    Ok(time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64)
}

pub(super) fn prepare_native(data: &[u8]) -> Result<NativeScroll, PlatformError> {
    if data.is_empty() || data.len() > MAX_NATIVE_QUARTZ_EVENT_SIZE {
        return Err(PlatformError::Operation(format!(
            "native Quartz event size {} is outside 1..={MAX_NATIVE_QUARTZ_EVENT_SIZE}",
            data.len()
        )));
    }
    let flattened = CFData::from_buffer(data);
    unsafe {
        let event = NativeScroll(owned(
            CGEventCreateFromData(std::ptr::null(), flattened.as_concrete_TypeRef().cast()),
            "CGEventCreateFromData rejected native Quartz event",
        )?);
        if CGEventGetType(event.raw()) != CGEventType::ScrollWheel as u32 {
            return Err(PlatformError::Operation(
                "native Quartz payload is not a scroll event".into(),
            ));
        }
        SCROLL_SOURCE.with(|cached| {
            let mut cached = cached.borrow_mut();
            if cached.is_none() {
                *cached = Some(owned(
                    CGEventSourceCreate(0),
                    "CGEventSourceCreate returned null",
                )?);
            }
            let source = cached
                .as_ref()
                .expect("scroll source initialized")
                .as_concrete_TypeRef();
            let context = owned(CGEventCreate(source), "cannot read local Quartz context")?;
            let current = context.as_concrete_TypeRef().cast_mut();
            CGEventSetSource(event.raw(), source);
            CGEventSetLocation(event.raw(), CGEventGetLocation(current));
            // A CGEvent timestamp is nanoseconds since THIS Mac booted. Zero
            // carries no sample timing, and the source Mac's uptime is not
            // usable either. Read the actual local uptime explicitly: even a
            // freshly created CGEvent can have a zero timestamp. No sleep,
            // fixed-rate batching or synthetic scroll deltas are introduced.
            CGEventSetTimestamp(event.raw(), local_timestamp()?);
            CGEventSetIntegerValueField(
                event.raw(),
                FIELD_EVENT_SOURCE_USER_DATA,
                ARC_INPUT_EVENT_TAG,
            );
            Ok(event)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(phase: i64, momentum: i64, timestamp: u64) -> Vec<u8> {
        unsafe {
            let event = owned(
                CGEventCreateScrollWheelEvent2(
                    std::ptr::null(),
                    ScrollEventUnit::PIXEL,
                    2,
                    12,
                    -3,
                    0,
                ),
                "test scroll",
            )
            .unwrap();
            let raw = event.as_concrete_TypeRef().cast_mut();
            CGEventSetTimestamp(raw, timestamp);
            CGEventSetLocation(raw, CGPoint::new(-123_456.0, -123_456.0));
            CGEventSetIntegerValueField(raw, FIELD_SCROLL_WHEEL_EVENT_SCROLL_PHASE, phase);
            CGEventSetIntegerValueField(raw, FIELD_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE, momentum);
            flatten_quartz_event(raw).unwrap()
        }
    }

    fn now() -> u64 {
        local_timestamp().unwrap()
    }

    #[test]
    fn rebases_zero_and_foreign_timestamps_without_posting_events() {
        let mut previous = 0;
        for timestamp in [0, 1, u64::MAX, 0] {
            let data = payload(2, 0, timestamp);
            let before = now();
            let event = prepare_native(&data).unwrap();
            let after = now();
            unsafe {
                let actual = CGEventGetTimestamp(event.raw());
                assert!(actual >= before && actual <= after);
                assert!(actual > previous);
                previous = actual;
                assert_ne!(CGEventGetLocation(event.raw()).x, -123_456.0);
                assert_eq!(
                    CGEventGetIntegerValueField(event.raw(), FIELD_EVENT_SOURCE_USER_DATA),
                    ARC_INPUT_EVENT_TAG
                );
            }
        }
    }

    #[test]
    fn keeps_all_scroll_and_momentum_boundaries_and_native_deltas() {
        let mut state = InjectionState::default();
        for (phase, momentum, gesture_active, momentum_active) in [
            (128, 0, false, false),
            (1, 0, true, false),
            (2, 0, true, false),
            (4, 0, false, false),
            (0, 1, false, true),
            (0, 2, false, true),
            (0, 3, false, false),
            (1, 0, true, false),
            (8, 0, false, false),
        ] {
            let event = prepare_native(&payload(phase, momentum, 1)).unwrap();
            let scroll = event.scroll_event();
            assert_eq!(scroll.delta_x, -3.0);
            assert_eq!(scroll.delta_y, 12.0);
            assert_eq!(scroll.unit, ScrollUnit::Pixel);
            assert_eq!(quartz_scroll_phase(scroll.phase), phase);
            assert_eq!(quartz_momentum_phase(scroll.momentum_phase), momentum);
            unsafe {
                assert_eq!(
                    CGEventGetDoubleValueField(
                        event.raw(),
                        EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1
                    ),
                    1.1999969482421875
                );
            }
            state.observe_scroll(scroll);
            assert_eq!(state.scroll_gesture_active, gesture_active);
            assert_eq!(state.scroll_momentum_active, momentum_active);
        }
    }

    #[test]
    fn reuses_source_without_tokio_runtime() {
        let data = payload(2, 0, 0);
        let _ = prepare_native(&data).unwrap();
        let source =
            SCROLL_SOURCE.with(|cached| cached.borrow().as_ref().unwrap().as_concrete_TypeRef());
        for _ in 0..100 {
            let _ = prepare_native(&data).unwrap();
            SCROLL_SOURCE.with(|cached| {
                assert_eq!(
                    cached.borrow().as_ref().unwrap().as_concrete_TypeRef(),
                    source
                )
            });
        }
    }

    #[test]
    fn tracks_scroll_entering_mid_gesture_for_release_cleanup() {
        let mut state = InjectionState::default();
        state.observe_scroll(prepare_native(&payload(2, 0, 0)).unwrap().scroll_event());
        assert!(state.scroll_gesture_active);
        state.observe_scroll(prepare_native(&payload(8, 0, 0)).unwrap().scroll_event());
        assert!(!state.scroll_gesture_active);
        state.observe_scroll(prepare_native(&payload(0, 2, 0)).unwrap().scroll_event());
        assert!(state.scroll_momentum_active);
        state.observe_scroll(prepare_native(&payload(0, 3, 0)).unwrap().scroll_event());
        assert!(!state.scroll_momentum_active);
    }

    #[test]
    fn rejects_oversized_malformed_and_non_scroll_events() {
        assert!(prepare_native(&[]).is_err());
        assert!(prepare_native(&vec![0; MAX_NATIVE_QUARTZ_EVENT_SIZE + 1]).is_err());
        assert!(prepare_native(&[0; 16]).is_err());
        unsafe {
            let event = owned(CGEventCreate(std::ptr::null()), "test event").unwrap();
            let data = flatten_quartz_event(event.as_concrete_TypeRef().cast_mut()).unwrap();
            assert!(prepare_native(&data).is_err());
        }
    }
}
