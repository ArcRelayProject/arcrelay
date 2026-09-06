use super::{ForegroundApp, NativeWindow, ProtectedApp, WindowScanCache};

pub fn screen_mirrored() -> bool {
    false
}

pub fn frontmost_application() -> Option<ForegroundApp> {
    None
}

pub fn visible_protected_windows(
    _apps: &[ProtectedApp],
    _cache: &mut WindowScanCache,
) -> Vec<NativeWindow> {
    Vec::new()
}
