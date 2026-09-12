//! Bounded trackpad gesture adapter. DockSwipe fields are verified by the local
//! gesture laboratory on macOS 26; two-finger magnification uses the companion
//! Quartz gesture stream. This is not the Universal Control protocol.

use super::*;
use arcrelay_core::infrastructure::input_control::macos_system_gesture;
use arcrelay_input::SystemGestureEvent;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(super) const MASK: u64 = (1 << 29) | (1 << 30);

const MAGNIFICATION_SUBTYPE: i64 = 8;
const MIN_TRACKPAD_SENSOR_ROWS: i32 = 10;

type MTDeviceRef = *mut c_void;
type MTContactCallback = unsafe extern "C" fn(MTDeviceRef, *mut c_void, i32, f64, i32) -> i32;
type MTDeviceCreateList = unsafe extern "C" fn() -> *const c_void;
type MTRegisterContactFrameCallback = unsafe extern "C" fn(MTDeviceRef, MTContactCallback);
type MTDeviceStart = unsafe extern "C" fn(MTDeviceRef, i32);
type MTDeviceGetSensorDimensions = unsafe extern "C" fn(MTDeviceRef, *mut i32, *mut i32) -> i32;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
}

static CONTACT_TRACKING_ENABLED: AtomicBool = AtomicBool::new(false);
static CONTACT_COUNT: AtomicU32 = AtomicU32::new(0);
static CONTACTS_BY_DEVICE: Mutex<BTreeMap<usize, u32>> = Mutex::new(BTreeMap::new());

unsafe extern "C" fn contact_frame(
    device: MTDeviceRef,
    _contacts: *mut c_void,
    count: i32,
    _timestamp: f64,
    _frame: i32,
) -> i32 {
    if !CONTACT_TRACKING_ENABLED.load(Ordering::Acquire) {
        return 0;
    }
    let mut devices = CONTACTS_BY_DEVICE
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if count > 0 {
        devices.insert(device as usize, count as u32);
    } else {
        devices.remove(&(device as usize));
    }
    CONTACT_COUNT.store(
        devices.values().copied().max().unwrap_or(0),
        Ordering::Release,
    );
    0
}

struct ContactRuntime {
    _framework: *mut c_void,
    _devices: *const c_void,
    _registered: Vec<MTDeviceRef>,
}

// The private framework owns callback threads and the retained device array for
// the process lifetime. Access to shared observations is atomic or mutex-bound.
unsafe impl Send for ContactRuntime {}
unsafe impl Sync for ContactRuntime {}

impl ContactRuntime {
    unsafe fn load() -> Option<Self> {
        const PATHS: [&[u8]; 4] = [
            b"/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport\0",
            b"/System/Library/PrivateFrameworks/MultitouchSupport.framework/Versions/A/MultitouchSupport\0",
            b"/System/Library/PrivateFrameworks/MultitouchSupportPrivate.framework/MultitouchSupportPrivate\0",
            b"/System/Library/PrivateFrameworks/MultitouchSupportPrivate.framework/Versions/A/MultitouchSupportPrivate\0",
        ];
        let framework = PATHS.into_iter().find_map(|path| {
            let handle = libc::dlopen(path.as_ptr().cast(), libc::RTLD_NOW);
            (!handle.is_null()).then_some(handle)
        })?;
        let symbol = |name: &[u8]| {
            let value = libc::dlsym(framework, name.as_ptr().cast());
            (!value.is_null()).then_some(value)
        };
        let symbols = (
            symbol(b"MTDeviceCreateList\0"),
            symbol(b"MTRegisterContactFrameCallback\0"),
            symbol(b"MTDeviceStart\0"),
            symbol(b"MTDeviceGetSensorDimensions\0"),
        );
        let (Some(create_list), Some(register), Some(start), Some(dimensions)) = symbols else {
            libc::dlclose(framework);
            return None;
        };
        let create_list: MTDeviceCreateList = std::mem::transmute(create_list);
        let register: MTRegisterContactFrameCallback = std::mem::transmute(register);
        let start: MTDeviceStart = std::mem::transmute(start);
        let dimensions: MTDeviceGetSensorDimensions = std::mem::transmute(dimensions);
        let devices = create_list();
        if devices.is_null() {
            libc::dlclose(framework);
            return None;
        }
        let mut registered = Vec::new();
        for index in 0..CFArrayGetCount(devices) {
            let device = CFArrayGetValueAtIndex(devices, index) as MTDeviceRef;
            if device.is_null() {
                continue;
            }
            let (mut rows, mut columns) = (0, 0);
            if dimensions(device, &mut rows, &mut columns) != 0 || rows < MIN_TRACKPAD_SENSOR_ROWS {
                continue;
            }
            register(device, contact_frame);
            start(device, 0);
            registered.push(device);
        }
        if registered.is_empty() {
            CFRelease(devices);
            libc::dlclose(framework);
            return None;
        }
        // Deliberately retain the framework/device list for process lifetime.
        // MultitouchSupport has a documented-in-practice teardown race between
        // callback unregistration and MTDeviceStop on macOS 14/15.
        Some(Self {
            _framework: framework,
            _devices: devices,
            _registered: registered,
        })
    }
}

