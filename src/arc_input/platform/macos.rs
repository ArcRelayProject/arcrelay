use std::cell::RefCell;
use std::collections::BTreeSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arcrelay_core::infrastructure::input_control::macos_system_gesture::EVENT_TAG as ARC_INPUT_EVENT_TAG;
use arcrelay_input::{
    target_chord, CaptureOptions, CapturedInputEvent, DisplayFingerprint, DisplayId,
    DisplayInventory, DisplayInventoryPort, DisplayRotation, DisplaySurface, GeometryConfidence,
    InputCapturePort, InputInjectionPort, InventoryRevision, LogicalPoint, LogicalRect,
    MappedKeyboardEvent, OsFamily, PlatformCapabilities, PlatformError, ScaleFactor, ScrollEvent,
    ScrollMomentumPhase, ScrollPhase, ScrollUnit, SizeI64, SizeU32, HID_KEY_FUNCTION,
    MAX_NATIVE_QUARTZ_EVENT_SIZE,
};
use arcrelay_peer::ServiceInstanceId;
use core_graphics::display::CGDisplay;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, EventField, ScrollEventUnit,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

const MACOS_KEYCODE_FUNCTION: u16 = 0x3f;
mod consumer;
mod cursor;
mod gesture;
mod keyboard;
mod scroll;
use cursor::set_hidden as set_cursor_hidden;
const CAPTURE_QUEUE_CAPACITY: usize = 4096;
const FIELD_EVENT_SOURCE_USER_DATA: u32 = 42;
const FIELD_SCROLL_WHEEL_EVENT_SCROLL_PHASE: u32 = 99;
const FIELD_SCROLL_WHEEL_EVENT_SCROLL_COUNT: u32 = 100;
const FIELD_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE: u32 = 123;
const QUARTZ_SCROLL_PHASE_BEGAN: i64 = 1;
const QUARTZ_SCROLL_PHASE_CHANGED: i64 = 2;
const QUARTZ_SCROLL_PHASE_ENDED: i64 = 4;
const QUARTZ_SCROLL_PHASE_CANCELLED: i64 = 8;
const QUARTZ_SCROLL_PHASE_MAY_BEGIN: i64 = 128;
const QUARTZ_MOMENTUM_PHASE_BEGAN: i64 = 1;
const QUARTZ_MOMENTUM_PHASE_CHANGED: i64 = 2;
const QUARTZ_MOMENTUM_PHASE_ENDED: i64 = 3;
const EVENT_MASK: u64 = (1 << 1)
    | (1 << 2)
    | (1 << 3)
    | (1 << 4)
    | (1 << 5)
    | (1 << 6)
    | (1 << 7)
    | (1 << 10)
    | (1 << 11)
    | (1 << 12)
    | (1 << 22)
    | (1 << 25)
    | (1 << 26)
    | (1 << 27);

thread_local! {
    // Quartz source wrappers are not Send. A per-thread cache keeps the hot
    // pointer-injection path allocation-free without weakening the platform
    // adapter's Send + Sync contract.
    static EVENT_SOURCE: RefCell<Option<CGEventSource>> = const { RefCell::new(None) };
}

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFDataRef = *const c_void;
type CFRunLoopRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFStringRef = *const c_void;

