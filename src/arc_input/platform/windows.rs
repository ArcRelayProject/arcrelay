use std::ffi::c_void;
use std::mem::{size_of, MaybeUninit};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arcrelay_input::{
    CaptureOptions, CapturedInputEvent, DisplayFingerprint, DisplayId, DisplayInventory,
    DisplayInventoryPort, DisplayRotation, DisplaySurface, GeometryConfidence, InputCapturePort,
    InputInjectionPort, InventoryRevision, LogicalPoint, LogicalRect, MappedKeyboardEvent,
    OsFamily, PlatformCapabilities, PlatformError, ScaleFactor, ScrollEvent, ScrollMomentumPhase,
    ScrollPhase, ScrollUnit, SizeI64, SizeU32, HID_KEY_FUNCTION,
};
use arcrelay_peer::ServiceInstanceId;
use sha2::{Digest, Sha256};
use windows::core::{s, w, Error as WindowsError, BOOL, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, EnumDisplayDevicesW, EnumDisplayMonitors, EnumDisplaySettingsW,
    GetDeviceCaps, GetMonitorInfoW, DEVMODEW, DISPLAY_DEVICEW, DMDO_180, DMDO_270, DMDO_90,
    ENUM_CURRENT_SETTINGS, HDC, HMONITOR, HORZSIZE, MONITORINFO, MONITORINFOEXW, VERTSIZE,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::{
    DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE, POINTER_FEEDBACK_MODE,
    POINTER_FEEDBACK_NONE, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_CONFIDENCE, POINTER_FLAG_INCONTACT,
    POINTER_FLAG_INRANGE, POINTER_TOUCH_INFO,
};
use windows::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, MOUSE_MOVE_ABSOLUTE,
    MOUSE_VIRTUAL_DESKTOP, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE,
    RID_INPUT, RIM_TYPEMOUSE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetMessageW, GetSystemMetrics, KillTimer, PostThreadMessageW, RegisterClassW, SetTimer,
    SetWindowsHookExW, ShowCursor, SystemParametersInfoW, TranslateMessage, UnhookWindowsHookEx,
    EDD_GET_DEVICE_INTERFACE_NAME, HC_ACTION, HWND_MESSAGE, KBDLLHOOKSTRUCT, LLKHF_INJECTED,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, PT_TOUCHPAD, SM_CXSCREEN, SM_CXVIRTUALSCREEN, SM_CYSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SPI_GETMOUSE, SPI_GETMOUSESPEED,
    SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WH_KEYBOARD_LL,
    WH_MOUSE_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN,
    WM_XBUTTONUP, WNDCLASSW, XBUTTON1, XBUTTON2,
};

use super::take_compatible_wheel_delta;

const ARC_INPUT_EVENT_TAG: usize = 0x4152_4349_4e50_5554;
const CAPTURE_QUEUE_CAPACITY: usize = 4096;
const RAW_INPUT_CLASS: PCWSTR = w!("ArcRelayInputRawPointer");
const SCROLL_GESTURE_IDLE_TIMEOUT: Duration = Duration::from_millis(80);
const SCROLL_TIMER_ID: usize = 1;
const SCROLL_TIMER_INTERVAL_MS: u32 = 10;
const SPI_GETTOUCHPADPARAMETERS: SYSTEM_PARAMETERS_INFO_ACTION =
    SYSTEM_PARAMETERS_INFO_ACTION(0x00ae);
const TOUCHPAD_PARAMETERS_VERSION_1: u32 = 1;
const TOUCHPAD_SCROLL_DIRECTION_REVERSED: u32 = 1 << 9;
const SYNTHETIC_DEVICE_OPTION_PHYSICAL_SIZE: u32 = 1;
const SYNTHETIC_DEVICE_OPTION_TOUCHPAD_GESTURE_ONLY: u32 = 2;
const SYNTHETIC_TOUCHPAD_WIDTH: i32 = 10_000;
const SYNTHETIC_TOUCHPAD_HEIGHT: i32 = 6_000;
const SYNTHETIC_TOUCHPAD_MARGIN: f64 = 500.0;
const SYNTHETIC_TOUCHPAD_UNITS_PER_PIXEL: f64 = 4.0;
const SYNTHETIC_TOUCHPAD_FRAME_MS: u32 = 8;

#[repr(C)]
struct SyntheticDeviceCreationParams {
    pointer_type: windows::Win32::UI::WindowsAndMessaging::POINTER_INPUT_TYPE,
    max_count: u32,
    feedback_mode: POINTER_FEEDBACK_MODE,
    monitor: HMONITOR,
    device_width: u32,
    device_height: u32,
    options: u32,
}

type CreateSyntheticPointerDevice2Fn = unsafe extern "system" fn(
    params: *const SyntheticDeviceCreationParams,
) -> HSYNTHETICPOINTERDEVICE;

static CREATE_SYNTHETIC_POINTER_DEVICE_2: OnceLock<Option<CreateSyntheticPointerDevice2Fn>> =
    OnceLock::new();

fn create_synthetic_pointer_device_2() -> Option<CreateSyntheticPointerDevice2Fn> {
    *CREATE_SYNTHETIC_POINTER_DEVICE_2.get_or_init(|| unsafe {
        let module = GetModuleHandleW(w!("user32.dll")).ok()?;
        let procedure = GetProcAddress(module, s!("CreateSyntheticPointerDevice2"))?;
        Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            CreateSyntheticPointerDevice2Fn,
        >(procedure))
    })
}

#[repr(C)]
struct TouchpadParametersV1 {
    version_number: u32,
    max_supported_contacts: u32,
    legacy_touchpad_features: u32,
    system_flags: u32,
    setting_flags: u32,
    sensitivity_level: u32,
    cursor_speed: u32,
    feedback_intensity: u32,
    click_force_sensitivity: u32,
    right_click_zone_width: u32,
    right_click_zone_height: u32,
}

impl Default for TouchpadParametersV1 {
    fn default() -> Self {
        Self {
            version_number: TOUCHPAD_PARAMETERS_VERSION_1,
            max_supported_contacts: 0,
            legacy_touchpad_features: 0,
            system_flags: 0,
            setting_flags: 0,
            sensitivity_level: 0,
            cursor_speed: 0,
            feedback_intensity: 0,
            click_force_sensitivity: 0,
            right_click_zone_width: 0,
            right_click_zone_height: 0,
        }
    }
}

