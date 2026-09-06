use super::{MaskStyle, NativeWindow};
use core_graphics::display::{CGDirectDisplayID, CGDisplayRegisterReconfigurationCallback};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSFont, NSImage, NSImageScaling,
    NSImageView, NSNormalWindowLevel, NSPanel, NSTextAlignment, NSTextField, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification, NSWorkspaceDidHideApplicationNotification,
    NSWorkspaceDidLaunchApplicationNotification, NSWorkspaceDidTerminateApplicationNotification,
    NSWorkspaceDidUnhideApplicationNotification,
};
use objc2_foundation::{
    NSData, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::AppHandle;
use tokio::sync::Notify;

const LOGO_BYTES: &[u8] = include_bytes!("../../icons/icon-macos-1024.png");
const CENTER_ALIGNMENT: NSTextAlignment = NSTextAlignment(1);
static EVENT_WAKE: OnceLock<Arc<Notify>> = OnceLock::new();
static FORCE_OVERLAY_REFRESH: OnceLock<Arc<AtomicBool>> = OnceLock::new();
static DISPLAY_CALLBACK_RESULT: OnceLock<Result<(), String>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ArcRelayPrivacyWorkspaceObserver"]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct WorkspaceObserver;

    unsafe impl NSObjectProtocol for WorkspaceObserver {}

    impl WorkspaceObserver {
        #[unsafe(method(workspaceApplicationChanged:))]
        fn workspace_application_changed(&self, _notification: &NSNotification) {
            notify_refresh(true);
        }
    }
);

#[derive(Clone, Copy, PartialEq)]
struct Geometry {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

struct NativeOverlay {
    panel: Retained<NSPanel>,
    content_view: Retained<NSView>,
    effect_view: Retained<NSVisualEffectView>,
    logo_view: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    shortcut_label: Retained<NSTextField>,
    geometry: Geometry,
    style: MaskStyle,
}

thread_local! {
    static OVERLAYS: RefCell<HashMap<u64, NativeOverlay>> = RefCell::new(HashMap::new());
    static LOGO_IMAGE: RefCell<Option<Retained<NSImage>>> = const { RefCell::new(None) };
    static WORKSPACE_OBSERVER: RefCell<Option<Retained<WorkspaceObserver>>> = const { RefCell::new(None) };
}

extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> core_graphics::geometry::CGRect;
}

