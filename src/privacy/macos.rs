use super::{matches_protected_app, ForegroundApp, NativeWindow, ProtectedApp, WindowScanCache};
use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::display::{
    kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, CGDisplayIsInMirrorSet,
    CGDisplayMirrorsDisplay,
};
use core_graphics::geometry::CGRect;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use std::collections::HashSet;
use std::ffi::c_void;

#[derive(Clone, Copy, Debug)]
struct WindowBounds {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> *const c_void;
    fn CGRectMakeWithDictionaryRepresentation(
        dictionary: CFDictionaryRef,
        rect: *mut CGRect,
    ) -> bool;
    fn CGGetOnlineDisplayList(
        maximum: u32,
        online_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
}

unsafe fn dict_get_value(dict: CFDictionaryRef, key: &str) -> Option<CFType> {
    use core_foundation::dictionary::CFDictionaryGetValue;
    use core_foundation::string::CFStringRef;
    let key = CFString::new(key);
    let raw = CFDictionaryGetValue(dict, key.as_CFTypeRef());
    (!raw.is_null()).then(|| CFType::wrap_under_get_rule(raw as CFStringRef as *const c_void))
}

fn dict_get_i64(dict: CFDictionaryRef, key: &str) -> Option<i64> {
    unsafe {
        dict_get_value(dict, key).and_then(|value| {
            let number = CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as *const _);
            number.to_i64()
        })
    }
}

fn dict_get_f64(dict: CFDictionaryRef, key: &str) -> Option<f64> {
    unsafe {
        dict_get_value(dict, key).and_then(|value| {
            let number = CFNumber::wrap_under_get_rule(value.as_CFTypeRef() as *const _);
            number.to_f64()
        })
    }
}

fn dict_get_string(dict: CFDictionaryRef, key: &str) -> Option<String> {
    unsafe {
        dict_get_value(dict, key).map(|value| {
            let string = CFString::wrap_under_get_rule(value.as_CFTypeRef() as *const _);
            string.to_string()
        })
    }
}

fn dict_get_bounds(dict: CFDictionaryRef) -> Option<WindowBounds> {
    let bounds_value = unsafe { dict_get_value(dict, "kCGWindowBounds") }?;
    let bounds_dict = bounds_value.as_CFTypeRef() as CFDictionaryRef;
    let mut rect = CGRect::new(
        &core_graphics::geometry::CGPoint::new(0.0, 0.0),
        &core_graphics::geometry::CGSize::new(0.0, 0.0),
    );
    unsafe { CGRectMakeWithDictionaryRepresentation(bounds_dict, &mut rect) }.then_some(
        WindowBounds {
            x: rect.origin.x,
            y: rect.origin.y,
            width: rect.size.width,
            height: rect.size.height,
        },
    )
}

fn same_bounds(left: WindowBounds, right: WindowBounds) -> bool {
    const TOLERANCE: f64 = 2.0;
    (left.x - right.x).abs() <= TOLERANCE
        && (left.y - right.y).abs() <= TOLERANCE
        && (left.width - right.width).abs() <= TOLERANCE
        && (left.height - right.height).abs() <= TOLERANCE
}

fn is_stage_manager_preview(bounds: WindowBounds, previews: &[WindowBounds]) -> bool {
    previews.iter().any(|preview| same_bounds(bounds, *preview))
}

fn bundle_identifier(pid: i64) -> Option<String> {
    let pid = i32::try_from(pid).ok()?;
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
        .and_then(|application| application.bundleIdentifier())
        .map(|identifier| identifier.to_string())
}

pub fn screen_mirrored() -> bool {
    let mut displays = [0_u32; 32];
    let mut count = 0_u32;
    let status =
        unsafe { CGGetOnlineDisplayList(displays.len() as u32, displays.as_mut_ptr(), &mut count) };
    if status != 0 {
        return false;
    }

    // A mirrored slave display may be omitted from CGGetActiveDisplayList,
    // leaving only the drawable master and making the old `count < 2` check
    // report a false negative. Online displays retain every member of the
    // mirror set. CGDisplayIsInMirrorSet also reports the master, while
    // CGDisplayMirrorsDisplay is kept as a fallback for older configurations.
    displays[..count as usize].iter().any(|display| unsafe {
        CGDisplayIsInMirrorSet(*display) != 0 || CGDisplayMirrorsDisplay(*display) != 0
    })
}