type EventTapCallback = unsafe extern "C" fn(
    proxy: *const c_void,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: EventTapCallback,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: CGEventRef, field: u32) -> f64;
    fn CGEventGetType(event: CGEventRef) -> u32;
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    #[cfg(test)]
    fn CGEventCreateKeyboardEvent(
        source: *const c_void,
        virtual_key: u16,
        key_down: bool,
    ) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    #[cfg(test)]
    fn CGEventGetTimestamp(event: CGEventRef) -> u64;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    #[cfg(test)]
    fn CGEventSetFlags(event: CGEventRef, flags: u64);
    fn CGEventCreateData(allocator: *const c_void, event: CGEventRef) -> CFDataRef;
    fn CGEventCreateFromData(allocator: *const c_void, data: CFDataRef) -> CGEventRef;
    fn CGEventCreateMouseEvent(
        source: *const c_void,
        mouse_type: u32,
        position: CGPoint,
        button: u32,
    ) -> CGEventRef;
    #[cfg(test)]
    fn CGEventCreateScrollWheelEvent2(
        source: *const c_void,
        units: u32,
        wheel_count: u32,
        wheel_1: i32,
        wheel_2: i32,
        wheel_3: i32,
    ) -> CGEventRef;
    fn CGEventSourceCreate(state_id: i32) -> *mut c_void;
    fn CGEventPost(tap: u32, event: CGEventRef);
    fn CGEventSetLocation(event: CGEventRef, location: CGPoint);
    fn CGEventSetSource(event: CGEventRef, source: *const c_void);
    fn CGEventSetTimestamp(event: CGEventRef, timestamp: u64);
    fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    #[cfg(test)]
    fn CFDataCreate(allocator: *const c_void, bytes: *const u8, length: isize) -> CFDataRef;
    fn CFDataGetBytePtr(data: CFDataRef) -> *const u8;
    fn CFDataGetLength(data: CFDataRef) -> isize;
    fn CFMachPortCreateRunLoopSource(
        allocator: *const c_void,
        port: CFMachPortRef,
        order: isize,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(loop_ref: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopRun();
    fn CFRunLoopStop(loop_ref: CFRunLoopRef);
    fn CFMachPortInvalidate(port: CFMachPortRef);
    fn CFRunLoopRemoveSource(loop_ref: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRelease(value: *const c_void);
    static kCFRunLoopCommonModes: CFStringRef;
}

struct CaptureContext {
    consumer: Arc<Mutex<arcrelay_input::ConsumerCapture>>,
    events: SyncSender<CapturedInputEvent>,
    options: CaptureOptions,
    suppress_local: Arc<AtomicBool>,
    cursor_hidden: Arc<Mutex<bool>>,
    native_quartz_capture: Arc<AtomicBool>,
    system_gesture_generation: Arc<AtomicU64>,
    system_gesture_format_version: Arc<AtomicU32>,
    gesture_gate: RefCell<gesture::CaptureGate>,
    pressed_modifiers: AtomicUsize,
    caps_lock: std::cell::Cell<Option<bool>>,
    tap: AtomicUsize,
    gesture_tap: AtomicUsize,
    overflowed: AtomicBool,
}

unsafe fn flatten_quartz_event(event: CGEventRef) -> Option<Vec<u8>> {
    let data = CGEventCreateData(std::ptr::null(), event);
    if data.is_null() {
        return None;
    }
    let length = CFDataGetLength(data);
    let payload = if length > 0 && length as usize <= MAX_NATIVE_QUARTZ_EVENT_SIZE {
        let bytes = CFDataGetBytePtr(data);
        (!bytes.is_null()).then(|| std::slice::from_raw_parts(bytes, length as usize).to_vec())
    } else {
        None
    };
    CFRelease(data);
    payload
}

fn portable_scroll_phase(value: i64) -> ScrollPhase {
    match value {
        QUARTZ_SCROLL_PHASE_MAY_BEGIN => ScrollPhase::MayBegin,
        QUARTZ_SCROLL_PHASE_BEGAN => ScrollPhase::Began,
        QUARTZ_SCROLL_PHASE_CHANGED => ScrollPhase::Changed,
        QUARTZ_SCROLL_PHASE_ENDED => ScrollPhase::Ended,
        QUARTZ_SCROLL_PHASE_CANCELLED => ScrollPhase::Cancelled,
        _ => ScrollPhase::Unspecified,
    }
}

fn portable_momentum_phase(value: i64) -> ScrollMomentumPhase {
    match value {
        QUARTZ_MOMENTUM_PHASE_BEGAN => ScrollMomentumPhase::Began,
        QUARTZ_MOMENTUM_PHASE_CHANGED => ScrollMomentumPhase::Changed,
        QUARTZ_MOMENTUM_PHASE_ENDED => ScrollMomentumPhase::Ended,
        _ => ScrollMomentumPhase::Unspecified,
    }
}

fn quartz_scroll_phase(phase: ScrollPhase) -> i64 {
    match phase {
        ScrollPhase::MayBegin => QUARTZ_SCROLL_PHASE_MAY_BEGIN,
        ScrollPhase::Began => QUARTZ_SCROLL_PHASE_BEGAN,
        ScrollPhase::Changed => QUARTZ_SCROLL_PHASE_CHANGED,
        ScrollPhase::Ended => QUARTZ_SCROLL_PHASE_ENDED,
        ScrollPhase::Cancelled => QUARTZ_SCROLL_PHASE_CANCELLED,
        ScrollPhase::Unspecified => 0,
    }
}

fn quartz_momentum_phase(phase: ScrollMomentumPhase) -> i64 {
    match phase {
        ScrollMomentumPhase::Began => QUARTZ_MOMENTUM_PHASE_BEGAN,
        ScrollMomentumPhase::Changed => QUARTZ_MOMENTUM_PHASE_CHANGED,
        ScrollMomentumPhase::Ended => QUARTZ_MOMENTUM_PHASE_ENDED,
        ScrollMomentumPhase::Unspecified => 0,
    }
}

unsafe fn portable_scroll_event(event: CGEventRef) -> ScrollEvent {
    let continuous =
        CGEventGetIntegerValueField(event, EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS) != 0;
    let (delta_x, delta_y) = if continuous {
        // Fixed-point deltas are expressed in line-equivalent units. On a
        // default event source, a 12-point trackpad movement is represented
        // there as roughly 1.2. Windows touchpad injection consumes physical
        // movement, so forwarding that field as pixels makes ordinary gestures
        // too small to cross the pan recognizer's threshold.
        (
            CGEventGetDoubleValueField(event, EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2),
            CGEventGetDoubleValueField(event, EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1),
        )
    } else {
        (
            CGEventGetDoubleValueField(
                event,
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
            ),
            CGEventGetDoubleValueField(
                event,
                EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            ),
        )
    };
    ScrollEvent {
        delta_x,
        delta_y,
        unit: if continuous {
            ScrollUnit::Pixel
        } else {
            ScrollUnit::WheelDetent
        },
        phase: portable_scroll_phase(CGEventGetIntegerValueField(
            event,
            FIELD_SCROLL_WHEEL_EVENT_SCROLL_PHASE,
        )),
        momentum_phase: portable_momentum_phase(CGEventGetIntegerValueField(
            event,
            FIELD_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE,
        )),
    }
}

impl CaptureContext {
    fn fail_open(&self) {
        lock(&self.consumer).set_route(0, 0);
        self.suppress_local.store(false, Ordering::Release);
        self.native_quartz_capture.store(false, Ordering::Release);
        self.system_gesture_generation.store(0, Ordering::Release);
        if let Err(error) = set_cursor_hidden(&self.cursor_hidden, false) {
            tracing::error!(%error, "Arc Input could not restore the local macOS cursor");
        }
    }

    fn emit(&self, event: CapturedInputEvent) {
        if self.overflowed.swap(false, Ordering::AcqRel)
            && self
                .events
                .try_send(CapturedInputEvent::EmergencyRelease)
                .is_err()
        {
            self.fail_open();
            self.overflowed.store(true, Ordering::Release);
        }
        if self.events.try_send(event).is_err() {
            // A full native queue means the consumer is no longer keeping up.
            // Failing open is essential: never keep swallowing OS input while
            // the release signal itself is unable to reach the runtime.
            self.fail_open();
            self.overflowed.store(true, Ordering::Release);
        }
    }
}

const MODIFIER_SHIFT: usize = 1 << 0;
const MODIFIER_OPTION: usize = 1 << 1;
const MODIFIER_COMMAND: usize = 1 << 2;

fn emergency_modifier_bit(hid_usage: u16) -> usize {
    match hid_usage {
        0xe1 | 0xe5 => MODIFIER_SHIFT,
        0xe2 | 0xe6 => MODIFIER_OPTION,
        0xe3 | 0xe7 => MODIFIER_COMMAND,
        _ => 0,
    }
}

fn emergency_release_pressed(modifiers: usize, hid_usage: u16, down: bool) -> bool {
    down && matches!(hid_usage, 0x10 | 0x29)
        && modifiers & (MODIFIER_SHIFT | MODIFIER_OPTION | MODIFIER_COMMAND)
            == MODIFIER_SHIFT | MODIFIER_OPTION | MODIFIER_COMMAND
}

unsafe extern "C" fn capture_callback(
    _proxy: *const c_void,
    event_type: u32,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    if user_info.is_null() {
        return event;
    }
    let context = &*(user_info as *const CaptureContext);
    if matches!(event_type, 0xffff_fffe | 0xffff_ffff) {
        // A disabled tap may supply no event. Fail open before resuming observation.
        context.fail_open();
        for tap in [&context.tap, &context.gesture_tap] {
            let tap = tap.load(Ordering::Acquire) as CFMachPortRef;
            if !tap.is_null() {
                CGEventTapEnable(tap, true);
            }
        }
        context.emit(CapturedInputEvent::EmergencyRelease);
        return event;
    }
    if event.is_null() {
        return event;
    }
    if CGEventGetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA) == ARC_INPUT_EVENT_TAG {
        return event;
    }
    if event_type == 12
        && context.options.capture_keyboard
        && CGEventGetIntegerValueField(event, EventField::KEYBOARD_EVENT_KEYCODE) == 0x39
    {
        // macOS reports Caps Lock's latched state, not a physical down/up.
        // Send one complete key stroke per state transition; Windows toggles
        // VK_CAPITAL on down and would ignore the old up-only stream.
        let locked = CGEventGetFlags(event) & CGEventFlags::CGEventFlagAlphaShift.bits() != 0;
        if context.caps_lock.replace(Some(locked)) != Some(locked) {
            for down in [true, false] {
                context.emit(CapturedInputEvent::Keyboard(
                    MappedKeyboardEvent::Physical {
                        hid_usage: 0x39,
                        down,
                    },
                ));
            }
        }
        return if context.suppress_local.load(Ordering::Acquire) {
            std::ptr::null_mut()
        } else {
            event
        };
    }
    if event_type == 14 && context.options.capture_keyboard {
        if let Some(value) = consumer::decode(event) {
            let result = lock(&context.consumer).route(value);
            match result {
                arcrelay_input::ConsumerCaptureResult::Pass => {}
                arcrelay_input::ConsumerCaptureResult::Suppress => return std::ptr::null_mut(),
                arcrelay_input::ConsumerCaptureResult::Forward { event, generation } => {
                    context.emit(CapturedInputEvent::ConsumerKey { event, generation });
                    return std::ptr::null_mut();
                }
            }
        }
        return event;
    }
    if matches!(event_type, 29 | 30) {
        let subtype = CGEventGetIntegerValueField(event, 110);
        let generation =
            if context.options.capture_pointer && context.suppress_local.load(Ordering::Acquire) {
                context.system_gesture_generation.load(Ordering::Acquire)
            } else {
                0
            };
        let format_version = context
            .system_gesture_format_version
            .load(Ordering::Acquire);
        let decoded = gesture::decode(event);
        let (blocked, routed) = context.gesture_gate.borrow_mut().route(
            decoded,
            event_type == 29 && subtype == 0,
            event_type == 29 && subtype == 8,
            gesture::contact_count(),
            generation,
            format_version,
        );
        if blocked {
            if let Some(event) = routed {
                context.emit(CapturedInputEvent::SystemGesture { event, generation });
            }
            if context.suppress_local.load(Ordering::Acquire)
                && context.system_gesture_generation.load(Ordering::Acquire) == generation
                && context
                    .system_gesture_format_version
                    .load(Ordering::Acquire)
                    == format_version
            {
                return std::ptr::null_mut();
            }
        }
        return event;
    }
    let captured = match event_type {
        5 | 6 | 7 | 27 if context.options.capture_pointer => {
            Some(CapturedInputEvent::PointerDelta {
                x: CGEventGetDoubleValueField(event, EventField::MOUSE_EVENT_DELTA_X),
                y: CGEventGetDoubleValueField(event, EventField::MOUSE_EVENT_DELTA_Y),
            })
        }
        1 | 2 | 3 | 4 | 25 | 26 if context.options.capture_pointer => {
            let button = CGEventGetIntegerValueField(event, EventField::MOUSE_EVENT_BUTTON_NUMBER)
                .clamp(0, u16::MAX as i64) as u16
                + 1;
            Some(CapturedInputEvent::PointerButton {
                hid_usage: button,
                down: matches!(event_type, 1 | 3 | 25),
            })
        }
        22 if context.options.capture_pointer => {
            let native_quartz_event = (context.suppress_local.load(Ordering::Acquire)
                && context.native_quartz_capture.load(Ordering::Acquire))
            .then(|| flatten_quartz_event(event))
            .flatten();
            Some(CapturedInputEvent::Scroll {
                event: portable_scroll_event(event),
                native_quartz_event,
            })
        }
        10 | 11 if context.options.capture_keyboard => {
            let keycode = CGEventGetIntegerValueField(event, EventField::KEYBOARD_EVENT_KEYCODE)
                .clamp(0, u16::MAX as i64) as u16;
            macos_keycode_to_hid(keycode).map(|hid_usage| {
                let down = event_type == 10;
                if emergency_release_pressed(
                    context.pressed_modifiers.load(Ordering::Acquire),
                    hid_usage,
                    down,
                ) {
                    // This is the last-resort path and must take effect inside
                    // the event tap, even if the async router is waiting on a
                    // dead peer. Passing subsequent events through also lets
                    // the OS-level global shortcut handler run.
                    context.fail_open();
                    CapturedInputEvent::EmergencyRelease
                } else {
                    CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down })
                }
            })
        }
        12 if context.options.capture_keyboard => {
            let keycode = CGEventGetIntegerValueField(event, EventField::KEYBOARD_EVENT_KEYCODE)
                .clamp(0, u16::MAX as i64) as u16;
            macos_keycode_to_hid(keycode).map(|hid_usage| {
                let down = modifier_is_down(hid_usage, CGEventGetFlags(event));
                let bit = emergency_modifier_bit(hid_usage);
                if down {
                    context.pressed_modifiers.fetch_or(bit, Ordering::AcqRel);
                } else {
                    context.pressed_modifiers.fetch_and(!bit, Ordering::AcqRel);
                }
                CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down })
            })
        }
        _ => None,
    };
    if let Some(captured) = captured {
        if let CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down }) =
            &captured
        {
            let result = lock(&context.consumer).physical(*hid_usage, *down);
            match result {
                Some(arcrelay_input::ConsumerCaptureResult::Forward { event, generation }) => {
                    context.emit(CapturedInputEvent::ConsumerKey { event, generation });
                    return std::ptr::null_mut();
                }
                Some(arcrelay_input::ConsumerCaptureResult::Suppress) => {
                    return std::ptr::null_mut()
                }
                _ => {}
            }
        }
        context.emit(captured);
        if context.suppress_local.load(Ordering::Acquire) {
            return std::ptr::null_mut();
        }
    }
    event
}

