//! Experimental legacy DockSwipe fields, NOT the Universal Control protocol.
//! Field references / compatibility notes: https://github.com/joshuarli/iss
//! and Mac Mouse Fix's Helper/Core/Touch/TouchSimulator.m. See README.md.

use std::ffi::{c_void, CStr};
use std::sync::{mpsc, OnceLock};

use super::*;

type Event = *mut c_void;
type CfRef = *const c_void;
type CfMut = *mut c_void;
type Callback = unsafe extern "C" fn(CfRef, u32, Event, *mut c_void) -> Event;

const GESTURE: u32 = 29;
const DOCK_CONTROL: u32 = 30;
const DOCK_SWIPE: i64 = 23;
const BEGAN: i64 = 1;
const CHANGED: i64 = 2;
const ENDED: i64 = 4;
const CANCELLED: i64 = 8;
const MASK: u64 = (1 << GESTURE) | (1 << DOCK_CONTROL);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn CGEventCreate(source: CfRef) -> Event;
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: Callback,
        context: *mut c_void,
    ) -> CfMut;
    fn CGEventTapEnable(tap: CfMut, enabled: bool);
    fn CGEventTapIsEnabled(tap: CfMut) -> bool;
    fn CGEventGetIntegerValueField(event: Event, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: Event, field: u32) -> f64;
    fn CGEventSetIntegerValueField(event: Event, field: u32, value: i64);
    fn CGEventSetDoubleValueField(event: Event, field: u32, value: f64);
    fn CGEventPost(tap: u32, event: Event);
    fn CFMachPortCreateRunLoopSource(allocator: CfRef, tap: CfMut, order: isize) -> CfMut;
    fn CFRunLoopGetCurrent() -> CfMut;
    fn CFRunLoopAddSource(run_loop: CfMut, source: CfMut, mode: CfRef);
    fn CFRunLoopRemoveSource(run_loop: CfMut, source: CfMut, mode: CfRef);
    fn CFRunLoopRunInMode(mode: CfRef, seconds: f64, return_after_source: bool) -> i32;
    fn CFMachPortInvalidate(tap: CfMut);
    fn CFRelease(value: CfRef);
    static kCFRunLoopDefaultMode: CfRef;
}

pub(super) fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

pub(super) fn os_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let mut value = [0u8; 128];
        let mut size = value.len();
        let result = unsafe {
            libc::sysctlbyname(
                c"kern.osproductversion".as_ptr(),
                value.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if result == 0 {
            CStr::from_bytes_until_nul(&value)
                .map(|text| text.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            "unknown".into()
        }
    })
}

pub(super) fn ensure_injection_supported() -> Result<(), String> {
    if !trusted() {
        return Err(
            "enable macOS accessibility permission for the current ArcRelay application".into(),
        );
    }
    if os_version()
        .split('.')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .is_none_or(|major| major >= 27)
    {
        return Err("this experiment implements legacy DockSwipe synthesis only through macOS 26; observation may work on this version, but synthesis is not adapted".into());
    }
    Ok(())
}

#[derive(Default)]
struct Suppression {
    blocked: bool,
    companion_pending: bool,
}

impl Suppression {
    fn should_block(&mut self, event_type: u32, subtype: i64, phase: i64, enabled: bool) -> bool {
        if !enabled {
            self.blocked = false;
            self.companion_pending = false;
            return false;
        }
        if event_type == DOCK_CONTROL && subtype == DOCK_SWIPE {
            // Never intercept a gesture whose begin we did not see.
            if phase == BEGAN {
                self.blocked = true;
            }
            let block = self.blocked;
            if block {
                self.companion_pending = true;
            }
            if phase & (ENDED | CANCELLED) != 0 {
                self.blocked = false;
            }
            return block;
        }
        if event_type == GESTURE && subtype == 0 && self.companion_pending {
            self.companion_pending = false;
            return true;
        }
        false
    }
}

struct CaptureContext {
    shared: Arc<Shared>,
    suppression: Suppression,
}

