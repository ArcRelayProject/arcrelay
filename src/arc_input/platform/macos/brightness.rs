use arcrelay_input::PlatformError;
use core_graphics::display::CGDisplay;
use std::collections::BTreeMap;
use std::sync::OnceLock;

type Get = unsafe extern "C" fn(u32, *mut f32) -> i32;
type Set = unsafe extern "C" fn(u32, f32) -> i32;
fn api() -> Option<(Get, Set)> {
    static API: OnceLock<Option<(Get, Set)>> = OnceLock::new();
    *API.get_or_init(|| unsafe {
        // Capability-probed, never assume every internal/external panel works.
        let library = libc::dlopen(
            c"/System/Library/PrivateFrameworks/DisplayServices.framework/DisplayServices".as_ptr(),
            libc::RTLD_LAZY | libc::RTLD_LOCAL,
        );
        if library.is_null() {
            return None;
        }
        let get = libc::dlsym(library, c"DisplayServicesGetBrightness".as_ptr());
        let set = libc::dlsym(library, c"DisplayServicesSetBrightness".as_ptr());
        if get.is_null() || set.is_null() {
            libc::dlclose(library);
            return None;
        }
        // Keep the library loaded for the lifetime of these function pointers.
        Some((
            std::mem::transmute::<*mut libc::c_void, Get>(get),
            std::mem::transmute::<*mut libc::c_void, Set>(set),
        ))
    })
}
fn id(display: CGDisplay) -> String {
    format!(
        "mac-{:x}-{:x}-{:x}-{}x{}",
        display.vendor_number(),
        display.model_number(),
        display.serial_number(),
        display.pixels_wide(),
        display.pixels_high()
    )
}
fn displays_by_id() -> BTreeMap<String, Vec<u32>> {
    let mut result: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for display in CGDisplay::active_displays().unwrap_or_default() {
        result
            .entry(id(CGDisplay::new(display)))
            .or_default()
            .push(display);
    }
    result
}
pub(super) fn probe() -> Vec<String> {
    let Some((get, _)) = api() else {
        return vec![];
    };
    displays_by_id()
        .into_iter()
        .filter_map(|(id, displays)| {
            // Identical panels without unique serials must not resolve to the
            // first active screen. Advertise only unambiguous native targets.
            let [display] = displays.as_slice() else {
                return None;
            };
            let mut value = 0.0;
            (unsafe { get(*display, &mut value) } == 0 && (0.0..=1.0).contains(&value))
                .then_some(id)
        })
        .collect()
}
pub(super) fn adjust(target: &str, steps: i32) -> Result<(), PlatformError> {
    let (get, set) = api().ok_or_else(|| {
        PlatformError::Unsupported("DisplayServices brightness is unavailable".into())
    })?;
    let displays = displays_by_id().remove(target).unwrap_or_default();
    let [display] = displays.as_slice() else {
        return Err(PlatformError::Unsupported(
            "brightness display is offline or ambiguous".into(),
        ));
    };
    let display = *display;
    let mut current = 0.0;
    if unsafe { get(display, &mut current) } != 0 || !(0.0..=1.0).contains(&current) {
        return Err(PlatformError::Unsupported(
            "display brightness cannot be read".into(),
        ));
    }
    if unsafe { set(display, (current + steps as f32 / 16.0).clamp(0.0, 1.0)) } != 0 {
        return Err(PlatformError::Operation(
            "display brightness write failed".into(),
        ));
    }
    Ok(())
}
