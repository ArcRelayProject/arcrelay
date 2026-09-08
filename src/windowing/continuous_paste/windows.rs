use super::HudModel;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::OnceLock;
use tauri::{AppHandle, Theme, Window};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, DeleteObject,
    DrawTextW, EndPaint, FillRect, InvalidateRect, LineTo, MoveToEx, RoundRect, SelectObject,
    SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
    FW_SEMIBOLD, HGDIOBJ, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetCursorPos, GetParent, RegisterClassW,
    SendMessageW, SetWindowPos, ShowWindow, HTCAPTION, MA_NOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER,
    SW_HIDE, SW_SHOWNOACTIVATE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_NCDESTROY, WM_NCLBUTTONDOWN, WM_PAINT, WNDCLASSW, WS_CHILD, WS_EX_NOACTIVATE, WS_VISIBLE,
};

const HUD_CLASS: PCWSTR = w!("ArcRelayContinuousPasteHudContent");

struct NativeHud {
    child: HWND,
    app: AppHandle,
    model: Option<HudModel>,
    close_pressed: bool,
}

thread_local! {
    static HUDS: RefCell<HashMap<isize, NativeHud>> = RefCell::new(HashMap::new());
}

static CLASS_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

pub fn install(app: &AppHandle, window: &Window) -> Result<(), String> {
    register_class()?;
    let parent = window.hwnd().map_err(|error| error.to_string())?;
    let module = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let child = unsafe {
        CreateWindowExW(
            WS_EX_NOACTIVATE,
            HUD_CLASS,
            w!("ArcRelay Continuous Paste"),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            320,
            92,
            Some(parent),
            None,
            Some(HINSTANCE(module.0)),
            None::<*const c_void>,
        )
    }
    .map_err(|error| error.to_string())?;
    let mut bounds = RECT::default();
    unsafe { GetClientRect(parent, &mut bounds) }.map_err(|error| error.to_string())?;
    resize_child(
        child,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
    );
    HUDS.with(|items| {
        items.borrow_mut().insert(
            parent.0 as isize,
            NativeHud {
                child,
                app: app.clone(),
                model: None,
                close_pressed: false,
            },
        );
    });
    Ok(())
}

pub fn update(window: &Window, model: &HudModel) -> Result<(), String> {
    let parent = window.hwnd().map_err(|error| error.to_string())?;
    let child = HUDS.with(|items| {
        let mut items = items.borrow_mut();
        let hud = items
            .get_mut(&(parent.0 as isize))
            .ok_or("native Windows HUD is not installed")?;
        let mut model = model.clone();
        if model.theme == crate::settings::ThemePreference::System {
            model.theme = if window.theme() == Ok(Theme::Dark) {
                crate::settings::ThemePreference::Dark
            } else {
                crate::settings::ThemePreference::Light
            };
        }
        hud.model = Some(model);
        Ok::<_, String>(hud.child)
    })?;
    let mut bounds = RECT::default();
    if unsafe { GetClientRect(parent, &mut bounds) }.is_ok() {
        resize_child(
            child,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
        );
    }
    let _ = unsafe { InvalidateRect(Some(child), None, true) };
    Ok(())
}

pub fn show(window: &Window) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    Ok(())
}

pub fn hide(window: &Window) -> Result<(), String> {
    let hwnd = window.hwnd().map_err(|error| error.to_string())?;
    let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    Ok(())
}