pub(super) struct ContactTrackingGuard {
    available: bool,
}

impl ContactTrackingGuard {
    pub(super) fn start() -> Self {
        static RUNTIME: OnceLock<Option<ContactRuntime>> = OnceLock::new();
        let available = RUNTIME
            .get_or_init(|| unsafe { ContactRuntime::load() })
            .is_some();
        CONTACT_COUNT.store(0, Ordering::Release);
        CONTACTS_BY_DEVICE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        CONTACT_TRACKING_ENABLED.store(available, Ordering::Release);
        Self { available }
    }

    pub(super) fn available(&self) -> bool {
        self.available
    }
}

impl Drop for ContactTrackingGuard {
    fn drop(&mut self) {
        CONTACT_TRACKING_ENABLED.store(false, Ordering::Release);
        CONTACT_COUNT.store(0, Ordering::Release);
        CONTACTS_BY_DEVICE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

pub(super) fn contact_count() -> u32 {
    CONTACT_COUNT.load(Ordering::Acquire)
}

pub(super) fn supported() -> bool {
    macos_system_gesture::supported()
}

pub(super) unsafe fn decode(event: CGEventRef) -> Option<SystemGestureEvent> {
    let event_type = CGEventGetIntegerValueField(event, 55);
    let subtype = CGEventGetIntegerValueField(event, 110);
    let magnification = event_type == 29 && subtype == MAGNIFICATION_SUBTYPE;
    if !magnification && (event_type != 30 || subtype != 23) {
        return None;
    }
    let axis = if magnification {
        3
    } else {
        u32::try_from(CGEventGetIntegerValueField(event, 123)).ok()?
    };
    let mut decoded = SystemGestureEvent {
        axis,
        phase: u32::try_from(CGEventGetIntegerValueField(event, 132)).ok()?,
        progress: CGEventGetDoubleValueField(event, if magnification { 113 } else { 124 }),
        velocity_x: if magnification {
            0.0
        } else {
            CGEventGetDoubleValueField(event, 129)
        },
        velocity_y: if magnification {
            0.0
        } else {
            CGEventGetDoubleValueField(event, 130)
        },
        // Pinch's native direction flag affects how the Dock interprets it.
        // Swipes retain the v1 injection convention used by mobile and old peers.
        inverted_from_device: !magnification
            && axis == 3
            && CGEventGetIntegerValueField(event, 136) != 0,
        finger_count: 0,
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
    gesture: Option<SystemGestureEvent>,
    companion_pending: bool,
}

impl CaptureGate {
    pub(super) fn route(
        &mut self,
        event: Option<SystemGestureEvent>,
        marker: bool,
        incremental: bool,
        observed_fingers: u32,
        generation: u64,
        format_version: u32,
    ) -> (bool, Option<SystemGestureEvent>) {
        if self.generation != generation || self.format_version != format_version || generation == 0
        {
            *self = Self {
                generation,
                format_version,
                ..Self::default()
            };
        }
        if generation == 0 {
            return (false, None);
        }
        if let Some(mut event) = event {
            self.companion_pending = false;
            if event.validate_format(format_version).is_err() {
                return (false, None);
            }
            if event.phase == 1 {
                if format_version >= arcrelay_input::SYSTEM_GESTURE_CONTACTS_FORMAT_VERSION {
                    event.finger_count = if incremental {
                        2
                    } else if matches!(observed_fingers, 3 | 4) {
                        observed_fingers
                    } else {
                        0
                    };
                }
                if incremental {
                    event.progress = 0.0;
                }
            } else if let Some(active) = self.gesture {
                event.finger_count = active.finger_count;
                if incremental {
                    event.progress = (active.progress + event.progress).clamp(-4.0, 4.0);
                }
                if active.axis != event.axis
                    || active.inverted_from_device != event.inverted_from_device
                {
                    return (false, None);
                }
            } else {
                return (false, None);
            }
            if event.validate_format(format_version).is_err() {
                return (false, None);
            }
            self.companion_pending = !incremental;
            if matches!(event.phase, 4 | 8) {
                self.gesture = None;
            } else {
                self.gesture = Some(event);
            }
            (true, Some(event))
        } else if marker && self.companion_pending {
            self.companion_pending = false;
            (true, None)
        } else {
            self.companion_pending = false;
            (false, None)
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
            finger_count: 0,
        }
    }

    fn route(
        gate: &mut CaptureGate,
        event: Option<SystemGestureEvent>,
        marker: bool,
        generation: u64,
        version: u32,
    ) -> bool {
        gate.route(event, marker, false, 0, generation, version).0
    }

    #[test]
    fn gesture_gate_requires_a_begin_and_resets_on_target_change() {
        let mut gate = CaptureGate::default();
        assert!(!route(&mut gate, Some(event(1)), false, 0, 1));
        assert!(!route(&mut gate, Some(event(2)), false, 1, 1));
        assert!(route(&mut gate, Some(event(1)), false, 1, 1));
        assert!(route(&mut gate, None, true, 1, 1));
        assert!(!route(&mut gate, None, false, 1, 1)); // other gestures, including scroll
        assert!(route(&mut gate, Some(event(2)), false, 1, 1));
        assert!(!route(&mut gate, Some(event(2)), false, 2, 1));
        assert!(!route(&mut gate, Some(event(4)), false, 2, 1));
        assert!(route(&mut gate, Some(event(1)), false, 2, 1));
        assert!(route(&mut gate, Some(event(4)), false, 2, 1));
        assert!(route(&mut gate, None, true, 2, 1));
        assert!(!route(&mut gate, None, true, 2, 1));
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
            CGEventSetIntegerValueField(raw.0, 55, 29);
            CGEventSetIntegerValueField(raw.0, 110, 8);
            CGEventSetDoubleValueField(raw.0, 113, 0.125);
        }
        assert_eq!(
            raw.decode(),
            Some(SystemGestureEvent {
                axis: 3,
                progress: 0.125,
                velocity_x: 0.0,
                velocity_y: 0.0,
                ..event(1)
            })
        );
        unsafe {
            CGEventSetIntegerValueField(raw.0, 110, 23);
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
        assert!(route(&mut gate, Some(event(1)), false, 1, 1));
        assert!(!route(&mut gate, Some(pinch(1)), false, 1, 1));
        assert!(!route(&mut gate, None, true, 1, 1));
        assert!(!route(&mut gate, Some(pinch(2)), false, 1, 2));
        assert!(route(&mut gate, Some(pinch(1)), false, 1, 2));
        assert!(route(&mut gate, None, true, 1, 2));
        assert!(route(&mut gate, Some(pinch(2)), false, 1, 2));
        assert!(!route(&mut gate, Some(pinch(4)), false, 2, 2));
        assert!(!route(&mut gate, None, true, 2, 2));
        assert!(route(&mut gate, Some(pinch(1)), false, 2, 2));
        assert!(!route(&mut gate, Some(pinch(2)), false, 2, 1));
        assert!(!route(&mut gate, None, true, 2, 1));
        for terminal in [4, 8] {
            assert!(route(&mut gate, Some(pinch(1)), false, 3, 2));
            assert!(route(&mut gate, Some(pinch(terminal)), false, 3, 2));
            assert!(route(&mut gate, None, true, 3, 2));
            assert!(!route(&mut gate, Some(pinch(2)), false, 3, 2));
        }
        assert!(!route(&mut gate, Some(pinch(1)), false, 0, 2));
    }

    #[test]
    fn v3_preserves_swipe_contacts_and_accumulates_two_finger_magnification() {
        let mut gate = CaptureGate::default();
        let (_, began) = gate.route(Some(event(1)), false, false, 3, 1, 3);
        assert_eq!(began.unwrap().finger_count, 3);
        let (_, changed) = gate.route(Some(event(2)), false, false, 0, 1, 3);
        assert_eq!(changed.unwrap().finger_count, 3);

        let magnification = |phase, progress| SystemGestureEvent {
            axis: 3,
            phase,
            progress,
            velocity_x: 0.0,
            velocity_y: 0.0,
            inverted_from_device: false,
            finger_count: 0,
        };
        let (_, began) = gate.route(Some(magnification(1, 0.1)), false, true, 2, 2, 3);
        let began = began.unwrap();
        assert_eq!(began.progress, 0.0);
        assert_eq!(began.finger_count, 2);
        let (_, changed) = gate.route(Some(magnification(2, 0.15)), false, true, 2, 2, 3);
        assert_eq!(changed.unwrap().progress, 0.15);
        let (_, changed) = gate.route(Some(magnification(2, 0.2)), false, true, 2, 2, 3);
        assert_eq!(changed.unwrap().progress, 0.35);
        let (_, ended) = gate.route(Some(magnification(4, 0.05)), false, true, 0, 2, 3);
        assert!((ended.unwrap().progress - 0.4).abs() < f64::EPSILON);
        assert!(
            !gate
                .route(Some(magnification(2, 0.1)), false, true, 0, 2, 3)
                .0
        );
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