fn target_scroll_direction_reversed() -> bool {
    let mut parameters = TouchpadParametersV1::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETTOUCHPADPARAMETERS,
            size_of::<TouchpadParametersV1>() as u32,
            Some((&mut parameters as *mut TouchpadParametersV1).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && parameters.setting_flags & TOUCHPAD_SCROLL_DIRECTION_REVERSED != 0
}

fn synthetic_touchpad_contact_delta(delta: f64, scroll_direction_reversed: bool) -> f64 {
    // ScrollEvent deltas describe the requested content scroll. With the
    // default Windows touchpad setting, content moves opposite to the contact
    // motion; the reversed setting makes it move with the contacts.
    let direction = if scroll_direction_reversed { -1.0 } else { 1.0 };
    delta * SYNTHETIC_TOUCHPAD_UNITS_PER_PIXEL * direction
}

#[derive(Default)]
struct CapturedScrollState {
    active: bool,
    last_sample: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
struct WindowsPointerSettings {
    threshold_one: i32,
    threshold_two: i32,
    acceleration: i32,
    speed: i32,
}

impl Default for WindowsPointerSettings {
    fn default() -> Self {
        Self {
            threshold_one: 6,
            threshold_two: 10,
            acceleration: 1,
            speed: 10,
        }
    }
}

impl WindowsPointerSettings {
    fn read() -> Self {
        let mut settings = Self::default();
        let mut mouse = [
            settings.threshold_one,
            settings.threshold_two,
            settings.acceleration,
        ];
        if unsafe {
            SystemParametersInfoW(
                SPI_GETMOUSE,
                0,
                Some(mouse.as_mut_ptr().cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok()
        {
            settings.threshold_one = mouse[0].max(0);
            settings.threshold_two = mouse[1].max(settings.threshold_one);
            settings.acceleration = mouse[2].clamp(0, 2);
        }
        let mut speed = settings.speed;
        if unsafe {
            SystemParametersInfoW(
                SPI_GETMOUSESPEED,
                0,
                Some((&mut speed as *mut i32).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        }
        .is_ok()
        {
            settings.speed = speed.clamp(1, 20);
        }
        settings
    }

    fn apply(self, x: i32, y: i32) -> (f64, f64) {
        let distance = i64::from(x).abs().max(i64::from(y).abs());
        let mut gain = 1.0;
        if self.acceleration > 0 && distance > i64::from(self.threshold_one) {
            gain *= 2.0;
        }
        if self.acceleration == 2 && distance > i64::from(self.threshold_two) {
            gain *= 2.0;
        }
        gain *= f64::from(self.speed) / 10.0;
        (f64::from(x) * gain, f64::from(y) * gain)
    }
}

struct CaptureContext {
    consumer: Arc<Mutex<arcrelay_input::ConsumerCapture>>,
    events: SyncSender<CapturedInputEvent>,
    options: CaptureOptions,
    suppress_local: Arc<AtomicBool>,
    cursor_hidden: Arc<Mutex<bool>>,
    last_pointer: Mutex<Option<(i32, i32)>>,
    last_raw_absolute: Mutex<Option<(i32, i32)>>,
    pointer_settings: Mutex<WindowsPointerSettings>,
    scroll: Mutex<CapturedScrollState>,
    pressed_modifiers: AtomicUsize,
    overflowed: AtomicBool,
}

impl CaptureContext {
    fn fail_open(&self) {
        lock(&self.consumer).set_route(0, 0);
        self.suppress_local.store(false, Ordering::Release);
        set_cursor_hidden(&self.cursor_hidden, false);
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
            // Never leave the low-level hooks swallowing input if their
            // consumer is stalled and cannot receive the emergency signal.
            self.fail_open();
            self.overflowed.store(true, Ordering::Release);
        }
    }
}

fn set_cursor_hidden(cursor_hidden: &Mutex<bool>, hidden: bool) {
    let mut current = lock(cursor_hidden);
    if *current == hidden {
        return;
    }
    unsafe {
        if hidden {
            while ShowCursor(false) >= 0 {}
        } else {
            while ShowCursor(true) < 0 {}
        }
    }
    *current = hidden;
}

const MODIFIER_SHIFT: usize = 1 << 0;
const MODIFIER_ALT: usize = 1 << 1;
const MODIFIER_CONTROL: usize = 1 << 2;

fn emergency_modifier_bit(hid_usage: u16) -> usize {
    match hid_usage {
        0xe1 | 0xe5 => MODIFIER_SHIFT,
        0xe2 | 0xe6 => MODIFIER_ALT,
        0xe0 | 0xe4 => MODIFIER_CONTROL,
        _ => 0,
    }
}

fn emergency_release_pressed(modifiers: usize, hid_usage: u16, down: bool) -> bool {
    down && matches!(hid_usage, 0x10 | 0x29)
        && modifiers & (MODIFIER_SHIFT | MODIFIER_ALT | MODIFIER_CONTROL)
            == MODIFIER_SHIFT | MODIFIER_ALT | MODIFIER_CONTROL
}

static ACTIVE_CAPTURE: OnceLock<Mutex<Option<Arc<CaptureContext>>>> = OnceLock::new();
static RAW_INPUT_CLASS_REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();

fn active_capture() -> &'static Mutex<Option<Arc<CaptureContext>>> {
    ACTIVE_CAPTURE.get_or_init(|| Mutex::new(None))
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let event = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    if event.flags.contains(LLKHF_INJECTED) || event.dwExtraInfo == ARC_INPUT_EVENT_TAG {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let context = lock(active_capture()).clone();
    let Some(context) = context else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    let message = wparam.0 as u32;
    let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
    let is_key = down || matches!(message, WM_KEYUP | WM_SYSKEYUP);
    if is_key && context.options.capture_keyboard {
        if let Some(key) = super::consumer::from_windows_vk(event.vkCode as u16) {
            let result = lock(&context.consumer).route(arcrelay_input::ConsumerKeyEvent {
                key,
                down,
                repeat: false,
            });
            match result {
                arcrelay_input::ConsumerCaptureResult::Forward { event, generation } => {
                    context.emit(CapturedInputEvent::ConsumerKey { event, generation });
                    return LRESULT(1);
                }
                arcrelay_input::ConsumerCaptureResult::Suppress => return LRESULT(1),
                arcrelay_input::ConsumerCaptureResult::Pass => {
                    return CallNextHookEx(None, code, wparam, lparam)
                }
            }
        }
    }
    let captured = is_key
        .then(|| windows_vk_to_hid(event.vkCode as u16))
        .flatten()
        .filter(|_| context.options.capture_keyboard)
        .map(|hid_usage| {
            let bit = emergency_modifier_bit(hid_usage);
            if bit != 0 {
                if down {
                    context.pressed_modifiers.fetch_or(bit, Ordering::AcqRel);
                } else {
                    context.pressed_modifiers.fetch_and(!bit, Ordering::AcqRel);
                }
            }
            if emergency_release_pressed(
                context.pressed_modifiers.load(Ordering::Acquire),
                hid_usage,
                down,
            ) {
                context.fail_open();
                CapturedInputEvent::EmergencyRelease
            } else {
                CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down })
            }
        });
    if let Some(captured) = captured {
        if let CapturedInputEvent::Keyboard(MappedKeyboardEvent::Physical { hid_usage, down }) =
            &captured
        {
            let result = lock(&context.consumer).physical(*hid_usage, *down);
            match result {
                Some(arcrelay_input::ConsumerCaptureResult::Forward { event, generation }) => {
                    context.emit(CapturedInputEvent::ConsumerKey { event, generation });
                    return LRESULT(1);
                }
                Some(arcrelay_input::ConsumerCaptureResult::Suppress) => return LRESULT(1),
                _ => {}
            }
        }
        context.emit(captured);
        if context.suppress_local.load(Ordering::Acquire) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code != HC_ACTION as i32 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let event = &*(lparam.0 as *const MSLLHOOKSTRUCT);
    if event.flags & LLMHF_INJECTED != 0 || event.dwExtraInfo == ARC_INPUT_EVENT_TAG {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let context = lock(active_capture()).clone();
    let Some(context) = context else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    if !context.options.capture_pointer {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let message = wparam.0 as u32;
    let captured = match message {
        WM_MOUSEMOVE if !context.suppress_local.load(Ordering::Acquire) => {
            let mut last = lock(&context.last_pointer);
            let current = (event.pt.x, event.pt.y);
            let delta = (*last)
                .map(|previous| (current.0 - previous.0, current.1 - previous.1))
                .filter(|(x, y)| *x != 0 || *y != 0)
                .map(|(x, y)| CapturedInputEvent::PointerDelta {
                    x: f64::from(x),
                    y: f64::from(y),
                });
            *last = Some(current);
            delta
        }
        WM_MOUSEMOVE => None,
        WM_LBUTTONDOWN | WM_LBUTTONUP => Some(CapturedInputEvent::PointerButton {
            hid_usage: 1,
            down: message == WM_LBUTTONDOWN,
        }),
        WM_RBUTTONDOWN | WM_RBUTTONUP => Some(CapturedInputEvent::PointerButton {
            hid_usage: 2,
            down: message == WM_RBUTTONDOWN,
        }),
        WM_MBUTTONDOWN | WM_MBUTTONUP => Some(CapturedInputEvent::PointerButton {
            hid_usage: 3,
            down: message == WM_MBUTTONDOWN,
        }),
        WM_XBUTTONDOWN | WM_XBUTTONUP => Some(CapturedInputEvent::PointerButton {
            hid_usage: if event.mouseData >> 16 == 1 { 4 } else { 5 },
            down: message == WM_XBUTTONDOWN,
        }),
        WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
            let (delta, _) = super::decode_windows_wheel(event.mouseData);
            let phase = {
                let now = Instant::now();
                let mut state = lock(&context.scroll);
                let began = !state.active
                    || state
                        .last_sample
                        .is_none_or(|last| now.duration_since(last) >= SCROLL_GESTURE_IDLE_TIMEOUT);
                state.active = true;
                state.last_sample = Some(now);
                if began {
                    ScrollPhase::Began
                } else {
                    ScrollPhase::Changed
                }
            };
            Some(CapturedInputEvent::Scroll {
                event: ScrollEvent {
                    delta_x: if message == WM_MOUSEHWHEEL {
                        delta
                    } else {
                        0.0
                    },
                    delta_y: if message == WM_MOUSEWHEEL { delta } else { 0.0 },
                    unit: ScrollUnit::WheelDetent,
                    phase,
                    momentum_phase: ScrollMomentumPhase::Unspecified,
                },
                native_quartz_event: None,
            })
        }
        _ => None,
    };
    if let Some(captured) = captured {
        context.emit(captured);
    }
    if context.suppress_local.load(Ordering::Acquire)
        && matches!(
            message,
            WM_MOUSEMOVE
                | WM_LBUTTONDOWN
                | WM_LBUTTONUP
                | WM_RBUTTONDOWN
                | WM_RBUTTONUP
                | WM_MBUTTONDOWN
                | WM_MBUTTONUP
                | WM_XBUTTONDOWN
                | WM_XBUTTONUP
                | WM_MOUSEWHEEL
                | WM_MOUSEHWHEEL
        )
    {
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn raw_input_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_INPUT {
        emit_raw_pointer(lparam);
    } else if message == WM_TIMER && wparam.0 == SCROLL_TIMER_ID {
        emit_scroll_end_if_idle();
    }
    DefWindowProcW(hwnd, message, wparam, lparam)
}

unsafe fn emit_raw_pointer(lparam: LPARAM) {
    let context = lock(active_capture()).clone();
    let Some(context) = context else {
        return;
    };
    if !context.options.capture_pointer || !context.suppress_local.load(Ordering::Acquire) {
        *lock(&context.last_raw_absolute) = None;
        return;
    }
    let mut raw = MaybeUninit::<RAWINPUT>::zeroed();
    let mut raw_size = size_of::<RAWINPUT>() as u32;
    let read = GetRawInputData(
        HRAWINPUT(lparam.0 as *mut c_void),
        RID_INPUT,
        Some(raw.as_mut_ptr().cast()),
        &mut raw_size,
        size_of::<RAWINPUTHEADER>() as u32,
    );
    if read == u32::MAX || read < size_of::<RAWINPUT>() as u32 {
        return;
    }
    let raw = raw.assume_init();
    if raw.header.dwType != RIM_TYPEMOUSE.0 {
        return;
    }
    let mouse = raw.data.mouse;
    let delta = if mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE.0 != 0 {
        let current = (mouse.lLastX, mouse.lLastY);
        let mut last = lock(&context.last_raw_absolute);
        let delta = last.map(|previous| {
            let virtual_desktop = mouse.usFlags.0 & MOUSE_VIRTUAL_DESKTOP.0 != 0;
            let width = GetSystemMetrics(if virtual_desktop {
                SM_CXVIRTUALSCREEN
            } else {
                SM_CXSCREEN
            })
            .max(1);
            let height = GetSystemMetrics(if virtual_desktop {
                SM_CYVIRTUALSCREEN
            } else {
                SM_CYSCREEN
            })
            .max(1);
            (
                f64::from(current.0 - previous.0) * f64::from(width) / 65_535.0,
                f64::from(current.1 - previous.1) * f64::from(height) / 65_535.0,
            )
        });
        *last = Some(current);
        delta
    } else {
        *lock(&context.last_raw_absolute) = None;
        Some(lock(&context.pointer_settings).apply(mouse.lLastX, mouse.lLastY))
    };
    if let Some((x, y)) = delta.filter(|(x, y)| *x != 0.0 || *y != 0.0) {
        context.emit(CapturedInputEvent::PointerDelta { x, y });
    }
}

fn emit_scroll_end_if_idle() {
    let context = lock(active_capture()).clone();
    let Some(context) = context else {
        return;
    };
    let should_end = {
        let now = Instant::now();
        let mut state = lock(&context.scroll);
        let should_end = state.active
            && state
                .last_sample
                .is_some_and(|last| now.duration_since(last) >= SCROLL_GESTURE_IDLE_TIMEOUT);
        if should_end {
            state.active = false;
            state.last_sample = None;
        }
        should_end
    };
    if should_end {
        context.emit(CapturedInputEvent::Scroll {
            event: ScrollEvent {
                unit: ScrollUnit::WheelDetent,
                phase: ScrollPhase::Ended,
                ..ScrollEvent::default()
            },
            native_quartz_event: None,
        });
    }
}

fn create_raw_input_window() -> Result<HWND, PlatformError> {
    let module = unsafe { GetModuleHandleW(None) }
        .map_err(|error| PlatformError::Operation(error.to_string()))?;
    let instance = HINSTANCE(module.0);
    RAW_INPUT_CLASS_REGISTERED
        .get_or_init(|| unsafe {
            let class = WNDCLASSW {
                lpfnWndProc: Some(raw_input_window_proc),
                hInstance: instance,
                lpszClassName: RAW_INPUT_CLASS,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                Err(windows::core::Error::from_win32().to_string())
            } else {
                Ok(())
            }
        })
        .clone()
        .map_err(PlatformError::Operation)?;
    let window = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            RAW_INPUT_CLASS,
            w!("ArcRelay Raw Pointer Input"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None::<*const c_void>,
        )
    }
    .map_err(|error| PlatformError::Operation(error.to_string()))?;
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_INPUTSINK,
        hwndTarget: window,
    };
    if let Err(error) =
        unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }
    {
        let _ = unsafe { DestroyWindow(window) };
        return Err(PlatformError::Operation(error.to_string()));
    }
    if unsafe {
        SetTimer(
            Some(window),
            SCROLL_TIMER_ID,
            SCROLL_TIMER_INTERVAL_MS,
            None,
        )
    } == 0
    {
        let _ = unsafe { DestroyWindow(window) };
        return Err(PlatformError::Operation(
            "cannot create Windows scroll gesture timer".into(),
        ));
    }
    Ok(window)
}

fn destroy_raw_input_window(window: HWND) {
    let _ = unsafe { KillTimer(Some(window), SCROLL_TIMER_ID) };
    let remove = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: RIDEV_REMOVE,
        hwndTarget: HWND::default(),
    };
    let _ = unsafe { RegisterRawInputDevices(&[remove], size_of::<RAWINPUTDEVICE>() as u32) };
    let _ = unsafe { DestroyWindow(window) };
}

struct CaptureState {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub(super) struct WindowsMonitor {
    pub(super) display_id: DisplayId,
    pub(super) native_handle: isize,
    pub(super) device_instance: String,
    fingerprint: DisplayFingerprint,
    name: String,
    rect: RECT,
    scale: f64,
    physical_size_um: SizeI64,
    rotation: DisplayRotation,
    confidence: GeometryConfidence,
    primary: bool,
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<WindowsMonitor>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if !GetMonitorInfoW(
        monitor,
        (&mut info as *mut MONITORINFOEXW).cast::<MONITORINFO>(),
    )
    .as_bool()
    {
        return BOOL(1);
    }
    let device_name = utf16_string(&info.szDevice);
    let mut device = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..DISPLAY_DEVICEW::default()
    };
    let has_device = EnumDisplayDevicesW(
        PCWSTR(info.szDevice.as_ptr()),
        0,
        &mut device,
        EDD_GET_DEVICE_INTERFACE_NAME,
    )
    .as_bool();
    let device_id = has_device
        .then(|| utf16_string(&device.DeviceID))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| device_name.clone());
    let friendly_name = has_device
        .then(|| utf16_string(&device.DeviceString))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| device_name.clone());

    let mut dpi_x = 96;
    let mut dpi_y = 96;
    let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    let scale = f64::from(dpi_x.max(dpi_y).max(1)) / 96.0;

    let dc = CreateDCW(
        PCWSTR::null(),
        PCWSTR(info.szDevice.as_ptr()),
        PCWSTR::null(),
        None,
    );
    let physical_mm = if dc.is_invalid() {
        None
    } else {
        let width = GetDeviceCaps(Some(dc), HORZSIZE);
        let height = GetDeviceCaps(Some(dc), VERTSIZE);
        let _ = DeleteDC(dc);
        (width > 0 && height > 0).then_some((width, height))
    };
    let pixel_width = (info.monitorInfo.rcMonitor.right - info.monitorInfo.rcMonitor.left).max(1);
    let pixel_height = (info.monitorInfo.rcMonitor.bottom - info.monitorInfo.rcMonitor.top).max(1);
    let (physical_size_um, confidence) = physical_mm.map_or_else(
        || {
            (
                SizeI64 {
                    width: (f64::from(pixel_width) / f64::from(dpi_x.max(1)) * 25_400.0).round()
                        as i64,
                    height: (f64::from(pixel_height) / f64::from(dpi_y.max(1)) * 25_400.0).round()
                        as i64,
                },
                GeometryConfidence::Estimated,
            )
        },
        |(width, height)| {
            (
                SizeI64 {
                    width: i64::from(width) * 1000,
                    height: i64::from(height) * 1000,
                },
                GeometryConfidence::HardwareReported,
            )
        },
    );

    let mut mode = DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..DEVMODEW::default()
    };
    let rotation = if EnumDisplaySettingsW(
        PCWSTR(info.szDevice.as_ptr()),
        ENUM_CURRENT_SETTINGS,
        &mut mode,
    )
    .as_bool()
    {
        match mode.Anonymous1.Anonymous2.dmDisplayOrientation {
            DMDO_90 => DisplayRotation::Degrees90,
            DMDO_180 => DisplayRotation::Degrees180,
            DMDO_270 => DisplayRotation::Degrees270,
            _ => DisplayRotation::Degrees0,
        }
    } else {
        DisplayRotation::Degrees0
    };

    let digest = Sha256::digest(format!("{device_id}|{pixel_width}x{pixel_height}").as_bytes());
    let suffix = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if let (Ok(display_id), Ok(fingerprint)) = (
        DisplayId::parse(format!("windows-{suffix}")),
        DisplayFingerprint::parse(format!("windows-{suffix}")),
    ) {
        monitors.push(WindowsMonitor {
            native_handle: monitor.0 as isize,
            device_instance: device_id,
            display_id,
            fingerprint,
            name: friendly_name,
            rect: info.monitorInfo.rcMonitor,
            scale,
            physical_size_um,
            rotation,
            confidence,
            primary: info.monitorInfo.dwFlags != 0,
        });
    }
    BOOL(1)
}