fn register_class() -> Result<(), String> {
    CLASS_RESULT
        .get_or_init(|| unsafe {
            let module = GetModuleHandleW(None).map_err(|error| error.to_string())?;
            let class = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: HINSTANCE(module.0),
                lpszClassName: HUD_CLASS,
                ..Default::default()
            };
            if RegisterClassW(&class) == 0 {
                Err(windows::core::Error::from_win32().to_string())
            } else {
                Ok(())
            }
        })
        .clone()
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let point = client_point(lparam);
            let parent = GetParent(hwnd).unwrap_or_default();
            let close = close_rect(hwnd);
            if contains(close, point) {
                HUDS.with(|items| {
                    if let Some(hud) = items.borrow_mut().get_mut(&(parent.0 as isize)) {
                        hud.close_pressed = true;
                    }
                });
                let _ = InvalidateRect(Some(hwnd), Some(&close), false);
            } else {
                let _ = ReleaseCapture();
                let mut cursor = POINT::default();
                let _ = GetCursorPos(&mut cursor);
                let packed = ((cursor.y as u32 & 0xffff) << 16) | (cursor.x as u32 & 0xffff);
                SendMessageW(
                    parent,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(LPARAM(packed as isize)),
                );
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let point = client_point(lparam);
            let parent = GetParent(hwnd).unwrap_or_default();
            let close = close_rect(hwnd);
            let app = HUDS.with(|items| {
                let mut items = items.borrow_mut();
                let hud = items.get_mut(&(parent.0 as isize))?;
                let clicked = hud.close_pressed && contains(close, point);
                hud.close_pressed = false;
                clicked.then(|| hud.app.clone())
            });
            let _ = InvalidateRect(Some(hwnd), Some(&close), false);
            if let Some(app) = app {
                tauri::async_runtime::spawn(async move {
                    crate::commands::clipboard_stop_continuous_paste(app).await;
                });
            }
            LRESULT(0)
        }
        WM_NCDESTROY => {
            if let Ok(parent) = GetParent(hwnd) {
                HUDS.with(|items| {
                    items.borrow_mut().remove(&(parent.0 as isize));
                });
            }
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn paint(hwnd: HWND) {
    let mut paint = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut paint);
    let parent = GetParent(hwnd).unwrap_or_default();
    let model = HUDS.with(|items| {
        items
            .borrow()
            .get(&(parent.0 as isize))
            .and_then(|hud| hud.model.clone())
    });
    let mut bounds = RECT::default();
    if GetClientRect(hwnd, &mut bounds).is_ok() {
        draw(hdc, bounds, model.as_ref(), parent);
    }
    let _ = EndPaint(hwnd, &paint);
}

unsafe fn draw(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    bounds: RECT,
    model: Option<&HudModel>,
    parent: HWND,
) {
    let width = (bounds.right - bounds.left).max(1);
    let scale = width as f64 / 320.0;
    let dark = model.is_some_and(|model| match model.theme {
        crate::settings::ThemePreference::Dark => true,
        crate::settings::ThemePreference::Light => false,
        crate::settings::ThemePreference::System => false,
    });
    let surface = if dark {
        rgb(31, 34, 41)
    } else {
        rgb(250, 250, 252)
    };
    let text = if dark {
        rgb(243, 244, 248)
    } else {
        rgb(36, 38, 45)
    };
    let secondary = if dark {
        rgb(184, 187, 199)
    } else {
        rgb(115, 118, 129)
    };
    let accent = if model.is_some_and(|value| value.complete) {
        rgb(35, 138, 88)
    } else if dark {
        rgb(124, 130, 255)
    } else {
        rgb(91, 95, 240)
    };
    fill_rect(hdc, bounds, surface);
    draw_round_rect(
        hdc,
        scaled_rect(scale, 12, 42, 50, 80),
        s(scale, 11),
        if dark {
            rgb(41, 43, 82)
        } else {
            rgb(240, 240, 255)
        },
    );
    draw_clipboard_icon(hdc, scale, accent);
    if let Some(model) = model {
        draw_text(
            hdc,
            &model.title,
            scaled_rect(scale, 60, 62, 268, 81),
            s(scale, 13),
            true,
            text,
        );
        if !model.complete {
            draw_text(
                hdc,
                &model.position,
                scaled_rect(scale, 60, 46, 268, 61),
                s(scale, 11),
                true,
                accent,
            );
        }
        draw_text(
            hdc,
            &model.preview,
            scaled_rect(scale, 60, 26, 268, 44),
            s(scale, 11),
            false,
            text,
        );
        draw_progress(
            hdc,
            scale,
            model,
            accent,
            if dark {
                rgb(68, 72, 84)
            } else {
                rgb(232, 233, 240)
            },
        );
    }
    draw_close(hdc, scale, secondary);
    let region = CreateRoundRectRgn(
        0,
        0,
        width + 1,
        bounds.bottom + 1,
        s(scale, 28),
        s(scale, 28),
    );
    if !region.0.is_null() && SetWindowRgn(parent, Some(region), true) == 0 {
        let _ = DeleteObject(HGDIOBJ(region.0));
    }
}

