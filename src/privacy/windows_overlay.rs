use super::{MaskStyle, NativeWindow};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::Notify;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    InvalidateRect, RoundRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, CLEARTYPE_QUALITY,
    CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HGDIOBJ,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetMessageW, GetWindow,
    RegisterClassW, SetLayeredWindowAttributes, SetWindowPos, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_HIDE, EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GW_HWNDPREV, HTTRANSPARENT, LWA_ALPHA,
    MA_NOACTIVATE, MSG, OBJID_WINDOW, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_DESTROY, WM_ERASEBKGND, WM_MOUSEACTIVATE,
    WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT, WS_POPUP,
};

const OVERSCAN: i32 = 2;
const LOGO_BYTES: &[u8] = include_bytes!("../../icons/icon-macos-1024.png");
const FROSTED_CLASS: PCWSTR = w!("ArcRelayPrivacyFrosted");
const SOLID_CLASS: PCWSTR = w!("ArcRelayPrivacySolid");

#[derive(Clone, Copy, PartialEq, Eq)]
struct Geometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

struct NativeOverlay {
    hwnd: HWND,
    geometry: Geometry,
    style: MaskStyle,
}

struct LogoPixels {
    width: i32,
    height: i32,
    premultiplied_bgra: Vec<u8>,
}

thread_local! {
    static OVERLAYS: RefCell<HashMap<u64, NativeOverlay>> = RefCell::new(HashMap::new());
}

static CLASSES: OnceLock<Result<(), String>> = OnceLock::new();
static EVENT_WAKERS: OnceLock<Mutex<Vec<EventWake>>> = OnceLock::new();
static EVENT_WATCHER: OnceLock<bool> = OnceLock::new();
static LOGO_PIXELS: OnceLock<Result<LogoPixels, String>> = OnceLock::new();
static OVERLAY_APP_NAMES: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();

struct EventWake {
    wake: Weak<Notify>,
    force_overlay_refresh: Weak<AtomicBool>,
}

pub fn install_event_wake(wake: Arc<Notify>, force_overlay_refresh: Arc<AtomicBool>) {
    EVENT_WAKERS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(EventWake {
            wake: Arc::downgrade(&wake),
            force_overlay_refresh: Arc::downgrade(&force_overlay_refresh),
        });
    let _ = EVENT_WATCHER.get_or_init(start_event_watcher);
}

fn start_event_watcher() -> bool {
    std::thread::Builder::new()
        .name("arcrelay-privacy-window-events".into())
        .spawn(|| unsafe {
            let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
            let hooks = [
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                ),
                SetWinEventHook(
                    EVENT_SYSTEM_MINIMIZESTART,
                    EVENT_SYSTEM_MINIMIZEEND,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                ),
                SetWinEventHook(
                    EVENT_OBJECT_DESTROY,
                    EVENT_OBJECT_HIDE,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                ),
                SetWinEventHook(
                    EVENT_OBJECT_LOCATIONCHANGE,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(win_event_callback),
                    0,
                    0,
                    flags,
                ),
            ];
            if hooks.iter().all(|hook| hook.0.is_null()) {
                tracing::warn!("failed to install Windows privacy window event hooks");
                return;
            }
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).as_bool() {}
        })
        .is_ok()
}

unsafe extern "system" fn win_event_callback(
    _hook: HWINEVENTHOOK,
    event: u32,
    _window: HWND,
    object_id: i32,
    child_id: i32,
    _event_thread: u32,
    _event_time: u32,
) {
    if event >= EVENT_OBJECT_DESTROY && (object_id != OBJID_WINDOW.0 || child_id != 0) {
        return;
    }
    if let Some(wakers) = EVENT_WAKERS.get() {
        let mut wakers = wakers.lock().unwrap_or_else(|error| error.into_inner());
        wakers.retain(|entry| {
            let (Some(wake), Some(force_overlay_refresh)) =
                (entry.wake.upgrade(), entry.force_overlay_refresh.upgrade())
            else {
                return false;
            };
            force_overlay_refresh.store(true, Ordering::SeqCst);
            wake.notify_one();
            true
        });
    }
}