struct CaptureState {
    run_loop: Arc<AtomicUsize>,
    thread: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct InjectionState {
    consumer: arcrelay_input::ConsumerSequence,
    pressed_keys: BTreeSet<u16>,
    function_tap: keyboard::FunctionTap,
    pressed_buttons: BTreeSet<u16>,
    scroll_gesture_active: bool,
    scroll_momentum_active: bool,
    system_gesture: arcrelay_input::SystemGestureSequence,
    system_gesture_updated: Option<Instant>,
}

impl InjectionState {
    fn observe_scroll(&mut self, event: ScrollEvent) {
        match event.phase {
            ScrollPhase::Began | ScrollPhase::Changed => self.scroll_gesture_active = true,
            ScrollPhase::Ended | ScrollPhase::Cancelled => self.scroll_gesture_active = false,
            _ => {}
        }
        match event.momentum_phase {
            ScrollMomentumPhase::Began | ScrollMomentumPhase::Changed => {
                self.scroll_momentum_active = true;
            }
            ScrollMomentumPhase::Ended => self.scroll_momentum_active = false,
            _ => {}
        }
    }
}

pub struct NativePlatform {
    consumer: Arc<Mutex<arcrelay_input::ConsumerCapture>>,
    brightness: super::brightness::BrightnessWorker,
    device_id: ServiceInstanceId,
    capture: Mutex<Option<CaptureState>>,
    suppress_local: Arc<AtomicBool>,
    cursor_hidden: Arc<Mutex<bool>>,
    native_quartz_capture: Arc<AtomicBool>,
    system_gesture_generation: Arc<AtomicU64>,
    system_gesture_format_version: Arc<AtomicU32>,
    system_gesture_available: Arc<AtomicBool>,
    injection: Mutex<InjectionState>,
}

impl NativePlatform {
    pub fn new(device_id: ServiceInstanceId) -> Self {
        Self {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            brightness: super::brightness::BrightnessWorker::default(),
            device_id,
            capture: Mutex::new(None),
            suppress_local: Arc::new(AtomicBool::new(false)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            native_quartz_capture: Arc::new(AtomicBool::new(false)),
            system_gesture_generation: Arc::new(AtomicU64::new(0)),
            system_gesture_format_version: Arc::new(AtomicU32::new(0)),
            system_gesture_available: Arc::new(AtomicBool::new(true)),
            injection: Mutex::new(InjectionState::default()),
        }
    }

    fn permission_granted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    fn source(&self) -> Result<CGEventSource, PlatformError> {
        EVENT_SOURCE.with(|cached| {
            let mut cached = cached.borrow_mut();
            if cached.is_none() {
                *cached = Some(
                    CGEventSource::new(CGEventSourceStateID::CombinedSessionState).map_err(
                        |_| PlatformError::Operation("CGEventSourceCreate returned null".into()),
                    )?,
                );
            }
            Ok(cached
                .as_ref()
                .expect("Quartz event source was initialized")
                .clone())
        })
    }

    fn key_event(
        &self,
        hid_usage: u16,
        down: bool,
        function_down: bool,
    ) -> Result<CGEvent, PlatformError> {
        let keycode = hid_to_macos_keycode(hid_usage).ok_or_else(|| {
            PlatformError::Unsupported(format!("unsupported HID key 0x{hid_usage:02x}"))
        })?;
        let event = CGEvent::new_keyboard_event(self.source()?, keycode, down)
            .map_err(|_| PlatformError::Operation("cannot create keyboard event".into()))?;
        let mut flags = event.get_flags();
        if function_down {
            flags.insert(CGEventFlags::CGEventFlagSecondaryFn);
        } else {
            flags.remove(CGEventFlags::CGEventFlagSecondaryFn);
        }
        event.set_flags(flags);
        event.set_integer_value_field(FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
        Ok(event)
    }

    fn post_key(
        &self,
        hid_usage: u16,
        down: bool,
        function_down: bool,
    ) -> Result<(), PlatformError> {
        let event = self.key_event(hid_usage, down, function_down)?;
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn post_button(&self, hid_usage: u16, down: bool) -> Result<(), PlatformError> {
        if hid_usage == 0 {
            return Err(PlatformError::Unsupported("mouse button usage 0".into()));
        }
        let current = CGEvent::new(self.source()?)
            .map_err(|_| PlatformError::Operation("cannot read pointer location".into()))?
            .location();
        let button = u32::from(hid_usage - 1);
        let mouse_type = match (button, down) {
            (0, true) => 1,
            (0, false) => 2,
            (1, true) => 3,
            (1, false) => 4,
            (_, true) => 25,
            (_, false) => 26,
        };
        unsafe {
            let event = CGEventCreateMouseEvent(std::ptr::null(), mouse_type, current, button);
            if event.is_null() {
                return Err(PlatformError::Operation(
                    "CGEventCreateMouseEvent returned null".into(),
                ));
            }
            CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
            CGEventPost(0, event);
            CFRelease(event);
        }
        Ok(())
    }

    fn release_state(&self) -> Result<(), PlatformError> {
        let mut state = lock(&self.injection);
        state.function_tap = keyboard::FunctionTap::default();
        state.consumer.clear();
        self.brightness.cancel();
        if let Some(cancel) = state.system_gesture.cancel() {
            if let Err(error) = gesture::post(cancel) {
                tracing::warn!(%error, "Arc Input could not cancel the system gesture");
            }
        }
        state.system_gesture_updated = None;
        if state.scroll_gesture_active {
            let _ = self.post_portable_scroll(ScrollEvent {
                phase: ScrollPhase::Cancelled,
                unit: ScrollUnit::Pixel,
                ..ScrollEvent::default()
            });
        }
        if state.scroll_momentum_active {
            let _ = self.post_portable_scroll(ScrollEvent {
                momentum_phase: ScrollMomentumPhase::Ended,
                unit: ScrollUnit::Pixel,
                ..ScrollEvent::default()
            });
        }
        let pressed_keys = std::mem::take(&mut state.pressed_keys);
        let function_was_down = pressed_keys.contains(&HID_KEY_FUNCTION);
        for key in pressed_keys {
            let _ = self.post_key(key, false, function_was_down && key != HID_KEY_FUNCTION);
        }
        for button in std::mem::take(&mut state.pressed_buttons) {
            let _ = self.post_button(button, false);
        }
        state.scroll_gesture_active = false;
        state.scroll_momentum_active = false;
        Ok(())
    }

    fn post_portable_scroll(&self, event: ScrollEvent) -> Result<(), PlatformError> {
        if !event.is_finite() {
            return Err(PlatformError::Operation(
                "scroll delta is not finite".into(),
            ));
        }
        let continuous = event.unit == ScrollUnit::Pixel;
        let unit = if continuous {
            ScrollEventUnit::PIXEL
        } else {
            ScrollEventUnit::LINE
        };
        let scroll = CGEvent::new_scroll_event(
            self.source()?,
            unit,
            2,
            event.delta_y.round() as i32,
            event.delta_x.round() as i32,
            0,
        )
        .map_err(|_| PlatformError::Operation("cannot create scroll event".into()))?;
        scroll.set_integer_value_field(
            EventField::SCROLL_WHEEL_EVENT_IS_CONTINUOUS,
            i64::from(continuous),
        );
        scroll.set_integer_value_field(
            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
            (event.delta_y * 65_536.0).round() as i64,
        );
        scroll.set_integer_value_field(
            EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_2,
            (event.delta_x * 65_536.0).round() as i64,
        );
        if continuous {
            scroll.set_double_value_field(
                EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_1,
                event.delta_y,
            );
            scroll.set_double_value_field(
                EventField::SCROLL_WHEEL_EVENT_POINT_DELTA_AXIS_2,
                event.delta_x,
            );
        }
        scroll.set_integer_value_field(FIELD_SCROLL_WHEEL_EVENT_SCROLL_COUNT, 1);
        scroll.set_integer_value_field(
            FIELD_SCROLL_WHEEL_EVENT_SCROLL_PHASE,
            quartz_scroll_phase(event.phase),
        );
        scroll.set_integer_value_field(
            FIELD_SCROLL_WHEEL_EVENT_MOMENTUM_PHASE,
            quartz_momentum_phase(event.momentum_phase),
        );
        scroll.set_integer_value_field(FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
        scroll.post(CGEventTapLocation::HID);
        Ok(())
    }
}

impl InputCapturePort for NativePlatform {
    fn set_consumer_capture(&self, generation: u64, mask: u32) {
        lock(&self.consumer).set_route(generation, mask);
    }
    fn set_consumer_shortcuts(&self, shortcuts: Vec<arcrelay_input::ConsumerShortcut>) {
        lock(&self.consumer).set_shortcuts(shortcuts);
    }
    fn capabilities(&self) -> PlatformCapabilities {
        let permission = Self::permission_granted();
        PlatformCapabilities {
            can_capture_pointer: permission,
            can_capture_keyboard: permission,
            can_suppress_local_input: permission,
            can_place_internal_barrier: false,
            can_inject_absolute_pointer: permission,
            can_inject_keyboard: permission,
            can_inject_app_pointer: false,
            can_control_elevated_apps: permission,
            can_persist_permission: true,
            can_capture_native_quartz_events: permission,
            can_inject_native_quartz_events: permission,
            can_capture_precision_touchpad_events: false,
            can_inject_precision_touchpad_events: false,
            can_capture_system_gestures: permission
                && gesture::supported()
                && self.system_gesture_available.load(Ordering::Acquire),
            can_inject_system_gestures: permission && gesture::supported(),
            system_gesture_format_version: arcrelay_input::MAX_SYSTEM_GESTURE_FORMAT_VERSION,
            consumer_capture_mask: if permission
                && self.system_gesture_available.load(Ordering::Acquire)
            {
                arcrelay_input::ConsumerKey::ALL_MASK
            } else {
                0
            },
            consumer_inject_mask: if permission {
                arcrelay_input::ConsumerKey::ALL_MASK
            } else {
                0
            },
            brightness_display_ids: self.brightness.supported(),
            limitation: (!permission)
                .then(|| "macOS Accessibility permission is required".to_string()),
        }
    }

    fn start(
        &self,
        options: CaptureOptions,
    ) -> Result<Receiver<CapturedInputEvent>, PlatformError> {
        if !Self::permission_granted() {
            return Err(PlatformError::PermissionDenied(
                "macOS Accessibility permission is required".into(),
            ));
        }
        let mut active = lock(&self.capture);
        if active.is_some() {
            return Err(PlatformError::Operation("capture is already active".into()));
        }
        let (events_tx, events_rx) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        self.set_suppress_local(options.suppress_local)?;
        let suppress_local = self.suppress_local.clone();
        let cursor_hidden = self.cursor_hidden.clone();
        let consumer = self.consumer.clone();
        let native_quartz_capture = self.native_quartz_capture.clone();
        let system_gesture_generation = self.system_gesture_generation.clone();
        let system_gesture_format_version = self.system_gesture_format_version.clone();
        let system_gesture_available = self.system_gesture_available.clone();
        let run_loop = Arc::new(AtomicUsize::new(0));
        let run_loop_thread = run_loop.clone();
        let thread = match std::thread::Builder::new()
            .name("arc-input-quartz-capture".into())
            .spawn(move || unsafe {
                let contact_tracking = gesture::ContactTrackingGuard::start();
                if !contact_tracking.available() {
                    tracing::warn!("Arc Input could not observe trackpad contact count; legacy swipe routing remains enabled");
                }
                let context = Box::new(CaptureContext {
                    consumer,
                    events: events_tx,
                    options,
                    suppress_local,
                    cursor_hidden,
                    native_quartz_capture,
                    system_gesture_generation,
                    system_gesture_format_version,
                    gesture_gate: RefCell::new(gesture::CaptureGate::default()),
                    pressed_modifiers: AtomicUsize::new(0),
                    caps_lock: std::cell::Cell::new(None),
                    tap: AtomicUsize::new(0),
                    gesture_tap: AtomicUsize::new(0),
                    overflowed: AtomicBool::new(false),
                });
                let context = Box::into_raw(context);
                let tap = CGEventTapCreate(0, 0, 0, EVENT_MASK, capture_callback, context.cast());
                if tap.is_null() {
                    drop(Box::from_raw(context));
                    let _ = started_tx.send(Err(PlatformError::PermissionDenied(
                        "Quartz event tap could not be created".into(),
                    )));
                    return;
                }
                (*context).tap.store(tap as usize, Ordering::Release);
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                // Keep the existing HID keyboard/mouse tap unchanged. DockSwipe
                // arrives on a separate session-level stream on the same thread.
                let mut gesture_tap = CGEventTapCreate(
                    1, 0, 0, (if gesture::supported() { gesture::MASK } else { 0 }) | (1 << 14),
                    capture_callback, context.cast(),
                );
                let mut gesture_source = if gesture_tap.is_null() {
                    std::ptr::null_mut()
                } else {
                    CFMachPortCreateRunLoopSource(std::ptr::null(), gesture_tap, 0)
                };
                if source.is_null() {
                    if !gesture_source.is_null() {
                        CFRelease(gesture_source);
                    }
                    if !gesture_tap.is_null() {
                        CFMachPortInvalidate(gesture_tap);
                        CFRelease(gesture_tap);
                    }
                    CFMachPortInvalidate(tap);
                    CFRelease(tap);
                    drop(Box::from_raw(context));
                    let _ = started_tx.send(Err(PlatformError::Operation(
                        "cannot create Quartz keyboard/pointer capture source".into(),
                    )));
                    return;
                }
                let gesture_ready = !gesture_tap.is_null() && !gesture_source.is_null();
                system_gesture_available.store(gesture_ready, Ordering::Release);
                if !gesture_ready {
                    if !gesture_source.is_null() {
                        CFRelease(gesture_source);
                    }
                    if !gesture_tap.is_null() {
                        CFMachPortInvalidate(gesture_tap);
                        CFRelease(gesture_tap);
                    }
                    gesture_tap = std::ptr::null_mut();
                    gesture_source = std::ptr::null_mut();
                    if gesture::supported() {
                        tracing::warn!("Arc Input system gesture tap unavailable; keyboard, pointer and scroll remain enabled");
                    }
                }
                (*context).gesture_tap.store(gesture_tap as usize, Ordering::Release);
                let loop_ref = CFRunLoopGetCurrent();
                run_loop_thread.store(loop_ref as usize, Ordering::Release);
                CFRunLoopAddSource(loop_ref, source, kCFRunLoopCommonModes);
                if !gesture_source.is_null() {
                    CFRunLoopAddSource(loop_ref, gesture_source, kCFRunLoopCommonModes);
                    CGEventTapEnable(gesture_tap, true);
                }
                CGEventTapEnable(tap, true);
                let _ = started_tx.send(Ok(()));
                CFRunLoopRun();
                run_loop_thread.store(0, Ordering::Release);
                if !gesture_tap.is_null() {
                    CGEventTapEnable(gesture_tap, false);
                    CFRunLoopRemoveSource(loop_ref, gesture_source, kCFRunLoopCommonModes);
                    CFMachPortInvalidate(gesture_tap);
                    CFRelease(gesture_source);
                    CFRelease(gesture_tap);
                }
                CGEventTapEnable(tap, false);
                CFRunLoopRemoveSource(loop_ref, source, kCFRunLoopCommonModes);
                CFMachPortInvalidate(tap);
                CFRelease(source);
                CFRelease(tap);
                drop(Box::from_raw(context));
            }) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = self.set_suppress_local(false);
                return Err(PlatformError::Operation(error.to_string()));
            }
        };
        match started_rx.recv() {
            Ok(Ok(())) => {
                *active = Some(CaptureState {
                    run_loop,
                    thread: Some(thread),
                });
                Ok(events_rx)
            }
            Ok(Err(error)) => {
                let _ = thread.join();
                let _ = self.set_suppress_local(false);
                Err(error)
            }
            Err(error) => {
                let _ = thread.join();
                let _ = self.set_suppress_local(false);
                Err(PlatformError::Operation(error.to_string()))
            }
        }
    }

    fn set_suppress_local(&self, suppress: bool) -> Result<(), PlatformError> {
        if !suppress {
            lock(&self.consumer).set_route(0, 0);
        }
        if suppress {
            // Suppressing events freezes the source pointer but does not hide
            // it. Hide it while control is on another device so only the
            // active target cursor remains visible.
            set_cursor_hidden(&self.cursor_hidden, true)?;
            self.suppress_local.store(true, Ordering::Release);
        } else {
            // Fail open before changing cursor visibility so a Quartz error
            // can never leave local input blocked.
            self.suppress_local.store(false, Ordering::Release);
            self.native_quartz_capture.store(false, Ordering::Release);
            self.system_gesture_generation.store(0, Ordering::Release);
            set_cursor_hidden(&self.cursor_hidden, false)?;
        }
        Ok(())
    }

    fn set_native_quartz_capture_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        self.native_quartz_capture.store(enabled, Ordering::Release);
        Ok(())
    }

    fn maintain_cursor_visibility(&self) -> Result<(), PlatformError> {
        cursor::maintain_hidden(&self.cursor_hidden)
    }

    fn current_pointer_position(&self) -> Result<LogicalPoint, PlatformError> {
        let point = CGEvent::new(self.source()?)
            .map_err(|_| PlatformError::Operation("cannot read pointer location".into()))?
            .location();
        Ok(LogicalPoint {
            x: point.x,
            y: point.y,
        })
    }

    fn set_system_gesture_capture_generation(
        &self,
        generation: u64,
        format_version: u32,
    ) -> Result<(), PlatformError> {
        self.system_gesture_generation.store(0, Ordering::Release);
        self.system_gesture_format_version
            .store(format_version, Ordering::Release);
        self.system_gesture_generation.store(
            if gesture::supported() && self.system_gesture_available.load(Ordering::Acquire) {
                generation
            } else {
                0
            },
            Ordering::Release,
        );
        Ok(())
    }

    fn stop(&self) -> Result<(), PlatformError> {
        lock(&self.consumer).reset();
        self.suppress_local.store(false, Ordering::Release);
        self.native_quartz_capture.store(false, Ordering::Release);
        self.system_gesture_generation.store(0, Ordering::Release);
        let cursor_result = set_cursor_hidden(&self.cursor_hidden, false);
        let mut state = lock(&self.capture).take();
        if let Some(state) = state.as_mut() {
            let loop_ref = state.run_loop.load(Ordering::Acquire) as CFRunLoopRef;
            if !loop_ref.is_null() {
                unsafe { CFRunLoopStop(loop_ref) };
            }
            if let Some(thread) = state.thread.take() {
                thread
                    .join()
                    .map_err(|_| PlatformError::Operation("capture thread panicked".into()))?;
            }
        }
        self.release_state()?;
        cursor_result
    }
}

impl InputInjectionPort for NativePlatform {
    fn release_consumer_keys(&self) {
        lock(&self.injection).consumer.clear();
        self.brightness.cancel();
    }
    fn refresh_consumer_capabilities(&self) {
        self.brightness.refresh();
    }
    fn consumer_key(
        &self,
        event: arcrelay_input::ConsumerKeyEvent,
        display: &DisplayId,
    ) -> Result<(), PlatformError> {
        let mut state = lock(&self.injection);
        state.function_tap.cancel();
        if state.consumer.apply(event) {
            if event.key.is_brightness() {
                self.brightness.adjust(
                    display,
                    event.key == arcrelay_input::ConsumerKey::BrightnessUp,
                )?;
            } else {
                consumer::pulse(event.key)?;
            }
        }
        Ok(())
    }
    fn capabilities(&self) -> PlatformCapabilities {
        InputCapturePort::capabilities(self)
    }