pub fn install_event_wake(
    app: &AppHandle,
    wake: Arc<Notify>,
    force_overlay_refresh: Arc<AtomicBool>,
) {
    let _ = EVENT_WAKE.set(wake);
    let _ = FORCE_OVERLAY_REFRESH.set(force_overlay_refresh);
    DISPLAY_CALLBACK_RESULT.get_or_init(|| {
        let result = unsafe {
            CGDisplayRegisterReconfigurationCallback(
                display_reconfigured,
                std::ptr::null::<c_void>(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(format!("display callback registration failed: {result:?}"))
        }
    });

    if let Err(error) = app.run_on_main_thread(install_workspace_observer) {
        tracing::warn!(%error, "Failed to install privacy workspace observer");
    }
}

unsafe extern "C" fn display_reconfigured(
    _display: CGDirectDisplayID,
    _flags: u32,
    _user_info: *const c_void,
) {
    notify_refresh(false);
}

fn notify_refresh(force_overlay_refresh: bool) {
    if force_overlay_refresh {
        if let Some(force_refresh) = FORCE_OVERLAY_REFRESH.get() {
            force_refresh.store(true, Ordering::SeqCst);
        }
    }
    if let Some(wake) = EVENT_WAKE.get() {
        wake.notify_one();
    }
}

fn install_workspace_observer() {
    WORKSPACE_OBSERVER.with(|storage| {
        if storage.borrow().is_some() {
            return;
        }
        let Some(marker) = MainThreadMarker::new() else {
            tracing::warn!("Privacy workspace observer must be installed on the main thread");
            return;
        };
        let observer = WorkspaceObserver::new(marker);
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        for name in [
            unsafe { NSWorkspaceDidActivateApplicationNotification },
            unsafe { NSWorkspaceDidHideApplicationNotification },
            unsafe { NSWorkspaceDidUnhideApplicationNotification },
            unsafe { NSWorkspaceDidLaunchApplicationNotification },
            unsafe { NSWorkspaceDidTerminateApplicationNotification },
        ] {
            unsafe {
                center.addObserver_selector_name_object(
                    &observer,
                    sel!(workspaceApplicationChanged:),
                    Some(name),
                    None,
                );
            }
        }
        *storage.borrow_mut() = Some(observer);
    });
}

pub fn reconcile(
    windows: &[NativeWindow],
    style: MaskStyle,
    force_reorder: bool,
) -> Result<(), String> {
    let main_height = unsafe { CGDisplayBounds(CGMainDisplayID()).size.height };
    let wanted = windows
        .iter()
        .map(|window| window.id)
        .collect::<HashSet<_>>();
    let removed = OVERLAYS.with(|storage| {
        let mut overlays = storage.borrow_mut();
        let stale = overlays
            .keys()
            .filter(|id| !wanted.contains(id))
            .copied()
            .collect::<Vec<_>>();
        stale
            .into_iter()
            .filter_map(|id| overlays.remove(&id))
            .collect::<Vec<_>>()
    });
    for overlay in removed {
        overlay.panel.close();
    }

    OVERLAYS.with(|storage| {
        let mut overlays = storage.borrow_mut();
        for target in windows {
            let geometry = Geometry {
                x: target.x,
                y: main_height - target.y - target.height,
                width: target.width,
                height: target.height,
            };
            let (overlay, created) = match overlays.entry(target.id) {
                std::collections::hash_map::Entry::Occupied(entry) => (entry.into_mut(), false),
                std::collections::hash_map::Entry::Vacant(entry) => (
                    entry.insert(
                        create_overlay(target, geometry, style)
                            .map_err(|error| format!("window {}: {error}", target.id))?,
                    ),
                    true,
                ),
            };
            if overlay.geometry != geometry {
                overlay.panel.setFrame_display(ns_rect(geometry), false);
                overlay.geometry = geometry;
                layout_overlay(overlay);
            }
            if overlay.style != style {
                apply_style(overlay, style);
                overlay.style = style;
            }
            if created || force_reorder {
                let target_window_number = target_window_number(target.id)?;
                overlay
                    .panel
                    .orderWindow_relativeTo(NSWindowOrderingMode::Above, target_window_number);
            }
        }
        Ok(())
    })
}

fn create_overlay(
    target: &NativeWindow,
    geometry: Geometry,
    style: MaskStyle,
) -> Result<NativeOverlay, String> {
    let marker = MainThreadMarker::new().ok_or("native overlay must run on main thread")?;
    let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
        marker.alloc(),
        ns_rect(geometry),
        NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
        NSBackingStoreType::Buffered,
        false,
    );
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::CanJoinAllApplications,
    );
    // Protected windows are layer-zero windows, so the mask must use the same
    // level and be ordered directly above its target. A floating-level panel or
    // orderFrontRegardless() escapes the target application's Z order and can
    // cover an unrelated foreground application when the target is behind it.
    panel.setLevel(NSNormalWindowLevel);
    panel.setFloatingPanel(false);
    panel.setBecomesKeyOnlyIfNeeded(true);
    panel.setCanHide(false);
    panel.setIgnoresMouseEvents(true);
    panel.setHidesOnDeactivate(false);
    panel.setExcludedFromWindowsMenu(true);
    panel.setMovable(false);
    panel.setHasShadow(true);
    panel.setOpaque(false);
    panel.setBackgroundColor(Some(&NSColor::clearColor()));
    unsafe { panel.setReleasedWhenClosed(false) };

    let content = panel
        .contentView()
        .ok_or("native overlay is missing its content view")?;
    content.setWantsLayer(true);
    let content_layer = content
        .layer()
        .ok_or("native overlay content view is missing its layer")?;
    content_layer.setCornerRadius(14.0);
    content_layer.setMasksToBounds(true);
    let effect_view = NSVisualEffectView::initWithFrame(
        marker.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(geometry.width, geometry.height)),
    );
    effect_view.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect_view.setState(NSVisualEffectState::Active);
    effect_view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    let logo_view = NSImageView::initWithFrame(marker.alloc(), NSRect::ZERO);
    let logo = logo_image(marker)?;
    logo_view.setImage(Some(&logo));
    logo_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);

    let title_label = NSTextField::labelWithString(&NSString::from_str("隐私内容已隐藏"), marker);
    title_label.setAlignment(CENTER_ALIGNMENT);
    title_label.setFont(Some(&NSFont::boldSystemFontOfSize(20.0)));

    let subtitle = format!("{} 已由 ArcRelay 投屏隐私模式保护", target.app_name);
    let subtitle_label = NSTextField::labelWithString(&NSString::from_str(&subtitle), marker);
    subtitle_label.setAlignment(CENTER_ALIGNMENT);
    subtitle_label.setFont(Some(&NSFont::systemFontOfSize(12.0)));

    let shortcut_label = NSTextField::labelWithString(
        &NSString::from_str("按 ⌘⇧U 解锁当前应用 · 鼠标可直接操作下层窗口"),
        marker,
    );
    shortcut_label.setAlignment(CENTER_ALIGNMENT);
    shortcut_label.setFont(Some(&NSFont::systemFontOfSize(12.0)));

    content.addSubview(&effect_view);
    content.addSubview(&logo_view);
    content.addSubview(&title_label);
    content.addSubview(&subtitle_label);
    content.addSubview(&shortcut_label);

    let overlay = NativeOverlay {
        panel,
        content_view: content,
        effect_view,
        logo_view,
        title_label,
        subtitle_label,
        shortcut_label,
        geometry,
        style,
    };
    apply_style(&overlay, style);
    layout_overlay(&overlay);
    Ok(overlay)
}