unsafe fn draw_progress(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scale: f64,
    model: &HudModel,
    accent: COLORREF,
    track: COLORREF,
) {
    let count = model.total.clamp(1, 8);
    let gap = 5.0;
    let segment = (208.0 - gap * (count.saturating_sub(1) as f64)) / count as f64;
    for index in 0..count {
        let left = 60.0 + index as f64 * (segment + gap);
        draw_round_rect(
            hdc,
            RECT {
                left: (left * scale).round() as i32,
                top: s(scale, 73),
                right: ((left + segment) * scale).round() as i32,
                bottom: s(scale, 77),
            },
            s(scale, 4),
            if index < model.current { accent } else { track },
        );
    }
}

unsafe fn draw_clipboard_icon(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    scale: f64,
    color: COLORREF,
) {
    let pen = CreatePen(PS_SOLID, s(scale, 2).max(1), color);
    let old = SelectObject(hdc, HGDIOBJ(pen.0));
    let x = s(scale, 23);
    let y = s(scale, 52);
    let w = s(scale, 16);
    let h = s(scale, 18);
    let _ = RoundRect(hdc, x, y, x + w, y + h, s(scale, 3), s(scale, 3));
    let _ = MoveToEx(hdc, x + s(scale, 4), y + s(scale, 5), None);
    let _ = LineTo(hdc, x + w - s(scale, 4), y + s(scale, 5));
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}

unsafe fn draw_close(hdc: windows::Win32::Graphics::Gdi::HDC, scale: f64, color: COLORREF) {
    let pen = CreatePen(PS_SOLID, s(scale, 2).max(1), color);
    let old = SelectObject(hdc, HGDIOBJ(pen.0));
    let _ = MoveToEx(hdc, s(scale, 287), s(scale, 59), None);
    let _ = LineTo(hdc, s(scale, 299), s(scale, 71));
    let _ = MoveToEx(hdc, s(scale, 299), s(scale, 59), None);
    let _ = LineTo(hdc, s(scale, 287), s(scale, 71));
    let _ = SelectObject(hdc, old);
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
    let old = SelectObject(hdc, HGDIOBJ(font.0));
    let _ = SetBkMode(hdc, TRANSPARENT);
    let _ = SetTextColor(hdc, color);
    let mut utf16 = text.encode_utf16().collect::<Vec<_>>();
    let _ = DrawTextW(
        hdc,
        &mut utf16,
        &mut rect,
        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
    );
    let _ = SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(font.0));
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
    color: COLORREF,
) {
    let brush = CreateSolidBrush(color);
    let pen = CreatePen(PS_SOLID, 1, color);
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

fn resize_child(child: HWND, width: i32, height: i32) {
    unsafe {
        SetWindowPos(
            child,
            None,
            0,
            0,
            width,
            height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        )
    }
    .ok();
}

fn close_rect(hwnd: HWND) -> RECT {
    let mut bounds = RECT::default();
    unsafe { GetClientRect(hwnd, &mut bounds) }.ok();
    let scale = (bounds.right - bounds.left).max(1) as f64 / 320.0;
    scaled_rect(scale, 276, 48, 310, 82)
}

fn client_point(value: LPARAM) -> POINT {
    POINT {
        x: value.0 as i16 as i32,
        y: (value.0 >> 16) as i16 as i32,
    }
}

fn contains(rect: RECT, point: POINT) -> bool {
    point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
}

fn scaled_rect(scale: f64, left: i32, top: i32, right: i32, bottom: i32) -> RECT {
    RECT {
        left: s(scale, left),
        top: s(scale, top),
        right: s(scale, right),
        bottom: s(scale, bottom),
    }
}

fn s(scale: f64, value: i32) -> i32 {
    (value as f64 * scale).round() as i32
}

fn rgb(red: u8, green: u8, blue: u8) -> COLORREF {
    COLORREF(u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16))
}
