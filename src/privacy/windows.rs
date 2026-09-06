use super::{matches_protected_app, ForegroundApp, NativeWindow, ProtectedApp, WindowScanCache};
use std::collections::HashSet;
use windows::core::BOOL;
use windows::Win32::Devices::Display::{
    GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
};

pub fn screen_mirrored() -> bool {
    unsafe {
        let mut path_count = 0_u32;
        let mut mode_count = 0_u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            != ERROR_SUCCESS
            || path_count < 2
        {
            return false;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let mut sources = HashSet::new();
        paths[..path_count as usize].iter().any(|path| {
            let source = path.sourceInfo;
            let key = (
                source.adapterId.HighPart,
                source.adapterId.LowPart,
                source.id,
            );
            !sources.insert(key)
        })
    }
}

pub fn frontmost_application() -> Option<ForegroundApp> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 || pid == std::process::id() {
        return None;
    }
    unsafe { process_name(pid) }.map(|name| ForegroundApp {
        name,
        identifier: None,
    })
}

unsafe fn process_name(pid: u32) -> Option<String> {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
    let mut buffer = vec![0_u16; 2048];
    let mut length = buffer.len() as u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(buffer.as_mut_ptr()),
        &mut length,
    );
    let _ = windows::Win32::Foundation::CloseHandle(handle);
    result.ok()?;
    let path = String::from_utf16_lossy(&buffer[..length as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_string)
}

pub fn visible_protected_windows(
    apps: &[ProtectedApp],
    _cache: &mut WindowScanCache,
) -> Vec<NativeWindow> {
    if !apps.iter().any(|app| app.enabled) {
        return Vec::new();
    }
    struct Context<'a> {
        apps: &'a [ProtectedApp],
        windows: Vec<NativeWindow>,
    }

    unsafe extern "system" fn callback(hwnd: HWND, parameter: LPARAM) -> BOOL {
        let context = &mut *(parameter.0 as *mut Context<'_>);
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return true.into();
        }
        let mut cloaked = 0_u32;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return true.into();
        }
        let mut pid = 0_u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return true.into();
        }
        let process_name = process_name(pid).unwrap_or_default();
        if !matches_protected_app(&process_name, None, context.apps) {
            return true.into();
        }
        let mut rect = RECT::default();
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_err()
        {
            return true.into();
        }
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width < 80 || height < 60 {
            return true.into();
        }
        context.windows.push(NativeWindow {
            id: hwnd.0 as usize as u64,
            app_name: process_name,
            x: rect.left as f64,
            y: rect.top as f64,
            width: width as f64,
            height: height as f64,
        });
        true.into()
    }

    let mut context = Context {
        apps,
        windows: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut context as *mut Context<'_> as isize),
        );
    }
    context.windows
}