unsafe fn sample(
    event: Event,
    event_type: u32,
    source: &str,
    suppressed: bool,
) -> GestureDebugSample {
    let finite = |field| {
        let value = CGEventGetDoubleValueField(event, field);
        if value.is_finite() {
            value
        } else {
            0.0
        }
    };
    GestureDebugSample {
        id: 0,
        batch_id: 0,
        elapsed_ms: 0,
        source: source.into(),
        event_type,
        subtype: CGEventGetIntegerValueField(event, 110),
        motion: CGEventGetIntegerValueField(event, 123),
        phase: CGEventGetIntegerValueField(event, 132),
        progress: finite(124),
        velocity_x: finite(129),
        velocity_y: finite(130),
        magnification: finite(113),
        rotation: finite(114),
        swipe_mask: CGEventGetIntegerValueField(event, 115),
        suppressed,
    }
}

unsafe extern "C" fn capture_callback(
    _: CfRef,
    event_type: u32,
    event: Event,
    context: *mut c_void,
) -> Event {
    if context.is_null() {
        return event;
    }
    let context = &mut *context.cast::<CaptureContext>();
    if matches!(event_type, 0xffff_fffe | 0xffff_ffff) {
        // Fail open on timeout/permission loss; require an explicit new start.
        context.shared.suppressing.store(false, Ordering::Release);
        context.shared.cancel_capture.store(true, Ordering::Release);
        return event;
    }
    if event.is_null() || !matches!(event_type, GESTURE | DOCK_CONTROL) {
        return event;
    }
    let mut record = sample(event, event_type, "tap", false);
    record.suppressed = context.suppression.should_block(
        event_type,
        record.subtype,
        record.phase,
        context.shared.suppressing.load(Ordering::Acquire),
    );
    let suppressed = record.suppressed;
    context.shared.record(record);
    if suppressed {
        std::ptr::null_mut()
    } else {
        event
    }
}

pub(super) fn start_capture(
    shared: Arc<Shared>,
    suppress: bool,
    duration: Duration,
) -> Result<JoinHandle<()>, String> {
    if !trusted() {
        return Err(
            "enable macOS accessibility permission for the current ArcRelay application".into(),
        );
    }
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let cancel = shared.clone();
    let thread = std::thread::Builder::new()
        .name("gesture-debug-tap".into())
        .spawn(move || unsafe {
            let mut context = Box::new(CaptureContext {
                shared: shared.clone(),
                suppression: Suppression::default(),
            });
            // Session tap is required for the private DockControl stream.
            let tap = CGEventTapCreate(
                1,
                0,
                if suppress { 0 } else { 1 },
                MASK,
                capture_callback,
                (&mut *context as *mut CaptureContext).cast(),
            );
            if tap.is_null() {
                let _ = started_tx.send(Err(
                    "Failed to create gesture event tap. Check Accessibility and Input Monitoring permissions, then reopen ArcRelay."
                        .to_string(),
                ));
                return;
            }
            let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
            if source.is_null() {
                CFMachPortInvalidate(tap);
                CFRelease(tap);
                let _ = started_tx.send(Err("failed to create gesture event run-loop source".to_string()));
                return;
            }
            let run_loop = CFRunLoopGetCurrent();
            CFRunLoopAddSource(run_loop, source, kCFRunLoopDefaultMode);
            CGEventTapEnable(tap, true);
            shared.capturing.store(true, Ordering::Release);
            shared.message(if suppress {
                "Interception test is active. Only complete DockSwipe sequences are intercepted, with automatic release after 10 seconds."
            } else {
                "Read-only observation is active. System gestures continue locally, with automatic stop after 120 seconds."
            });
            let _ = started_tx.send(Ok(()));
            let deadline = Instant::now() + duration;
            let mut next_permission_check = Instant::now();
            let mut lost_permission = false;
            while !shared.cancel_capture.load(Ordering::Acquire) && Instant::now() < deadline {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.02, true);
                if Instant::now() >= next_permission_check {
                    if !trusted() || !CGEventTapIsEnabled(tap) {
                        lost_permission = true;
                        break;
                    }
                    next_permission_check = Instant::now() + Duration::from_millis(500);
                }
            }
            shared.suppressing.store(false, Ordering::Release);
            CGEventTapEnable(tap, false);
            CFRunLoopRemoveSource(run_loop, source, kCFRunLoopDefaultMode);
            CFMachPortInvalidate(tap);
            CFRelease(source);
            CFRelease(tap);
            // Callback's context stays alive until the tap has been invalidated.
            drop(context);
            let reason = if lost_permission {
                "Permission was revoked or the event tap was disabled by the system. Capture stopped safely; check permissions and restart."
            } else if Instant::now() >= deadline {
                "Capture time limit was reached. Interception stopped and was released automatically; logs remain in memory."
            } else {
                "Capture stopped manually or because the system disabled the event tap."
            };
            shared.finish_capture(reason);
        })
        .map_err(|error| error.to_string())?;
    match started_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok(thread),
        result => {
            cancel.cancel_capture.store(true, Ordering::Release);
            let _ = thread.join();
            Err(match result {
                Ok(Err(error)) => error,
                _ => "gesture tap startup timed out or its thread exited".into(),
            })
        }
    }
}