impl WorkspaceObserver {
    fn new(marker: MainThreadMarker) -> Retained<Self> {
        let this = marker.alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn logo_image(marker: MainThreadMarker) -> Result<Retained<NSImage>, String> {
    LOGO_IMAGE.with(|cached| {
        if let Some(image) = cached.borrow().as_ref() {
            return Ok(image.clone());
        }
        let data =
            unsafe { NSData::dataWithBytes_length(LOGO_BYTES.as_ptr().cast(), LOGO_BYTES.len()) };
        let image =
            NSImage::initWithData(marker.alloc(), &data).ok_or("failed to decode ArcRelay logo")?;
        *cached.borrow_mut() = Some(image.clone());
        Ok(image)
    })
}

fn apply_style(overlay: &NativeOverlay, style: MaskStyle) {
    let (background, title, subtitle) = match style {
        MaskStyle::Frosted => (
            NSColor::colorWithWhite_alpha(0.96, 0.26),
            NSColor::labelColor(),
            NSColor::secondaryLabelColor(),
        ),
        MaskStyle::Solid => (
            NSColor::blackColor(),
            NSColor::whiteColor(),
            NSColor::lightGrayColor(),
        ),
    };
    if let Some(layer) = overlay.content_view.layer() {
        let background = background.CGColor();
        layer.setBackgroundColor(Some(&background));
    }
    overlay
        .effect_view
        .setHidden(matches!(style, MaskStyle::Solid));
    overlay.title_label.setTextColor(Some(&title));
    overlay.subtitle_label.setTextColor(Some(&subtitle));
    overlay.shortcut_label.setTextColor(Some(&subtitle));
}

fn layout_overlay(overlay: &NativeOverlay) {
    let width = overlay.geometry.width;
    let height = overlay.geometry.height;
    overlay
        .effect_view
        .setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(width, height)));
    let show_controls = width >= 300.0 && height >= 240.0;
    overlay.title_label.setHidden(!show_controls);
    overlay.subtitle_label.setHidden(!show_controls);
    overlay.shortcut_label.setHidden(!show_controls);

    if show_controls {
        let center_x = width / 2.0;
        let center_y = height / 2.0;
        overlay.logo_view.setFrame(NSRect::new(
            NSPoint::new(center_x - 36.0, center_y + 34.0),
            NSSize::new(72.0, 72.0),
        ));
        overlay.title_label.setFrame(NSRect::new(
            NSPoint::new(24.0, center_y - 4.0),
            NSSize::new((width - 48.0).max(1.0), 28.0),
        ));
        overlay.subtitle_label.setFrame(NSRect::new(
            NSPoint::new(24.0, center_y - 31.0),
            NSSize::new((width - 48.0).max(1.0), 20.0),
        ));
        overlay.shortcut_label.setFrame(NSRect::new(
            NSPoint::new(24.0, center_y - 58.0),
            NSSize::new((width - 48.0).max(1.0), 20.0),
        ));
    } else {
        let size = width.min(height).mul_add(0.42, 0.0).clamp(28.0, 56.0);
        overlay.logo_view.setFrame(NSRect::new(
            NSPoint::new((width - size) / 2.0, (height - size) / 2.0),
            NSSize::new(size, size),
        ));
    }
}

fn ns_rect(geometry: Geometry) -> NSRect {
    NSRect::new(
        NSPoint::new(geometry.x, geometry.y),
        NSSize::new(geometry.width, geometry.height),
    )
}

fn target_window_number(window_id: u64) -> Result<isize, String> {
    isize::try_from(window_id)
        .map_err(|_| format!("target window id {window_id} does not fit NSInteger"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_window_number_preserves_core_graphics_window_id() {
        assert_eq!(target_window_number(u32::MAX as u64), Ok(u32::MAX as isize));
    }

    #[test]
    fn target_window_number_rejects_values_outside_ns_integer_range() {
        assert!(target_window_number((isize::MAX as u64) + 1).is_err());
    }
}