pub fn reconcile(
    windows: &[NativeWindow],
    style: MaskStyle,
    _force_order_front: bool,
) -> Result<(), String> {
    ensure_classes()?;
    let wanted = windows
        .iter()
        .map(|window| window.id)
        .collect::<HashSet<_>>();
    OVERLAYS.with(|storage| {
        let mut overlays = storage.borrow_mut();
        overlays.retain(|id, overlay| {
            if wanted.contains(id) {
                true
            } else {
                let _ = unsafe { DestroyWindow(overlay.hwnd) };
                false
            }
        });

        for target in windows {
            let geometry = Geometry {
                x: target.x as i32 - OVERSCAN,
                y: target.y as i32 - OVERSCAN,
                width: target.width.max(1.0) as i32 + OVERSCAN * 2,
                height: target.height.max(1.0) as i32 + OVERSCAN * 2,
            };
            let needs_recreate = overlays
                .get(&target.id)
                .is_some_and(|overlay| overlay.style != style);
            if needs_recreate {
                if let Some(overlay) = overlays.remove(&target.id) {
                    let _ = unsafe { DestroyWindow(overlay.hwnd) };
                }
            }
            let overlay = match overlays.entry(target.id) {
                std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::hash_map::Entry::Vacant(entry) => entry.insert(
                    create_overlay(target, geometry, style)
                        .map_err(|error| format!("window {}: {error}", target.id))?,
                ),
            };
            position_overlay(overlay, target.id, geometry)?;
        }
        Ok(())
    })
}

fn ensure_classes() -> Result<(), String> {
    CLASSES
        .get_or_init(|| unsafe {
            let module = GetModuleHandleW(None).map_err(|error| error.to_string())?;
            let instance = HINSTANCE(module.0);
            register_class(instance, FROSTED_CLASS, frosted_window_proc)?;
            register_class(instance, SOLID_CLASS, solid_window_proc)
        })
        .clone()
}

unsafe fn register_class(
    instance: HINSTANCE,
    name: PCWSTR,
    window_proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
) -> Result<(), String> {
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: name,
        ..Default::default()
    };
    if RegisterClassW(&class) == 0 {
        Err(windows::core::Error::from_win32().to_string())
    } else {
        Ok(())
    }
}

unsafe extern "system" fn frosted_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    overlay_window_proc(hwnd, message, wparam, lparam, MaskStyle::Frosted)
}

unsafe extern "system" fn solid_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    overlay_window_proc(hwnd, message, wparam, lparam, MaskStyle::Solid)
}

unsafe fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    style: MaskStyle,
) -> LRESULT {
    match message {
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_overlay(hwnd, style);
            LRESULT(0)
        }
        WM_DESTROY => {
            overlay_app_names()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&(hwnd.0 as usize));
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn create_overlay(
    target: &NativeWindow,
    geometry: Geometry,
    style: MaskStyle,
) -> Result<NativeOverlay, String> {
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let class = match style {
        MaskStyle::Frosted => FROSTED_CLASS,
        MaskStyle::Solid => SOLID_CLASS,
    };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
            class,
            w!("ArcRelay Privacy Mask"),
            WS_POPUP,
            geometry.x,
            geometry.y,
            geometry.width,
            geometry.height,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None::<*const c_void>,
        )
    }
    .map_err(|error| error.to_string())?;
    let opacity = match style {
        MaskStyle::Frosted => 224,
        MaskStyle::Solid => 255,
    };
    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), opacity, LWA_ALPHA) }
        .map_err(|error| error.to_string())?;
    overlay_app_names()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .insert(hwnd.0 as usize, target.app_name.clone());
    Ok(NativeOverlay {
        hwnd,
        geometry,
        style,
    })
}