pub fn frontmost_application() -> Option<ForegroundApp> {
    let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    Some(ForegroundApp {
        name: application.localizedName()?.to_string(),
        identifier: application
            .bundleIdentifier()
            .map(|identifier| identifier.to_string()),
    })
}

pub fn visible_protected_windows(
    apps: &[ProtectedApp],
    cache: &mut WindowScanCache,
) -> Vec<NativeWindow> {
    if !apps.iter().any(|app| app.enabled) {
        cache.identifiers.clear();
        return Vec::new();
    }
    let needs_identifier_lookup = apps
        .iter()
        .any(|app| app.enabled && app.identifier.is_some());
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let raw = unsafe { CGWindowListCopyWindowInfo(options, 0) };
    if raw.is_null() {
        return Vec::new();
    }
    let array: CFArray = unsafe { CFArray::wrap_under_create_rule(raw as *const _) };
    // Stage Manager exposes inactive application previews as ordinary layer-zero
    // windows. WindowManager places an identically-sized gesture overlay on each
    // preview, which lets us distinguish them from the application's real window.
    // Side previews intentionally remain unmasked; when the real window returns,
    // normal protection resumes with a newly-created overlay.
    let stage_manager_previews = array
        .get_all_values()
        .into_iter()
        .filter_map(|pointer| {
            let dict = pointer as CFDictionaryRef;
            (dict_get_string(dict, "kCGWindowOwnerName").as_deref() == Some("WindowManager")
                && dict_get_string(dict, "kCGWindowName").as_deref()
                    == Some("Gesture Blocking Overlay"))
            .then(|| dict_get_bounds(dict))
            .flatten()
        })
        .collect::<Vec<_>>();
    let own_pid = std::process::id() as i64;
    let mut seen_pids = HashSet::new();
    let mut windows = Vec::new();
    for (z_order, pointer) in array.get_all_values().into_iter().enumerate() {
        let dict = pointer as CFDictionaryRef;
        let owner_pid = dict_get_i64(dict, "kCGWindowOwnerPID");
        if dict_get_i64(dict, "kCGWindowLayer") != Some(0)
            || owner_pid == Some(own_pid)
            || dict_get_f64(dict, "kCGWindowAlpha").unwrap_or(1.0) <= 0.01
        {
            continue;
        }
        if let Some(pid) = owner_pid {
            seen_pids.insert(pid);
        }
        let app_name = dict_get_string(dict, "kCGWindowOwnerName").unwrap_or_default();
        if !matches_protected_app(&app_name, None, apps) {
            if !needs_identifier_lookup {
                continue;
            }
            let owner_identifier = owner_pid.and_then(|pid| {
                cache
                    .identifiers
                    .entry(pid)
                    .or_insert_with(|| bundle_identifier(pid))
                    .as_deref()
            });
            if !matches_protected_app(&app_name, owner_identifier, apps) {
                continue;
            }
        }
        let Some(window_id) = dict_get_i64(dict, "kCGWindowNumber") else {
            continue;
        };
        let Some(bounds) = dict_get_bounds(dict) else {
            continue;
        };
        if bounds.width < 80.0 || bounds.height < 60.0 {
            continue;
        }
        if is_stage_manager_preview(bounds, &stage_manager_previews) {
            continue;
        }
        windows.push(NativeWindow {
            id: window_id as u64,
            app_name,
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
            z_order,
        });
    }
    cache
        .identifiers
        .retain(|pid, _identifier| seen_pids.contains(pid));
    windows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_manager_bounds_allow_small_rounding_differences() {
        let preview = WindowBounds {
            x: 15.0,
            y: 366.0,
            width: 115.0,
            height: 108.0,
        };
        let application = WindowBounds {
            x: 15.8,
            y: 365.5,
            width: 114.0,
            height: 109.0,
        };
        assert!(same_bounds(preview, application));
    }

    #[test]
    fn real_window_does_not_match_stage_manager_preview() {
        let preview = WindowBounds {
            x: 15.0,
            y: 366.0,
            width: 115.0,
            height: 108.0,
        };
        let application = WindowBounds {
            x: 489.0,
            y: 110.0,
            width: 1147.0,
            height: 707.0,
        };
        assert!(!same_bounds(preview, application));
    }

    #[test]
    fn stage_manager_preview_is_excluded_from_protection() {
        let preview = WindowBounds {
            x: 15.0,
            y: 366.0,
            width: 115.0,
            height: 108.0,
        };
        assert!(is_stage_manager_preview(preview, &[preview]));
    }
}