pub(super) fn enumerate_monitors() -> Result<Vec<WindowsMonitor>, PlatformError> {
    let mut monitors: Vec<WindowsMonitor> = Vec::new();
    let success = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM((&mut monitors as *mut Vec<WindowsMonitor>) as isize),
        )
    };
    if !success.as_bool() || monitors.is_empty() {
        return Err(PlatformError::Operation(
            "Windows display enumeration returned no monitors".into(),
        ));
    }
    monitors.sort_by(|left, right| {
        (
            left.rect.left,
            left.rect.top,
            !left.primary,
            left.display_id.as_str(),
        )
            .cmp(&(
                right.rect.left,
                right.rect.top,
                !right.primary,
                right.display_id.as_str(),
            ))
    });
    monitors.dedup_by(|right, left| {
        left.rect.left == right.rect.left
            && left.rect.top == right.rect.top
            && left.rect.right == right.rect.right
            && left.rect.bottom == right.rect.bottom
    });
    Ok(monitors)
}

fn utf16_string(value: &[u16]) -> String {
    let length = value
        .iter()
        .position(|item| *item == 0)
        .unwrap_or(value.len());
    String::from_utf16_lossy(&value[..length])
}

struct SyntheticTouchpad {
    handle: HSYNTHETICPOINTERDEVICE,
    active: bool,
    ignore_source_momentum: bool,
    first_x: f64,
    second_x: f64,
    y: f64,
    frame_time: u32,
    last_frame_at: Option<Instant>,
    scroll_direction_reversed: bool,
}