    fn place_pointer(&self, _: &DisplayId, point: LogicalPoint) -> Result<(), PlatformError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(PlatformError::Operation(
                "pointer coordinate is not finite".into(),
            ));
        }
        let (event_type, button) = {
            let state = lock(&self.injection);
            if state.pressed_buttons.contains(&1) {
                (
                    CGEventType::LeftMouseDragged,
                    core_graphics::event::CGMouseButton::Left,
                )
            } else if state.pressed_buttons.contains(&2) {
                (
                    CGEventType::RightMouseDragged,
                    core_graphics::event::CGMouseButton::Right,
                )
            } else if state.pressed_buttons.is_empty() {
                (
                    CGEventType::MouseMoved,
                    core_graphics::event::CGMouseButton::Left,
                )
            } else {
                (
                    CGEventType::OtherMouseDragged,
                    core_graphics::event::CGMouseButton::Center,
                )
            }
        };
        let event = CGEvent::new_mouse_event(
            self.source()?,
            event_type,
            CGPoint::new(point.x, point.y),
            button,
        )
        .map_err(|_| PlatformError::Operation("cannot create pointer event".into()))?;
        event.set_integer_value_field(FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
        event.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn apply_keyboard(&self, event: &MappedKeyboardEvent) -> Result<(), PlatformError> {
        match event {
            MappedKeyboardEvent::Physical { hid_usage, down } => {
                let mut state = lock(&self.injection);
                let function_down = if *hid_usage == HID_KEY_FUNCTION {
                    *down
                } else {
                    state.pressed_keys.contains(&HID_KEY_FUNCTION)
                };
                self.post_key(*hid_usage, *down, function_down)?;
                if *down {
                    state.pressed_keys.insert(*hid_usage);
                } else {
                    state.pressed_keys.remove(hid_usage);
                }
                let alone = state
                    .pressed_keys
                    .iter()
                    .all(|key| *key == HID_KEY_FUNCTION)
                    && state.pressed_buttons.is_empty();
                if state.function_tap.key(*hid_usage, *down, alone) {
                    keyboard::perform_function_tap(self)?;
                }
            }
            MappedKeyboardEvent::Semantic { action, down } => {
                let chord = target_chord(*action, OsFamily::MacOs);
                if *down {
                    for modifier in &chord.modifiers {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: true,
                        })?;
                    }
                    self.apply_keyboard(&MappedKeyboardEvent::Physical {
                        hid_usage: chord.key,
                        down: true,
                    })?;
                } else {
                    self.apply_keyboard(&MappedKeyboardEvent::Physical {
                        hid_usage: chord.key,
                        down: false,
                    })?;
                    for modifier in chord.modifiers.iter().rev() {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: false,
                        })?;
                    }
                }
            }
            MappedKeyboardEvent::TextCommit(text) => {
                lock(&self.injection).function_tap.cancel();
                for chunk in text.chars().collect::<Vec<_>>().chunks(16) {
                    let text = chunk.iter().collect::<String>();
                    let event = CGEvent::new_keyboard_event(self.source()?, 0, true)
                        .map_err(|_| PlatformError::Operation("cannot create text event".into()))?;
                    event
                        .set_integer_value_field(FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
                    event.set_string(&text);
                    event.post(CGEventTapLocation::HID);
                }
            }
        }
        Ok(())
    }

    fn pointer_button(&self, hid_usage: u16, down: bool) -> Result<(), PlatformError> {
        self.post_button(hid_usage, down)?;
        let mut state = lock(&self.injection);
        state.function_tap.cancel();
        if down {
            state.pressed_buttons.insert(hid_usage);
        } else {
            state.pressed_buttons.remove(&hid_usage);
        }
        Ok(())
    }

    fn scroll(&self, event: ScrollEvent) -> Result<(), PlatformError> {
        let mut state = lock(&self.injection);
        state.function_tap.cancel();
        self.post_portable_scroll(event)?;
        state.observe_scroll(event);
        Ok(())
    }

    fn native_quartz_scroll(&self, data: &[u8]) -> Result<(), PlatformError> {
        let mut state = lock(&self.injection);
        state.function_tap.cancel();
        let event = scroll::prepare_native(data)?;
        event.post();
        // Native gestures must be cancelled on handoff/disconnect as well as
        // portable ones, or AppKit can retain a stale scrolling transaction.
        state.observe_scroll(event.scroll_event());
        Ok(())
    }

    fn release_all(&self) -> Result<(), PlatformError> {
        self.release_state()
    }

    fn system_gesture(
        &self,
        event: arcrelay_input::SystemGestureEvent,
    ) -> Result<(), PlatformError> {
        if !gesture::supported() || !Self::permission_granted() {
            return Err(PlatformError::Unsupported(
                "system gestures require supported macOS and Accessibility permission".into(),
            ));
        }
        let mut state = lock(&self.injection);
        state.function_tap.cancel();
        let phases = state
            .system_gesture
            .apply(event)
            .map_err(|error| PlatformError::Operation(error.into()))?;
        if !phases.is_empty() {
            state.system_gesture_updated = state.system_gesture.is_active().then(Instant::now);
        }
        for phase in phases {
            if let Err(error) = gesture::post(phase) {
                if let Some(cancel) = state.system_gesture.cancel() {
                    let _ = gesture::post(cancel);
                }
                return Err(error);
            }
        }
        Ok(())
    }

    fn maintain_system_gesture(&self) -> Result<bool, PlatformError> {
        let mut state = lock(&self.injection);
        if state
            .system_gesture_updated
            .is_some_and(|updated| updated.elapsed() >= Duration::from_secs(2))
        {
            state.system_gesture_updated = None;
            if let Some(cancel) = state.system_gesture.cancel() {
                gesture::post(cancel)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}

impl DisplayInventoryPort for NativePlatform {
    fn inventory(&self) -> Result<DisplayInventory, PlatformError> {
        let active_displays = CGDisplay::active_displays().map_err(|error| {
            PlatformError::Operation(format!("CGGetActiveDisplayList: {error}"))
        })?;
        let mut representatives = BTreeSet::new();
        for id in active_displays {
            let display = CGDisplay::new(id);
            let mirrored = display.mirrors_display();
            let hardware_primary = display.primary_display();
            let representative = if mirrored != 0 {
                mirrored
            } else if display.is_in_hw_mirror_set() && hardware_primary != 0 {
                hardware_primary
            } else {
                id
            };
            representatives.insert(representative);
        }
        let revision = InventoryRevision(1);
        let displays = representatives
            .into_iter()
            .map(|id| {
                let display = CGDisplay::new(id);
                let bounds = display.bounds();
                let screen_size = display.screen_size();
                let pixel_width = display.pixels_wide().min(u64::from(u32::MAX)) as u32;
                let pixel_height = display.pixels_high().min(u64::from(u32::MAX)) as u32;
                let fingerprint_value = format!(
                    "{:x}-{:x}-{:x}-{}x{}",
                    display.vendor_number(),
                    display.model_number(),
                    display.serial_number(),
                    pixel_width,
                    pixel_height
                );
                let fingerprint = DisplayFingerprint::parse(fingerprint_value.clone())
                    .map_err(|error| PlatformError::Operation(error.to_string()))?;
                let display_id = DisplayId::parse(format!("mac-{fingerprint_value}"))
                    .map_err(|error| PlatformError::Operation(error.to_string()))?;
                let width_um = (screen_size.width * 1000.0).round() as i64;
                let height_um = (screen_size.height * 1000.0).round() as i64;
                let physical_known = width_um > 0 && height_um > 0;
                let fallback_width = (bounds.size.width * 264.583).round() as i64;
                let fallback_height = (bounds.size.height * 264.583).round() as i64;
                let physical_width = if physical_known {
                    width_um
                } else {
                    fallback_width.max(1)
                };
                let physical_height = if physical_known {
                    height_um
                } else {
                    fallback_height.max(1)
                };
                let rotation = match display.rotation().round() as i32 {
                    90 => DisplayRotation::Degrees90,
                    180 => DisplayRotation::Degrees180,
                    270 => DisplayRotation::Degrees270,
                    _ => DisplayRotation::Degrees0,
                };
                Ok(DisplaySurface {
                    display_id,
                    device_id: self.device_id.clone(),
                    fingerprint,
                    name: format!("Display {id}"),
                    pixel_size: SizeU32 {
                        width: pixel_width,
                        height: pixel_height,
                    },
                    logical_bounds: LogicalRect {
                        x: bounds.origin.x,
                        y: bounds.origin.y,
                        width: bounds.size.width,
                        height: bounds.size.height,
                    },
                    scale_factor: ScaleFactor(pixel_width as f64 / bounds.size.width.max(1.0)),
                    physical_size_um: SizeI64 {
                        width: physical_width,
                        height: physical_height,
                    },
                    rotation,
                    desk_rect_um: arcrelay_input::DeskRectUm {
                        x: (bounds.origin.x * physical_width as f64 / bounds.size.width.max(1.0))
                            .round() as i64,
                        y: (bounds.origin.y * physical_height as f64 / bounds.size.height.max(1.0))
                            .round() as i64,
                        width: physical_width,
                        height: physical_height,
                    },
                    geometry_confidence: if physical_known {
                        GeometryConfidence::HardwareReported
                    } else {
                        GeometryConfidence::Estimated
                    },
                    inventory_revision: revision,
                })
            })
            .collect::<Result<Vec<_>, PlatformError>>()?;
        let inventory = DisplayInventory {
            device_id: self.device_id.clone(),
            revision,
            displays,
        };
        inventory
            .validate()
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        Ok(inventory)
    }
}

impl Drop for NativePlatform {
    fn drop(&mut self) {
        let _ = InputCapturePort::stop(self);
        let _ = self.release_state();
    }
}

fn modifier_is_down(hid_usage: u16, flags: u64) -> bool {
    let mask = match hid_usage {
        0xe0 | 0xe4 => 0x0004_0000,
        0xe1 | 0xe5 => 0x0002_0000,
        0xe2 | 0xe6 => 0x0008_0000,
        0xe3 | 0xe7 => 0x0010_0000,
        HID_KEY_FUNCTION => CGEventFlags::CGEventFlagSecondaryFn.bits(),
        _ => 0,
    };
    flags & mask != 0
}

fn macos_keycode_to_hid(keycode: u16) -> Option<u16> {
    if keycode == MACOS_KEYCODE_FUNCTION {
        return Some(HID_KEY_FUNCTION);
    }
    (0x04..=0xe7).find(|usage| hid_to_macos_keycode(*usage) == Some(keycode))
}

fn hid_to_macos_keycode(usage: u16) -> Option<u16> {
    Some(match usage {
        HID_KEY_FUNCTION => MACOS_KEYCODE_FUNCTION,
        0x04 => 0x00,
        0x05 => 0x0B,
        0x06 => 0x08,
        0x07 => 0x02,
        0x08 => 0x0E,
        0x09 => 0x03,
        0x0A => 0x05,
        0x0B => 0x04,
        0x0C => 0x22,
        0x0D => 0x26,
        0x0E => 0x28,
        0x0F => 0x25,
        0x10 => 0x2E,
        0x11 => 0x2D,
        0x12 => 0x1F,
        0x13 => 0x23,
        0x14 => 0x0C,
        0x15 => 0x0F,
        0x16 => 0x01,
        0x17 => 0x11,
        0x18 => 0x20,
        0x19 => 0x09,
        0x1A => 0x0D,
        0x1B => 0x07,
        0x1C => 0x10,
        0x1D => 0x06,
        0x1E => 0x12,
        0x1F => 0x13,
        0x20 => 0x14,
        0x21 => 0x15,
        0x22 => 0x17,
        0x23 => 0x16,
        0x24 => 0x1A,
        0x25 => 0x1C,
        0x26 => 0x19,
        0x27 => 0x1D,
        0x28 => 0x24,
        0x29 => 0x35,
        0x2A => 0x33,
        0x2B => 0x30,
        0x2C => 0x31,
        0x2D => 0x1B,
        0x2E => 0x18,
        0x2F => 0x21,
        0x30 => 0x1E,
        0x31 => 0x2A,
        0x33 => 0x29,
        0x34 => 0x27,
        0x35 => 0x32,
        0x36 => 0x2B,
        0x37 => 0x2F,
        0x38 => 0x2C,
        0x39 => 0x39,
        0x3A => 0x7A,
        0x3B => 0x78,
        0x3C => 0x63,
        0x3D => 0x76,
        0x3E => 0x60,
        0x3F => 0x61,
        0x40 => 0x62,
        0x41 => 0x64,
        0x42 => 0x65,
        0x43 => 0x6D,
        0x44 => 0x67,
        0x45 => 0x6F,
        0x49 => 0x72,
        0x4A => 0x73,
        0x4B => 0x74,
        0x4C => 0x75,
        0x4D => 0x77,
        0x4E => 0x79,
        0x4F => 0x7C,
        0x50 => 0x7B,
        0x51 => 0x7D,
        0x52 => 0x7E,
        0xE0 => 0x3B,
        0xE1 => 0x38,
        0xE2 => 0x3A,
        0xE3 => 0x37,
        0xE4 => 0x3E,
        0xE5 => 0x3C,
        0xE6 => 0x3D,
        0xE7 => 0x36,
        _ => return None,
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod emergency_shortcut_tests {
    use super::*;

    #[test]
    fn injected_pointer_placement_does_not_become_physical_takeover_input() {
        let (events, receiver) = mpsc::sync_channel(4);
        let mut context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: false,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: Arc::new(AtomicBool::new(false)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            native_quartz_capture: Arc::new(AtomicBool::new(false)),
            system_gesture_generation: Arc::new(AtomicU64::new(0)),
            system_gesture_format_version: Arc::new(AtomicU32::new(0)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        unsafe {
            // Construct and invoke the callback only, without posting/moving.
            let event =
                CGEventCreateMouseEvent(std::ptr::null(), 5, CGPoint { x: 100.0, y: 100.0 }, 0);
            assert!(!event.is_null());
            assert_eq!(
                capture_callback(
                    std::ptr::null(),
                    5,
                    event,
                    (&mut context as *mut CaptureContext).cast()
                ),
                event
            );
            assert!(matches!(
                receiver.try_recv(),
                Ok(CapturedInputEvent::PointerDelta { .. })
            ));
            CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
            assert_eq!(
                capture_callback(
                    std::ptr::null(),
                    5,
                    event,
                    (&mut context as *mut CaptureContext).cast()
                ),
                event
            );
            assert!(receiver.try_recv().is_err());
            CFRelease(event);
        }
    }

    #[test]
    fn arcrelay_tagged_paste_keys_do_not_become_physical_takeover_input() {
        let (events, receiver) = mpsc::sync_channel(4);
        let mut context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: false,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: Arc::new(AtomicBool::new(false)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            native_quartz_capture: Arc::new(AtomicBool::new(false)),
            system_gesture_generation: Arc::new(AtomicU64::new(0)),
            system_gesture_format_version: Arc::new(AtomicU32::new(0)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        unsafe {
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), 0x09, true);
            assert!(!event.is_null());
            CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
            assert_eq!(
                capture_callback(
                    std::ptr::null(),
                    CGEventType::KeyDown as u32,
                    event,
                    (&mut context as *mut CaptureContext).cast(),
                ),
                event
            );
            assert!(receiver.try_recv().is_err());
            CFRelease(event);
        }
    }

    #[test]
    fn arcrelay_tagged_system_gestures_do_not_become_physical_takeover_input() {
        let (events, receiver) = mpsc::sync_channel(4);
        let mut context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: false,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: Arc::new(AtomicBool::new(false)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            native_quartz_capture: Arc::new(AtomicBool::new(false)),
            system_gesture_generation: Arc::new(AtomicU64::new(7)),
            system_gesture_format_version: Arc::new(AtomicU32::new(2)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        unsafe {
            for event_type in [29, 30] {
                let event = CGEventCreate(std::ptr::null());
                assert!(!event.is_null());
                CGEventSetIntegerValueField(
                    event,
                    FIELD_EVENT_SOURCE_USER_DATA,
                    ARC_INPUT_EVENT_TAG,
                );
                assert_eq!(
                    capture_callback(
                        std::ptr::null(),
                        event_type,
                        event,
                        (&mut context as *mut CaptureContext).cast(),
                    ),
                    event
                );
                assert!(receiver.try_recv().is_err());
                CFRelease(event);
            }
        }
    }

    #[test]
    fn requires_command_option_shift_and_escape_down() {
        let required = MODIFIER_COMMAND | MODIFIER_OPTION | MODIFIER_SHIFT;
        assert!(emergency_release_pressed(required, 0x29, true));
        assert!(emergency_release_pressed(required, 0x10, true));
        assert!(!emergency_release_pressed(required, 0x29, false));
        assert!(!emergency_release_pressed(
            MODIFIER_COMMAND | MODIFIER_SHIFT,
            0x29,
            true
        ));
        assert!(!emergency_release_pressed(required, 0x11, true));
    }

    #[test]
    fn function_key_roundtrips_through_quartz_without_posting() {
        let platform = NativePlatform::new(ServiceInstanceId::parse("test-device").unwrap());
        assert_eq!(
            macos_keycode_to_hid(MACOS_KEYCODE_FUNCTION),
            Some(HID_KEY_FUNCTION)
        );
        assert_eq!(
            hid_to_macos_keycode(HID_KEY_FUNCTION),
            Some(MACOS_KEYCODE_FUNCTION)
        );

        let down = platform.key_event(HID_KEY_FUNCTION, true, true).unwrap();
        assert_eq!(down.get_type() as u32, CGEventType::FlagsChanged as u32);
        assert!(modifier_is_down(HID_KEY_FUNCTION, down.get_flags().bits()));

        let function_row = platform.key_event(0x3a, true, true).unwrap();
        assert!(function_row
            .get_flags()
            .contains(CGEventFlags::CGEventFlagSecondaryFn));

        let up = platform.key_event(HID_KEY_FUNCTION, false, false).unwrap();
        assert_eq!(up.get_type() as u32, CGEventType::FlagsChanged as u32);
        assert!(!modifier_is_down(HID_KEY_FUNCTION, up.get_flags().bits()));
    }

    #[test]
    fn capture_callback_forwards_function_key_down_and_up() {
        let (events, receiver) = mpsc::sync_channel(2);
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
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };

        unsafe {
            for down in [true, false] {
                let event =
                    CGEventCreateKeyboardEvent(std::ptr::null(), MACOS_KEYCODE_FUNCTION, down);
                assert!(!event.is_null());
                assert_eq!(CGEventGetType(event), CGEventType::FlagsChanged as u32);
                assert!(capture_callback(
                    std::ptr::null(),
                    CGEventType::FlagsChanged as u32,
                    event,
                    (&mut context as *mut CaptureContext).cast(),
                )
                .is_null());
                assert_eq!(
                    receiver.try_recv().unwrap(),
                    CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical {
                        hid_usage: HID_KEY_FUNCTION,
                        down,
                    })
                );
                CFRelease(event);
            }
        }
    }

    #[test]
    fn caps_lock_transitions_produce_complete_strokes_and_ignore_duplicate_flags() {
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
            caps_lock: std::cell::Cell::new(Some(false)),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        unsafe {
            for (locked, changed) in [
                (true, true),
                (true, false),
                (false, true),
                (false, false),
                (true, true),
            ] {
                let event = CGEventCreateKeyboardEvent(std::ptr::null(), 0x39, true);
                assert!(!event.is_null());
                CGEventSetFlags(
                    event,
                    if locked {
                        CGEventFlags::CGEventFlagAlphaShift.bits()
                    } else {
                        0
                    },
                );
                assert!(capture_callback(
                    std::ptr::null(),
                    12,
                    event,
                    (&mut context as *mut CaptureContext).cast()
                )
                .is_null());
                CFRelease(event);
                if changed {
                    for down in [true, false] {
                        assert_eq!(
                            receiver.try_recv().unwrap(),
                            CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical {
                                hid_usage: 0x39,
                                down
                            })
                        );
                    }
                }
                assert!(receiver.try_recv().is_err());
            }
            // Synthetic Caps Lock events must not become another toggle.
            let event = CGEventCreateKeyboardEvent(std::ptr::null(), 0x39, true);
            CGEventSetIntegerValueField(event, FIELD_EVENT_SOURCE_USER_DATA, ARC_INPUT_EVENT_TAG);
            assert_eq!(
                capture_callback(
                    std::ptr::null(),
                    12,
                    event,
                    (&mut context as *mut CaptureContext).cast()
                ),
                event
            );
            CFRelease(event);
            assert!(receiver.try_recv().is_err());
        }
    }

    #[test]
    fn capture_queue_overflow_immediately_fails_open() {
        let (events, _receiver) = mpsc::sync_channel(0);
        let suppress_local = Arc::new(AtomicBool::new(true));
        let native_quartz_capture = Arc::new(AtomicBool::new(true));
        let cursor_hidden = Arc::new(Mutex::new(false));
        let context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: true,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: suppress_local.clone(),
            cursor_hidden: cursor_hidden.clone(),
            native_quartz_capture: native_quartz_capture.clone(),
            system_gesture_generation: Arc::new(AtomicU64::new(7)),
            system_gesture_format_version: Arc::new(AtomicU32::new(2)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };

        context.emit(CapturedInputEvent::PointerDelta { x: 1.0, y: 0.0 });

        assert!(!suppress_local.load(Ordering::Acquire));
        assert!(!native_quartz_capture.load(Ordering::Acquire));
        assert_eq!(context.system_gesture_generation.load(Ordering::Acquire), 0);
        assert!(!*lock(&cursor_hidden));
        assert!(context.overflowed.load(Ordering::Acquire));
    }

    #[test]
    fn portable_scroll_phases_match_public_quartz_values() {
        for phase in [
            ScrollPhase::MayBegin,
            ScrollPhase::Began,
            ScrollPhase::Changed,
            ScrollPhase::Ended,
            ScrollPhase::Cancelled,
        ] {
            assert_eq!(portable_scroll_phase(quartz_scroll_phase(phase)), phase);
        }
        for phase in [
            ScrollMomentumPhase::Began,
            ScrollMomentumPhase::Changed,
            ScrollMomentumPhase::Ended,
        ] {
            assert_eq!(portable_momentum_phase(quartz_momentum_phase(phase)), phase);
        }
    }

    #[test]
    fn disabled_tap_with_null_event_still_releases_input() {
        let (events, receiver) = mpsc::sync_channel(2);
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
            native_quartz_capture: Arc::new(AtomicBool::new(true)),
            system_gesture_generation: Arc::new(AtomicU64::new(9)),
            system_gesture_format_version: Arc::new(AtomicU32::new(2)),
            gesture_gate: RefCell::new(gesture::CaptureGate::default()),
            pressed_modifiers: AtomicUsize::new(0),
            caps_lock: std::cell::Cell::new(None),
            tap: AtomicUsize::new(0),
            gesture_tap: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };
        unsafe {
            capture_callback(
                std::ptr::null(),
                0xffff_fffe,
                std::ptr::null_mut(),
                (&mut context as *mut CaptureContext).cast(),
            );
        }
        assert!(!context.suppress_local.load(Ordering::Acquire));
        assert_eq!(context.system_gesture_generation.load(Ordering::Acquire), 0);
        assert_eq!(
            receiver.try_recv().unwrap(),
            CapturedInputEvent::EmergencyRelease
        );
    }

    #[test]
    fn portable_scroll_preserves_trackpad_points_and_wheel_lines() {
        unsafe {
            let trackpad = CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                ScrollEventUnit::PIXEL,
                2,
                12,
                -3,
                0,
            );
            assert!(!trackpad.is_null());
            let trackpad_scroll = portable_scroll_event(trackpad);
            assert_eq!(trackpad_scroll.unit, ScrollUnit::Pixel);
            assert_eq!(trackpad_scroll.delta_x, -3.0);
            assert_eq!(trackpad_scroll.delta_y, 12.0);
            assert_eq!(
                CGEventGetDoubleValueField(
                    trackpad,
                    EventField::SCROLL_WHEEL_EVENT_FIXED_POINT_DELTA_AXIS_1,
                ),
                1.1999969482421875
            );
            CFRelease(trackpad);

            let wheel = CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                ScrollEventUnit::LINE,
                2,
                1,
                -2,
                0,
            );
            assert!(!wheel.is_null());
            let wheel_scroll = portable_scroll_event(wheel);
            assert_eq!(wheel_scroll.unit, ScrollUnit::WheelDetent);
            assert_eq!(wheel_scroll.delta_x, -2.0);
            assert_eq!(wheel_scroll.delta_y, 1.0);
            CFRelease(wheel);
        }
    }

    #[test]
    fn flattened_scroll_event_round_trips_quartz_fields() {
        unsafe {
            let event = CGEventCreateScrollWheelEvent2(
                std::ptr::null(),
                ScrollEventUnit::PIXEL,
                2,
                12,
                -3,
                0,
            );
            assert!(!event.is_null());
            // Public Quartz fields for scroll and momentum phases.
            CGEventSetIntegerValueField(event, 99, 2);
            CGEventSetIntegerValueField(event, 123, 1);
            let payload = flatten_quartz_event(event).expect("flattened Quartz event");
            CFRelease(event);
            assert!(payload.len() <= MAX_NATIVE_QUARTZ_EVENT_SIZE);

            let data = CFDataCreate(std::ptr::null(), payload.as_ptr(), payload.len() as isize);
            assert!(!data.is_null());
            let decoded = CGEventCreateFromData(std::ptr::null(), data);
            CFRelease(data);
            assert!(!decoded.is_null());
            assert_eq!(CGEventGetType(decoded), CGEventType::ScrollWheel as u32);
            assert_eq!(CGEventGetIntegerValueField(decoded, 99), 2);
            assert_eq!(CGEventGetIntegerValueField(decoded, 123), 1);
            CFRelease(decoded);
        }
    }
}