fn position_overlay(
    overlay: &mut NativeOverlay,
    target_window_id: u64,
    geometry: Geometry,
) -> Result<(), String> {
    let target = HWND(target_window_id as usize as *mut c_void);
    let window_above = unsafe { GetWindow(target, GW_HWNDPREV) }.ok();
    let already_above = window_above == Some(overlay.hwnd);
    let flags = if already_above {
        SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW
    } else {
        SWP_NOACTIVATE | SWP_SHOWWINDOW
    };
    unsafe {
        SetWindowPos(
            overlay.hwnd,
            if already_above { None } else { window_above },
            geometry.x,
            geometry.y,
            geometry.width,
            geometry.height,
            flags,
        )
    }
    .map_err(|error| error.to_string())?;
    let corner_diameter = 28.min(geometry.width).min(geometry.height).max(1);
    let region = unsafe {
        CreateRoundRectRgn(
            0,
            0,
            geometry.width + 1,
            geometry.height + 1,
            corner_diameter,
            corner_diameter,
        )
    };
    if !region.0.is_null() && unsafe { SetWindowRgn(overlay.hwnd, Some(region), true) } == 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(region.0)) };
    }
    let _ = unsafe { InvalidateRect(Some(overlay.hwnd), None, false) };
    overlay.geometry = geometry;
    Ok(())
}

unsafe fn paint_overlay(hwnd: HWND, style: MaskStyle) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let mut bounds = RECT::default();
    if GetClientRect(hwnd, &mut bounds).is_ok() {
        draw_overlay(hwnd, hdc, bounds, style);
    }
    let _ = EndPaint(hwnd, &paint);
}

unsafe fn draw_overlay(
    hwnd: HWND,
    hdc: windows::Win32::Graphics::Gdi::HDC,
    bounds: RECT,
    style: MaskStyle,
) {
    let width = (bounds.right - bounds.left).max(1);
    let height = (bounds.bottom - bounds.top).max(1);
    let (background, title, subtitle) = match style {
        MaskStyle::Frosted => (rgb(246, 246, 246), rgb(31, 31, 33), rgb(128, 128, 132)),
        MaskStyle::Solid => (rgb(0, 0, 0), rgb(255, 255, 255), rgb(211, 211, 214)),
    };
    fill_rect(hdc, bounds, background);

    let show_controls = width >= 300 && height >= 240;
    if show_controls {
        let center_x = width / 2;
        let center_y = height / 2;
        draw_logo(hdc, center_x - 36, center_y - 106, 72);
        draw_text(
            hdc,
            "隐私内容已隐藏",
            RECT {
                left: 24,
                top: center_y - 24,
                right: width - 24,
                bottom: center_y + 4,
            },
            20,
            true,
            title,
        );
        let protection_copy = format!("{} 已由 ArcRelay 投屏隐私模式保护", overlay_app_name(hwnd));
        draw_text(
            hdc,
            &protection_copy,
            RECT {
                left: 24,
                top: center_y + 11,
                right: width - 24,
                bottom: center_y + 31,
            },
            12,
            false,
            subtitle,
        );
        draw_text(
            hdc,
            "按 Ctrl+Shift+U 解锁当前应用 · 鼠标可直接操作下层窗口",
            RECT {
                left: 24,
                top: center_y + 38,
                right: width - 24,
                bottom: center_y + 58,
            },
            12,
            false,
            subtitle,
        );
    } else {
        let size = ((width.min(height) as f64 * 0.42) as i32).clamp(28, 56);
        draw_logo(hdc, (width - size) / 2, (height - size) / 2, size);
    }
}

fn overlay_app_names() -> &'static Mutex<HashMap<usize, String>> {
    OVERLAY_APP_NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn overlay_app_name(hwnd: HWND) -> String {
    overlay_app_names()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(hwnd.0 as usize))
        .cloned()
        .unwrap_or_else(|| "current application".to_string())
}