// Synthetic pointer handles are session-scoped kernel handles. Arc Input only
// accesses one while holding the platform injection mutex.
unsafe impl Send for SyntheticTouchpad {}

impl SyntheticTouchpad {
    fn create() -> Result<Self, PlatformError> {
        let create = create_synthetic_pointer_device_2().ok_or_else(|| {
            PlatformError::Unsupported(
                "Windows Precision Touchpad injection API is unavailable".into(),
            )
        })?;
        let parameters = SyntheticDeviceCreationParams {
            pointer_type: PT_TOUCHPAD,
            max_count: 2,
            feedback_mode: POINTER_FEEDBACK_NONE,
            monitor: HMONITOR::default(),
            device_width: SYNTHETIC_TOUCHPAD_WIDTH as u32,
            device_height: SYNTHETIC_TOUCHPAD_HEIGHT as u32,
            options: SYNTHETIC_DEVICE_OPTION_PHYSICAL_SIZE
                | SYNTHETIC_DEVICE_OPTION_TOUCHPAD_GESTURE_ONLY,
        };
        let handle = unsafe { create(&parameters) };
        if handle.is_invalid() {
            return Err(PlatformError::Operation(format!(
                "CreateSyntheticPointerDevice2 rejected the touchpad device: {}",
                WindowsError::from_win32()
            )));
        }
        Ok(Self {
            handle,
            active: false,
            ignore_source_momentum: false,
            first_x: 3_000.0,
            second_x: 7_000.0,
            y: 3_000.0,
            frame_time: 1,
            last_frame_at: None,
            scroll_direction_reversed: target_scroll_direction_reversed(),
        })
    }

    fn contact(&self, pointer_id: u32, x: f64, flags: u32) -> POINTER_TYPE_INFO {
        let mut touch = POINTER_TOUCH_INFO::default();
        touch.pointerInfo.pointerType = PT_TOUCHPAD;
        touch.pointerInfo.pointerId = pointer_id;
        touch.pointerInfo.pointerFlags = windows::Win32::UI::Input::Pointer::POINTER_FLAGS(flags);
        touch.pointerInfo.ptHimetricLocation = POINT {
            x: x.round() as i32,
            y: self.y.round() as i32,
        };
        touch.pointerInfo.dwTime = self.frame_time;
        POINTER_TYPE_INFO {
            r#type: PT_TOUCHPAD,
            Anonymous: POINTER_TYPE_INFO_0 { touchInfo: touch },
        }
    }

    fn inject_frame(&mut self, flags: u32) -> Result<(), PlatformError> {
        if let Some(last_frame_at) = self.last_frame_at {
            let interval = Duration::from_millis(u64::from(SYNTHETIC_TOUCHPAD_FRAME_MS));
            let elapsed = last_frame_at.elapsed();
            if elapsed < interval {
                std::thread::sleep(interval - elapsed);
            }
        }
        let contacts = [
            self.contact(1, self.first_x, flags),
            self.contact(2, self.second_x, flags),
        ];
        unsafe { InjectSyntheticPointerInput(self.handle, &contacts) }
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        self.last_frame_at = Some(Instant::now());
        Ok(())
    }

    fn begin(&mut self) -> Result<(), PlatformError> {
        if self.active {
            self.cancel()?;
        }
        self.first_x = 3_000.0;
        self.second_x = 7_000.0;
        self.y = 3_000.0;
        self.frame_time = 1;
        self.last_frame_at = None;
        self.ignore_source_momentum = false;
        self.inject_frame(
            POINTER_FLAG_CONFIDENCE.0 | POINTER_FLAG_INRANGE.0 | POINTER_FLAG_INCONTACT.0,
        )?;
        self.active = true;
        Ok(())
    }