struct OwnedEvent(Event);
impl OwnedEvent {
    fn new() -> Result<Self, String> {
        let event = unsafe { CGEventCreate(std::ptr::null()) };
        if event.is_null() {
            Err("failed to allocate Quartz gesture event".into())
        } else {
            Ok(Self(event))
        }
    }
}
impl Drop for OwnedEvent {
    fn drop(&mut self) {
        unsafe {
            CFRelease(self.0);
        }
    }
}

// All events are generated from bounded, whitelisted parameters. No arbitrary
// serialized native payload can be supplied by the frontend or a peer.
fn post_phase(
    shared: &Shared,
    motion: i64,
    phase: i64,
    progress: f64,
    velocity: f64,
) -> Result<(), String> {
    let (dock, companion) = make_phase(motion, phase, progress, velocity)?;
    unsafe {
        CGEventPost(1, dock.0);
        CGEventPost(1, companion.0);
        shared.record(sample(dock.0, DOCK_CONTROL, "posted", false));
    }
    Ok(())
}

// Kept separate from posting so unit tests can inspect the native event layout
// without sending any input to the user's desktop.
fn make_phase(
    motion: i64,
    phase: i64,
    progress: f64,
    velocity: f64,
) -> Result<(OwnedEvent, OwnedEvent), String> {
    let dock = OwnedEvent::new()?;
    let companion = OwnedEvent::new()?;
    unsafe {
        for (field, value) in [
            (55, DOCK_CONTROL as i64),
            (110, DOCK_SWIPE),
            (132, phase),
            (134, phase),
            (123, motion),
            (165, motion),
            (135, (progress as f32).to_bits() as i64),
            (136, 0),
        ] {
            CGEventSetIntegerValueField(dock.0, field, value);
        }
        CGEventSetDoubleValueField(dock.0, 124, progress);
        // These private fields encode the axis as float32 bits, not a delta.
        let encoded_axis = f32::from_bits(motion as u32) as f64;
        CGEventSetDoubleValueField(dock.0, 119, encoded_axis);
        CGEventSetDoubleValueField(dock.0, 139, encoded_axis);
        CGEventSetDoubleValueField(dock.0, 129, velocity);
        CGEventSetDoubleValueField(dock.0, 130, velocity);
        CGEventSetIntegerValueField(companion.0, 55, GESTURE as i64);
    }
    Ok((dock, companion))
}

fn interruptible_wait(shared: &Shared, duration: Duration) -> bool {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if shared.cancel_injection.load(Ordering::Acquire) {
            return false;
        }
        std::thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(10)),
        );
    }
    !shared.cancel_injection.load(Ordering::Acquire)
}