fn logo_pixels() -> Result<&'static LogoPixels, String> {
    LOGO_PIXELS
        .get_or_init(|| {
            let rgba = image::load_from_memory(LOGO_BYTES)
                .map_err(|error| format!("failed to decode ArcRelay logo: {error}"))?
                .to_rgba8();
            let (width, height) = rgba.dimensions();
            let mut premultiplied_bgra = Vec::with_capacity(rgba.as_raw().len());
            for pixel in rgba.as_raw().chunks_exact(4) {
                let alpha = u16::from(pixel[3]);
                premultiplied_bgra.push(((u16::from(pixel[2]) * alpha + 127) / 255) as u8);
                premultiplied_bgra.push(((u16::from(pixel[1]) * alpha + 127) / 255) as u8);
                premultiplied_bgra.push(((u16::from(pixel[0]) * alpha + 127) / 255) as u8);
                premultiplied_bgra.push(pixel[3]);
            }
            Ok(LogoPixels {
                width: width as i32,
                height: height as i32,
                premultiplied_bgra,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}

unsafe fn draw_logo(hdc: windows::Win32::Graphics::Gdi::HDC, left: i32, top: i32, size: i32) {
    if !draw_logo_bitmap(hdc, left, top, size) {
        draw_round_rect(
            hdc,
            RECT {
                left,
                top,
                right: left + size,
                bottom: top + size,
            },
            (size as f32 * 0.43) as i32,
            rgb(6, 21, 47),
            rgb(6, 21, 47),
        );
        draw_text(
            hdc,
            "A",
            RECT {
                left,
                top,
                right: left + size,
                bottom: top + size,
            },
            (size as f32 * 0.42) as i32,
            true,
            rgb(255, 255, 255),
        );
    }
}

unsafe fn draw_logo_bitmap(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    left: i32,
    top: i32,
    size: i32,
) -> bool {
    let Ok(logo) = logo_pixels() else {
        return false;
    };
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: logo.width,
            biHeight: -logo.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut::<c_void>();
    let Ok(bitmap) = CreateDIBSection(Some(hdc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return false;
    };
    if bits.is_null() {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        return false;
    }
    std::ptr::copy_nonoverlapping(
        logo.premultiplied_bgra.as_ptr(),
        bits.cast::<u8>(),
        logo.premultiplied_bgra.len(),
    );
    let memory_dc = CreateCompatibleDC(Some(hdc));
    if memory_dc.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        return false;
    }
    let old_bitmap = SelectObject(memory_dc, HGDIOBJ(bitmap.0));
    let rendered = AlphaBlend(
        hdc,
        left,
        top,
        size,
        size,
        memory_dc,
        0,
        0,
        logo.width,
        logo.height,
        BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        },
    )
    .as_bool();
    let _ = SelectObject(memory_dc, old_bitmap);
    let _ = DeleteDC(memory_dc);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    rendered
}

unsafe fn fill_rect(hdc: windows::Win32::Graphics::Gdi::HDC, rect: RECT, color: COLORREF) {
    let brush = CreateSolidBrush(color);
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn draw_round_rect(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    rect: RECT,
    radius: i32,
    fill: COLORREF,
    border: COLORREF,
) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let _ = RoundRect(
        hdc,
        rect.left,
        rect.top,
        rect.right,
        rect.bottom,
        radius,
        radius,
    );
    let _ = SelectObject(hdc, old_brush);
    let _ = SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_text(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    text: &str,
    mut rect: RECT,
    size: i32,
    semibold: bool,
    color: COLORREF,
) {
    let font = CreateFontW(
        -size,
        0,
        0,
        0,
        if semibold {
            FW_SEMIBOLD.0 as i32
        } else {
            FW_NORMAL.0 as i32
        },
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
        w!("Microsoft YaHei UI"),
    );
    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, color);
    let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
    let _ = DrawTextW(
        hdc,
        &mut utf16,
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let _ = SelectObject(hdc, old_font);
    let _ = DeleteObject(HGDIOBJ(font.0));
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
}