    fn update(&mut self, event: ScrollEvent) -> Result<(), PlatformError> {
        if !event.has_delta() {
            return Ok(());
        }
        let delta_x =
            synthetic_touchpad_contact_delta(event.delta_x, self.scroll_direction_reversed);
        let delta_y =
            synthetic_touchpad_contact_delta(event.delta_y, self.scroll_direction_reversed);
        self.first_x = (self.first_x + delta_x).clamp(
            SYNTHETIC_TOUCHPAD_MARGIN,
            f64::from(SYNTHETIC_TOUCHPAD_WIDTH) - SYNTHETIC_TOUCHPAD_MARGIN,
        );
        self.second_x = (self.second_x + delta_x).clamp(
            SYNTHETIC_TOUCHPAD_MARGIN,
            f64::from(SYNTHETIC_TOUCHPAD_WIDTH) - SYNTHETIC_TOUCHPAD_MARGIN,
        );
        self.y = (self.y + delta_y).clamp(
            SYNTHETIC_TOUCHPAD_MARGIN,
            f64::from(SYNTHETIC_TOUCHPAD_HEIGHT) - SYNTHETIC_TOUCHPAD_MARGIN,
        );
        self.frame_time = self.frame_time.saturating_add(SYNTHETIC_TOUCHPAD_FRAME_MS);
        self.inject_frame(
            POINTER_FLAG_CONFIDENCE.0 | POINTER_FLAG_INRANGE.0 | POINTER_FLAG_INCONTACT.0,
        )
    }

    fn end(&mut self) -> Result<(), PlatformError> {
        if self.active {
            self.frame_time = self.frame_time.saturating_add(SYNTHETIC_TOUCHPAD_FRAME_MS);
            self.inject_frame(POINTER_FLAG_CONFIDENCE.0)?;
            self.active = false;
        }
        self.ignore_source_momentum = true;
        Ok(())
    }

    fn cancel(&mut self) -> Result<(), PlatformError> {
        if self.active {
            self.frame_time = self.frame_time.saturating_add(SYNTHETIC_TOUCHPAD_FRAME_MS);
            self.inject_frame(POINTER_FLAG_CONFIDENCE.0)?;
            self.active = false;
        }
        self.ignore_source_momentum = false;
        Ok(())
    }