pub(super) fn inject(shared: &Shared, request: &GestureDebugTrigger) -> Result<(), String> {
    ensure_injection_supported()?;
    shared.message(format!(
        "Local gesture synthesis is scheduled to start in {} ms and can be stopped at any time.",
        request.delay_ms
    ));
    if !interruptible_wait(shared, Duration::from_millis(request.delay_ms.into())) {
        shared.message("Wait was cancelled; no gesture was delivered.");
        return Ok(());
    }
    ensure_injection_supported()?;
    let (motion, target) = request.motion_and_progress();
    // RAII cancellation closes any partially posted gesture on error/stop.
    struct Sequence<'a> {
        shared: &'a Shared,
        motion: i64,
        progress: f64,
        active: bool,
    }
    impl Drop for Sequence<'_> {
        fn drop(&mut self) {
            if self.active {
                let _ = post_phase(self.shared, self.motion, CANCELLED, self.progress, 0.0);
            }
        }
    }
    let mut sequence = Sequence {
        shared,
        motion,
        progress: 0.0,
        active: true,
    };
    post_phase(shared, motion, BEGAN, 0.0, 0.0)?;
    let steps = request.duration_ms.div_ceil(16);
    for step in 1..=steps {
        if !interruptible_wait(
            shared,
            Duration::from_millis(u64::from(request.duration_ms / steps)),
        ) {
            shared.message("Synthesis was aborted and native animation cancellation was requested. Confirm that the desktop returned to normal.");
            return Ok(());
        }
        if !trusted() {
            return Err("accessibility permission was lost during synthesis; gesture cancellation was attempted".into());
        }
        sequence.progress = target * f64::from(step) / f64::from(steps);
        post_phase(shared, motion, CHANGED, sequence.progress, 0.0)?;
    }
    let phase = if request.cancel_at_end {
        CANCELLED
    } else {
        ENDED
    };
    post_phase(
        shared,
        motion,
        phase,
        sequence.progress,
        if request.cancel_at_end {
            0.0
        } else {
            target / f64::from(steps) * 100.0
        },
    )?;
    sequence.active = false;
    shared.message("Event was delivered, which does not confirm system execution. Record the actual direction, animation, and whether cancellation works.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_native_phase_without_posting_or_tokio() {
        let (dock, companion) = make_phase(2, CANCELLED, -0.25, 0.0).unwrap();
        unsafe {
            let record = sample(dock.0, DOCK_CONTROL, "test", false);
            assert_eq!(CGEventGetIntegerValueField(dock.0, 55), 30);
            assert_eq!(CGEventGetIntegerValueField(companion.0, 55), 29);
            assert_eq!(record.subtype, DOCK_SWIPE);
            assert_eq!(record.motion, 2);
            assert_eq!(record.phase, CANCELLED);
            assert_eq!(record.progress, -0.25);
            assert_eq!(
                CGEventGetIntegerValueField(dock.0, 135),
                (-0.25f32).to_bits() as i64
            );
        }
    }

    #[test]
    fn suppression_only_claims_complete_dock_swipes_and_fails_open() {
        let mut gate = Suppression::default();
        assert!(!gate.should_block(DOCK_CONTROL, DOCK_SWIPE, CHANGED, true));
        assert!(gate.should_block(DOCK_CONTROL, DOCK_SWIPE, BEGAN, true));
        assert!(gate.should_block(GESTURE, 0, 0, true));
        assert!(!gate.should_block(GESTURE, 8, CHANGED, true)); // magnification
        assert!(gate.should_block(DOCK_CONTROL, DOCK_SWIPE, ENDED, true));
        assert!(gate.should_block(GESTURE, 0, 0, true));
        assert!(!gate.should_block(DOCK_CONTROL, DOCK_SWIPE, CHANGED, true));
        assert!(gate.should_block(DOCK_CONTROL, DOCK_SWIPE, BEGAN, true));
        assert!(!gate.should_block(DOCK_CONTROL, DOCK_SWIPE, CHANGED, false));
        assert!(!gate.should_block(DOCK_CONTROL, DOCK_SWIPE, ENDED, true));
    }

    #[test]
    fn cancel_before_injection_needs_no_runtime_and_posts_nothing() {
        let shared = Shared::default();
        shared.cancel_injection.store(true, Ordering::Release);
        assert!(!interruptible_wait(&shared, Duration::from_secs(1)));
        assert!(lock(&shared.data).batches.is_empty());
    }
}