    fn inject_scroll(&mut self, event: ScrollEvent) -> Result<bool, PlatformError> {
        if event.momentum_phase != ScrollMomentumPhase::Unspecified {
            if event.momentum_phase == ScrollMomentumPhase::Ended {
                self.ignore_source_momentum = false;
            }
            return Ok(self.ignore_source_momentum);
        }
        match event.phase {
            ScrollPhase::MayBegin => Ok(true),
            ScrollPhase::Began => {
                self.begin()?;
                self.update(event)?;
                Ok(true)
            }
            ScrollPhase::Changed if self.active => {
                self.update(event)?;
                Ok(true)
            }
            ScrollPhase::Ended => {
                self.update(event)?;
                self.end()?;
                Ok(true)
            }
            ScrollPhase::Cancelled => {
                self.cancel()?;
                Ok(true)
            }
            ScrollPhase::Changed | ScrollPhase::Unspecified => Ok(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollInjectionPath {
    PrecisionTouchpad,
    CompatibleWheel,
}

impl ScrollInjectionPath {
    fn as_str(self) -> &'static str {
        match self {
            Self::PrecisionTouchpad => "precision_touchpad",
            Self::CompatibleWheel => "compatible_wheel",
        }
    }
}

impl Drop for SyntheticTouchpad {
    fn drop(&mut self) {
        let _ = self.cancel();
        unsafe { DestroySyntheticPointerDevice(self.handle) };
    }
}

#[derive(Default)]
struct InjectedState {
    consumer: arcrelay_input::ConsumerSequence,
    keys: Vec<u16>,
    buttons: Vec<u16>,
    scroll_residual_x: f64,
    scroll_residual_y: f64,
    touchpad: Option<SyntheticTouchpad>,
    touchpad_creation_attempted: bool,
    scroll_path: Option<ScrollInjectionPath>,
}

impl InjectedState {
    fn try_precision_touchpad_scroll(&mut self, event: ScrollEvent) -> Result<bool, PlatformError> {
        if event.unit != ScrollUnit::Pixel
            || (event.phase == ScrollPhase::Unspecified
                && event.momentum_phase == ScrollMomentumPhase::Unspecified)
        {
            return Ok(false);
        }
        if self.touchpad.is_none() && !self.touchpad_creation_attempted {
            self.touchpad_creation_attempted = true;
            self.touchpad = Some(SyntheticTouchpad::create()?);
        }
        self.touchpad
            .as_mut()
            .map_or(Ok(false), |touchpad| touchpad.inject_scroll(event))
    }

    fn set_scroll_path(&mut self, path: ScrollInjectionPath) -> bool {
        if self.scroll_path == Some(path) {
            false
        } else {
            self.scroll_path = Some(path);
            true
        }
    }
}

pub struct NativePlatform {
    gestures:
        arcrelay_core::infrastructure::input_control::windows_system_gesture::WindowsSystemGesture,
    consumer: Arc<Mutex<arcrelay_input::ConsumerCapture>>,
    brightness: super::brightness::BrightnessWorker,
    device_id: ServiceInstanceId,
    capture: Mutex<Option<CaptureState>>,
    suppress_local: Arc<AtomicBool>,
    cursor_hidden: Arc<Mutex<bool>>,
    state: Mutex<InjectedState>,
}

impl NativePlatform {
    pub fn new(device_id: ServiceInstanceId) -> Self {
        Self {
            gestures: Default::default(),
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            brightness: super::brightness::BrightnessWorker::default(),
            device_id,
            capture: Mutex::new(None),
            suppress_local: Arc::new(AtomicBool::new(false)),
            cursor_hidden: Arc::new(Mutex::new(false)),
            state: Mutex::new(InjectedState::default()),
        }
    }

    fn send(inputs: &[INPUT]) -> Result<(), PlatformError> {
        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
        if sent == inputs.len() as u32 {
            Ok(())
        } else {
            Err(PlatformError::Operation(format!(
                "SendInput injected {sent}/{} events; elevated or secure desktops are isolated",
                inputs.len()
            )))
        }
    }

    fn key_input(vk: u16, down: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if down {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    time: 0,
                    dwExtraInfo: ARC_INPUT_EVENT_TAG,
                },
            },
        }
    }

    fn mouse_input(flags: MOUSE_EVENT_FLAGS, data: u32, dx: i32, dy: i32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: ARC_INPUT_EVENT_TAG,
                },
            },
        }
    }

    fn cursor_position() -> Result<POINT, PlatformError> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        Ok(point)
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
        PlatformCapabilities {
            can_capture_pointer: true,
            can_capture_keyboard: true,
            can_suppress_local_input: true,
            // The hook can suppress a fully remote session, but Arc Input does
            // not yet install selective native-display pointer barriers.
            can_place_internal_barrier: false,
            can_inject_absolute_pointer: true,
            can_inject_keyboard: true,
            can_inject_app_pointer: false,
            can_control_elevated_apps: false,
            can_persist_permission: true,
            can_capture_native_quartz_events: false,
            can_inject_native_quartz_events: false,
            can_capture_precision_touchpad_events: false,
            can_inject_precision_touchpad_events: create_synthetic_pointer_device_2().is_some(),
            can_capture_system_gestures: false,
            can_inject_system_gestures: self.gestures.available(),
            system_gesture_format_version: arcrelay_input::SYSTEM_GESTURE_FORMAT_VERSION,
            consumer_capture_mask: arcrelay_input::ConsumerKey::ALL_MASK,
            consumer_inject_mask: arcrelay_input::ConsumerKey::ALL_MASK,
            brightness_display_ids: self.brightness.supported(),
            limitation: Some(
                "UAC secure desktops and higher-integrity applications require the optional signed broker"
                    .into(),
            ),
        }
    }

    fn start(
        &self,
        options: CaptureOptions,
    ) -> Result<Receiver<CapturedInputEvent>, PlatformError> {
        let mut active = lock(&self.capture);
        if active.is_some() {
            return Err(PlatformError::Operation("capture is already active".into()));
        }
        if lock(active_capture()).is_some() {
            return Err(PlatformError::Operation(
                "another Arc Input capture hook is active".into(),
            ));
        }
        let (events_tx, events_rx) = mpsc::sync_channel(CAPTURE_QUEUE_CAPACITY);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        self.suppress_local
            .store(options.suppress_local, Ordering::Release);
        let suppress_local = self.suppress_local.clone();
        let cursor_hidden = self.cursor_hidden.clone();
        let consumer = self.consumer.clone();
        let initial_pointer = Self::cursor_position().ok().map(|point| (point.x, point.y));
        let thread = std::thread::Builder::new()
            .name("arc-input-windows-capture".into())
            .spawn(move || unsafe {
                let context = Arc::new(CaptureContext {
                    consumer,
                    events: events_tx,
                    options,
                    suppress_local,
                    cursor_hidden,
                    last_pointer: Mutex::new(initial_pointer),
                    last_raw_absolute: Mutex::new(None),
                    pointer_settings: Mutex::new(WindowsPointerSettings::read()),
                    scroll: Mutex::new(CapturedScrollState::default()),
                    pressed_modifiers: AtomicUsize::new(0),
                    overflowed: AtomicBool::new(false),
                });
                *lock(active_capture()) = Some(context);
                let keyboard = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0)
                {
                    Ok(hook) => hook,
                    Err(error) => {
                        *lock(active_capture()) = None;
                        let _ = started_tx.send(Err(PlatformError::Operation(format!(
                            "cannot install Windows keyboard hook: {error}"
                        ))));
                        return;
                    }
                };
                let mouse = match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0) {
                    Ok(hook) => hook,
                    Err(error) => {
                        let _ = UnhookWindowsHookEx(keyboard);
                        *lock(active_capture()) = None;
                        let _ = started_tx.send(Err(PlatformError::Operation(format!(
                            "cannot install Windows pointer hook: {error}"
                        ))));
                        return;
                    }
                };
                let raw_input_window = match create_raw_input_window() {
                    Ok(window) => window,
                    Err(error) => {
                        let _ = UnhookWindowsHookEx(mouse);
                        let _ = UnhookWindowsHookEx(keyboard);
                        *lock(active_capture()) = None;
                        let _ = started_tx.send(Err(error));
                        return;
                    }
                };
                let thread_id = GetCurrentThreadId();
                let _ = started_tx.send(Ok(thread_id));
                let mut message = MSG::default();
                while GetMessageW(&mut message, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                destroy_raw_input_window(raw_input_window);
                let _ = UnhookWindowsHookEx(mouse);
                let _ = UnhookWindowsHookEx(keyboard);
                *lock(active_capture()) = None;
            })
            .map_err(|error| PlatformError::Operation(error.to_string()))?;
        let thread_id = started_rx
            .recv()
            .map_err(|_| PlatformError::Operation("capture hook exited during startup".into()))??;
        *active = Some(CaptureState {
            thread_id,
            thread: Some(thread),
        });
        Ok(events_rx)
    }

    fn set_suppress_local(&self, suppress: bool) -> Result<(), PlatformError> {
        if !suppress {
            lock(&self.consumer).set_route(0, 0);
        }
        let point = Self::cursor_position();
        if !suppress {
            // Release first so a transient GetCursorPos failure can never leave
            // the local pointer swallowed or hidden.
            self.suppress_local.store(false, Ordering::Release);
            set_cursor_hidden(&self.cursor_hidden, false);
        }
        let point = point?;
        if let Some(context) = lock(active_capture()).as_ref() {
            *lock(&context.last_pointer) = Some((point.x, point.y));
            *lock(&context.last_raw_absolute) = None;
            if suppress {
                *lock(&context.pointer_settings) = WindowsPointerSettings::read();
            }
        }
        if suppress {
            // Keep the native cursor at the portal edge. WM_INPUT continues to
            // deliver relative hardware motion without an observable or
            // asynchronous synthetic center warp.
            set_cursor_hidden(&self.cursor_hidden, true);
            self.suppress_local.store(true, Ordering::Release);
        }
        Ok(())
    }

    fn current_pointer_position(&self) -> Result<LogicalPoint, PlatformError> {
        let point = Self::cursor_position()?;
        let belongs_to_monitor = enumerate_monitors()?.into_iter().any(|monitor| {
            point.x >= monitor.rect.left
                && point.x < monitor.rect.right
                && point.y >= monitor.rect.top
                && point.y < monitor.rect.bottom
        });
        if !belongs_to_monitor {
            return Err(PlatformError::Operation(
                "pointer is outside active displays".into(),
            ));
        }
        // Per-monitor DPI scaling does not produce one continuous logical
        // desktop on Windows. Monitor RECT, GetCursorPos and SendInput do share
        // the virtual-desktop pixel coordinate system, so expose that system
        // space to Arc Input and keep mixed-DPI display edges contiguous.
        Ok(LogicalPoint {
            x: f64::from(point.x),
            y: f64::from(point.y),
        })
    }

    fn stop(&self) -> Result<(), PlatformError> {
        lock(&self.consumer).reset();
        self.suppress_local.store(false, Ordering::Release);
        set_cursor_hidden(&self.cursor_hidden, false);
        let state = lock(&self.capture).take();
        if let Some(mut state) = state {
            let post_result =
                unsafe { PostThreadMessageW(state.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
            let join_failed = state
                .thread
                .take()
                .is_some_and(|thread| thread.join().is_err());
            post_result.map_err(|error| PlatformError::Operation(error.to_string()))?;
            if join_failed {
                return Err(PlatformError::Operation(
                    "Windows capture hook thread panicked".into(),
                ));
            }
        }
        Ok(())
    }
}

impl InputInjectionPort for NativePlatform {
    fn system_gesture(
        &self,
        event: arcrelay_input::SystemGestureEvent,
    ) -> Result<(), PlatformError> {
        self.gestures
            .apply(event)
            .map_err(|error| PlatformError::Operation(error.to_string()))
    }

    fn maintain_system_gesture(&self) -> Result<bool, PlatformError> {
        Ok(self.gestures.maintain())
    }

    fn release_consumer_keys(&self) {
        lock(&self.state).consumer.clear();
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
        let mut state = lock(&self.state);
        if state.consumer.apply(event) {
            if event.key.is_brightness() {
                self.brightness.adjust(
                    display,
                    event.key == arcrelay_input::ConsumerKey::BrightnessUp,
                )?;
            } else if let Some(vk) = super::consumer::windows_vk(event.key) {
                if let Err(error) =
                    Self::send(&[Self::key_input(vk, true), Self::key_input(vk, false)])
                {
                    let _ = Self::send(&[Self::key_input(vk, false)]);
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    fn capabilities(&self) -> PlatformCapabilities {
        InputCapturePort::capabilities(self)
    }

    fn place_pointer(
        &self,
        display_id: &DisplayId,
        point: LogicalPoint,
    ) -> Result<(), PlatformError> {
        let monitor = enumerate_monitors()?
            .into_iter()
            .find(|monitor| &monitor.display_id == display_id)
            .ok_or_else(|| PlatformError::Operation(format!("display {display_id} is offline")))?;
        let physical_x = point.x.clamp(
            f64::from(monitor.rect.left),
            f64::from(monitor.rect.right - 1),
        );
        let physical_y = point.y.clamp(
            f64::from(monitor.rect.top),
            f64::from(monitor.rect.bottom - 1),
        );
        let x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
        let y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
        let width = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(2);
        let height = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(2);
        let normalized_x = (((physical_x - f64::from(x)) * 65_535.0) / f64::from(width - 1))
            .round()
            .clamp(0.0, 65_535.0) as i32;
        let normalized_y = (((physical_y - f64::from(y)) * 65_535.0) / f64::from(height - 1))
            .round()
            .clamp(0.0, 65_535.0) as i32;
        Self::send(&[Self::mouse_input(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            0,
            normalized_x,
            normalized_y,
        )])
    }

    fn apply_keyboard(&self, event: &MappedKeyboardEvent) -> Result<(), PlatformError> {
        match event {
            MappedKeyboardEvent::Physical { hid_usage, .. } if *hid_usage == HID_KEY_FUNCTION => {
                Ok(())
            }
            MappedKeyboardEvent::Physical { hid_usage, down } => {
                let vk = hid_to_windows_vk(*hid_usage).ok_or_else(|| {
                    PlatformError::Unsupported(format!("HID key 0x{hid_usage:02x}"))
                })?;
                Self::send(&[Self::key_input(vk, *down)])?;
                let mut state = lock(&self.state);
                if *down {
                    if !state.keys.contains(hid_usage) {
                        state.keys.push(*hid_usage);
                    }
                } else {
                    state.keys.retain(|value| value != hid_usage);
                }
                Ok(())
            }
            MappedKeyboardEvent::Semantic { action, down } => {
                let chord = arcrelay_input::target_chord(*action, OsFamily::Windows);
                if *down {
                    for modifier in &chord.modifiers {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: true,
                        })?;
                    }
                }
                self.apply_keyboard(&MappedKeyboardEvent::Physical {
                    hid_usage: chord.key,
                    down: *down,
                })?;
                if !down {
                    for modifier in chord.modifiers.iter().rev() {
                        self.apply_keyboard(&MappedKeyboardEvent::Physical {
                            hid_usage: *modifier,
                            down: false,
                        })?;
                    }
                }
                Ok(())
            }
            MappedKeyboardEvent::TextCommit(text) => {
                let mut inputs = Vec::new();
                for value in text.encode_utf16() {
                    for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                        inputs.push(INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 {
                                ki: KEYBDINPUT {
                                    wVk: VIRTUAL_KEY(0),
                                    wScan: value,
                                    dwFlags: flags,
                                    time: 0,
                                    dwExtraInfo: ARC_INPUT_EVENT_TAG,
                                },
                            },
                        });
                    }
                }
                Self::send(&inputs)
            }
        }
    }

    fn pointer_button(&self, usage: u16, down: bool) -> Result<(), PlatformError> {
        let (flags, data) = match (usage, down) {
            (1, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (1, false) => (MOUSEEVENTF_LEFTUP, 0),
            (2, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (2, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (3, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (3, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (4, true) => (MOUSEEVENTF_XDOWN, XBUTTON1 as u32),
            (4, false) => (MOUSEEVENTF_XUP, XBUTTON1 as u32),
            (5, true) => (MOUSEEVENTF_XDOWN, XBUTTON2 as u32),
            (5, false) => (MOUSEEVENTF_XUP, XBUTTON2 as u32),
            _ => return Err(PlatformError::Unsupported(format!("mouse button {usage}"))),
        };
        Self::send(&[Self::mouse_input(flags, data, 0, 0)])?;
        let mut state = lock(&self.state);
        if down {
            if !state.buttons.contains(&usage) {
                state.buttons.push(usage);
            }
        } else {
            state.buttons.retain(|value| *value != usage);
        }
        Ok(())
    }

    fn scroll(&self, event: ScrollEvent) -> Result<(), PlatformError> {
        if !event.is_finite() {
            return Err(PlatformError::Operation(
                "scroll delta is not finite".into(),
            ));
        }
        let precision_result = lock(&self.state).try_precision_touchpad_scroll(event);
        match precision_result {
            Ok(true) => {
                let changed =
                    lock(&self.state).set_scroll_path(ScrollInjectionPath::PrecisionTouchpad);
                if changed {
                    tracing::info!(
                        event = "input.scroll.injection_path",
                        path = ScrollInjectionPath::PrecisionTouchpad.as_str(),
                        "Arc Input selected the Windows scroll injection path"
                    );
                }
                return Ok(());
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(
                    %error,
                    "Arc Input Precision Touchpad injection failed; using wheel fallback"
                );
                lock(&self.state).touchpad = None;
            }
        }
        let scale = match event.unit {
            ScrollUnit::Pixel => 12.0,
            ScrollUnit::WheelDetent | ScrollUnit::Unspecified => 120.0,
        };
        let flush =
            event.phase == ScrollPhase::Ended || event.momentum_phase == ScrollMomentumPhase::Ended;
        let (wheel_x, wheel_y) = {
            let mut state = lock(&self.state);
            if event.phase == ScrollPhase::Cancelled {
                state.scroll_residual_x = 0.0;
                state.scroll_residual_y = 0.0;
                return Ok(());
            }
            let total_x = state.scroll_residual_x + event.delta_x * scale;
            let total_y = state.scroll_residual_y + event.delta_y * scale;
            state.scroll_residual_x = total_x;
            state.scroll_residual_y = total_y;
            let wheel_x = take_compatible_wheel_delta(&mut state.scroll_residual_x, flush);
            let wheel_y = take_compatible_wheel_delta(&mut state.scroll_residual_y, flush);
            (wheel_x, wheel_y)
        };
        let mut inputs = Vec::new();
        if wheel_y != 0 {
            inputs.push(Self::mouse_input(MOUSEEVENTF_WHEEL, wheel_y as u32, 0, 0));
        }
        if wheel_x != 0 {
            inputs.push(Self::mouse_input(MOUSEEVENTF_HWHEEL, wheel_x as u32, 0, 0));
        }
        Self::send(&inputs)?;
        let changed = lock(&self.state).set_scroll_path(ScrollInjectionPath::CompatibleWheel);
        if changed {
            tracing::info!(
                event = "input.scroll.injection_path",
                path = ScrollInjectionPath::CompatibleWheel.as_str(),
                "Arc Input selected the Windows scroll injection path"
            );
        }
        Ok(())
    }

    fn release_all(&self) -> Result<(), PlatformError> {
        self.gestures.cancel();
        self.brightness.cancel();
        let (keys, buttons) = {
            let mut state = lock(&self.state);
            state.consumer.clear();
            if let Some(touchpad) = state.touchpad.as_mut() {
                let _ = touchpad.cancel();
            }
            state.scroll_residual_x = 0.0;
            state.scroll_residual_y = 0.0;
            (
                std::mem::take(&mut state.keys),
                std::mem::take(&mut state.buttons),
            )
        };
        for key in keys {
            let _ = Self::send(&[Self::key_input(hid_to_windows_vk(key).unwrap_or(0), false)]);
        }
        for button in buttons {
            let _ = self.pointer_button(button, false);
        }
        Ok(())
    }
}

impl DisplayInventoryPort for NativePlatform {
    fn inventory(&self) -> Result<DisplayInventory, PlatformError> {
        let revision = InventoryRevision(1);
        let displays = enumerate_monitors()?
            .into_iter()
            .map(|monitor| {
                let width = (monitor.rect.right - monitor.rect.left).max(1) as u32;
                let height = (monitor.rect.bottom - monitor.rect.top).max(1) as u32;
                DisplaySurface {
                    display_id: monitor.display_id,
                    device_id: self.device_id.clone(),
                    fingerprint: monitor.fingerprint,
                    name: monitor.name,
                    pixel_size: SizeU32 { width, height },
                    logical_bounds: LogicalRect {
                        // Windows' virtual desktop is the only common system
                        // coordinate space across monitors with different DPI.
                        x: f64::from(monitor.rect.left),
                        y: f64::from(monitor.rect.top),
                        width: f64::from(width),
                        height: f64::from(height),
                    },
                    scale_factor: ScaleFactor(monitor.scale),
                    physical_size_um: monitor.physical_size_um,
                    rotation: monitor.rotation,
                    desk_rect_um: arcrelay_input::DeskRectUm {
                        x: (f64::from(monitor.rect.left) / (monitor.scale * 96.0) * 25_400.0)
                            .round() as i64,
                        y: (f64::from(monitor.rect.top) / (monitor.scale * 96.0) * 25_400.0).round()
                            as i64,
                        width: monitor.physical_size_um.width,
                        height: monitor.physical_size_um.height,
                    },
                    geometry_confidence: monitor.confidence,
                    inventory_revision: revision,
                }
            })
            .collect();
        Ok(DisplayInventory {
            device_id: self.device_id.clone(),
            revision,
            displays,
        })
    }
}

impl Drop for NativePlatform {
    fn drop(&mut self) {
        let _ = self.stop();
        let _ = self.release_all();
    }
}

fn hid_to_windows_vk(usage: u16) -> Option<u16> {
    Some(match usage {
        0x04..=0x1d => b'A' as u16 + (usage - 0x04),
        0x1e..=0x26 => b'1' as u16 + (usage - 0x1e),
        0x27 => b'0' as u16,
        0x28 => VK_RETURN.0,
        0x29 => VK_ESCAPE.0,
        0x2a => VK_BACK.0,
        0x2b => VK_TAB.0,
        0x2c => VK_SPACE.0,
        0x2d => VK_OEM_MINUS.0,
        0x2e => VK_OEM_PLUS.0,
        0x2f => VK_OEM_4.0,
        0x30 => VK_OEM_6.0,
        0x31 => VK_OEM_5.0,
        0x33 => VK_OEM_1.0,
        0x34 => VK_OEM_7.0,
        0x35 => VK_OEM_3.0,
        0x36 => VK_OEM_COMMA.0,
        0x37 => VK_OEM_PERIOD.0,
        0x38 => VK_OEM_2.0,
        0x39 => VK_CAPITAL.0,
        0x3a..=0x45 => VK_F1.0 + (usage - 0x3a),
        0x46 => VK_SNAPSHOT.0,
        0x47 => VK_SCROLL.0,
        0x48 => VK_PAUSE.0,
        0x49 => VK_INSERT.0,
        0x4a => VK_HOME.0,
        0x4b => VK_PRIOR.0,
        0x4c => VK_DELETE.0,
        0x4d => VK_END.0,
        0x4e => VK_NEXT.0,
        0x4f => VK_RIGHT.0,
        0x50 => VK_LEFT.0,
        0x51 => VK_DOWN.0,
        0x52 => VK_UP.0,
        0x53 => VK_NUMLOCK.0,
        0x54 => VK_DIVIDE.0,
        0x55 => VK_MULTIPLY.0,
        0x56 => VK_SUBTRACT.0,
        0x57 => VK_ADD.0,
        0x58 => VK_RETURN.0,
        0x59..=0x61 => VK_NUMPAD1.0 + (usage - 0x59),
        0x62 => VK_NUMPAD0.0,
        0x63 => VK_DECIMAL.0,
        0x65 => VK_APPS.0,
        0x68..=0x73 => VK_F13.0 + (usage - 0x68),
        0xe0 => VK_LCONTROL.0,
        0xe1 => VK_LSHIFT.0,
        0xe2 => VK_LMENU.0,
        0xe3 => VK_LWIN.0,
        0xe4 => VK_RCONTROL.0,
        0xe5 => VK_RSHIFT.0,
        0xe6 => VK_RMENU.0,
        0xe7 => VK_RWIN.0,
        _ => return None,
    })
}

fn windows_vk_to_hid(vk: u16) -> Option<u16> {
    Some(match vk {
        value if (b'A' as u16..=b'Z' as u16).contains(&value) => 0x04 + value - b'A' as u16,
        value if (b'1' as u16..=b'9' as u16).contains(&value) => 0x1e + value - b'1' as u16,
        value if value == b'0' as u16 => 0x27,
        value if value == VK_RETURN.0 => 0x28,
        value if value == VK_ESCAPE.0 => 0x29,
        value if value == VK_BACK.0 => 0x2a,
        value if value == VK_TAB.0 => 0x2b,
        value if value == VK_SPACE.0 => 0x2c,
        value if value == VK_OEM_MINUS.0 => 0x2d,
        value if value == VK_OEM_PLUS.0 => 0x2e,
        value if value == VK_OEM_4.0 => 0x2f,
        value if value == VK_OEM_6.0 => 0x30,
        value if value == VK_OEM_5.0 => 0x31,
        value if value == VK_OEM_1.0 => 0x33,
        value if value == VK_OEM_7.0 => 0x34,
        value if value == VK_OEM_3.0 => 0x35,
        value if value == VK_OEM_COMMA.0 => 0x36,
        value if value == VK_OEM_PERIOD.0 => 0x37,
        value if value == VK_OEM_2.0 => 0x38,
        value if value == VK_CAPITAL.0 => 0x39,
        value if (VK_F1.0..=VK_F12.0).contains(&value) => 0x3a + value - VK_F1.0,
        value if value == VK_SNAPSHOT.0 => 0x46,
        value if value == VK_SCROLL.0 => 0x47,
        value if value == VK_PAUSE.0 => 0x48,
        value if value == VK_INSERT.0 => 0x49,
        value if value == VK_HOME.0 => 0x4a,
        value if value == VK_PRIOR.0 => 0x4b,
        value if value == VK_DELETE.0 => 0x4c,
        value if value == VK_END.0 => 0x4d,
        value if value == VK_NEXT.0 => 0x4e,
        value if value == VK_RIGHT.0 => 0x4f,
        value if value == VK_LEFT.0 => 0x50,
        value if value == VK_DOWN.0 => 0x51,
        value if value == VK_UP.0 => 0x52,
        value if value == VK_NUMLOCK.0 => 0x53,
        value if value == VK_DIVIDE.0 => 0x54,
        value if value == VK_MULTIPLY.0 => 0x55,
        value if value == VK_SUBTRACT.0 => 0x56,
        value if value == VK_ADD.0 => 0x57,
        value if (VK_NUMPAD1.0..=VK_NUMPAD9.0).contains(&value) => 0x59 + value - VK_NUMPAD1.0,
        value if value == VK_NUMPAD0.0 => 0x62,
        value if value == VK_DECIMAL.0 => 0x63,
        value if value == VK_APPS.0 => 0x65,
        value if (VK_F13.0..=VK_F24.0).contains(&value) => 0x68 + value - VK_F13.0,
        value if value == VK_LCONTROL.0 => 0xe0,
        value if value == VK_LSHIFT.0 => 0xe1,
        value if value == VK_LMENU.0 => 0xe2,
        value if value == VK_LWIN.0 => 0xe3,
        value if value == VK_RCONTROL.0 => 0xe4,
        value if value == VK_RSHIFT.0 => 0xe5,
        value if value == VK_RMENU.0 => 0xe6,
        value if value == VK_RWIN.0 => 0xe7,
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
    fn requires_control_alt_shift_and_escape_down() {
        let required = MODIFIER_CONTROL | MODIFIER_ALT | MODIFIER_SHIFT;
        assert!(emergency_release_pressed(required, 0x29, true));
        assert!(emergency_release_pressed(required, 0x10, true));
        assert!(!emergency_release_pressed(required, 0x29, false));
        assert!(!emergency_release_pressed(
            MODIFIER_CONTROL | MODIFIER_SHIFT,
            0x29,
            true
        ));
        assert!(!emergency_release_pressed(required, 0x11, true));
    }

    #[test]
    fn capture_queue_overflow_immediately_fails_open() {
        let (events, _receiver) = mpsc::sync_channel(0);
        let suppress_local = Arc::new(AtomicBool::new(true));
        let context = CaptureContext {
            consumer: Arc::new(Mutex::new(arcrelay_input::ConsumerCapture::default())),
            events,
            options: CaptureOptions {
                suppress_local: true,
                capture_pointer: true,
                capture_keyboard: true,
            },
            suppress_local: suppress_local.clone(),
            cursor_hidden: Arc::new(Mutex::new(false)),
            last_pointer: Mutex::new(None),
            last_raw_absolute: Mutex::new(None),
            pointer_settings: Mutex::new(WindowsPointerSettings::default()),
            scroll: Mutex::new(CapturedScrollState::default()),
            pressed_modifiers: AtomicUsize::new(0),
            overflowed: AtomicBool::new(false),
        };

        context.emit(CapturedInputEvent::PointerDelta { x: 1.0, y: 0.0 });

        assert!(!suppress_local.load(Ordering::Acquire));
        assert!(context.overflowed.load(Ordering::Acquire));
    }

    #[test]
    fn raw_pointer_settings_preserve_default_motion_and_apply_threshold_gain() {
        let settings = WindowsPointerSettings::default();
        assert_eq!(settings.apply(3, -2), (3.0, -2.0));
        assert_eq!(settings.apply(7, -2), (14.0, -4.0));
    }

    #[test]
    fn touchpad_contact_motion_honors_windows_scroll_direction() {
        assert_eq!(synthetic_touchpad_contact_delta(12.0, false), 48.0);
        assert_eq!(synthetic_touchpad_contact_delta(-3.0, false), -12.0);
        assert_eq!(synthetic_touchpad_contact_delta(12.0, true), -48.0);
        assert_eq!(synthetic_touchpad_contact_delta(-3.0, true), 12.0);
    }
}
