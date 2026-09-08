use super::{HudModel, HUD_HEIGHT, HUD_WIDTH};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSButton, NSColor, NSFont, NSImage, NSLineBreakMode, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use tauri::{AppHandle, Theme, Window};

const ACCENT: (f64, f64, f64) = (91.0 / 255.0, 95.0 / 255.0, 240.0 / 255.0);
const COMPLETE: (f64, f64, f64) = (35.0 / 255.0, 138.0 / 255.0, 88.0 / 255.0);

struct CloseHandlerIvars {
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ArcRelayContinuousPasteCloseHandler"]
    #[thread_kind = MainThreadOnly]
    #[ivars = CloseHandlerIvars]
    struct CloseHandler;

    unsafe impl NSObjectProtocol for CloseHandler {}

    impl CloseHandler {
        #[unsafe(method(stopContinuousPaste:))]
        fn stop_continuous_paste(&self, _sender: Option<&NSObject>) {
            let app = self.ivars().app.clone();
            tauri::async_runtime::spawn(async move {
                crate::commands::clipboard_stop_continuous_paste(app).await;
            });
        }
    }
);

impl CloseHandler {
    fn new(marker: MainThreadMarker, app: AppHandle) -> Retained<Self> {
        let allocated = marker.alloc().set_ivars(CloseHandlerIvars { app });
        unsafe { msg_send![super(allocated), init] }
    }
}

struct NativeHud {
    panel: *mut NSWindow,
    _effect: Retained<NSVisualEffectView>,
    icon_background: Retained<objc2_app_kit::NSView>,
    icon: Retained<objc2_app_kit::NSImageView>,
    title: Retained<objc2_app_kit::NSTextField>,
    position: Retained<objc2_app_kit::NSTextField>,
    preview: Retained<objc2_app_kit::NSTextField>,
    close: Retained<NSButton>,
    segments: Vec<Retained<objc2_app_kit::NSView>>,
    _close_handler: Retained<CloseHandler>,
}

thread_local! {
    static HUD: std::cell::RefCell<Option<NativeHud>> = const { std::cell::RefCell::new(None) };
}

pub fn install(app: &AppHandle, window: &Window) -> Result<(), String> {
    let marker =
        MainThreadMarker::new().ok_or("native HUD must be installed on the AppKit thread")?;
    let panel = window.ns_window().map_err(|error| error.to_string())? as *mut NSWindow;
    let panel_ref = unsafe { &*panel };
    panel_ref.setStyleMask(NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel);
    panel_ref.setLevel(20);
    panel_ref.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    panel_ref.setMovable(true);
    panel_ref.setMovableByWindowBackground(true);
    panel_ref.setHidesOnDeactivate(false);
    panel_ref.setOpaque(false);
    panel_ref.setBackgroundColor(Some(&NSColor::clearColor()));

    let content = panel_ref
        .contentView()
        .ok_or("native HUD window has no content view")?;
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(14.0);
        layer.setMasksToBounds(true);
    }

    let effect =
        NSVisualEffectView::initWithFrame(marker.alloc(), rect(0.0, 0.0, HUD_WIDTH, HUD_HEIGHT));
    effect.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
    effect.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    effect.setState(NSVisualEffectState::Active);

    let icon_background =
        objc2_app_kit::NSView::initWithFrame(marker.alloc(), rect(12.0, 42.0, 38.0, 38.0));
    icon_background.setWantsLayer(true);
    if let Some(layer) = icon_background.layer() {
        layer.setCornerRadius(11.0);
    }
    let icon =
        objc2_app_kit::NSImageView::initWithFrame(marker.alloc(), rect(21.0, 51.0, 20.0, 20.0));
    if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("doc.on.clipboard"),
        None,
    ) {
        icon.setImage(Some(&image));
    }
    icon.setContentTintColor(Some(&color(ACCENT, 1.0)));

    let title = label(marker, rect(60.0, 63.0, 208.0, 18.0), 13.0, true, false);
    let position = label(marker, rect(60.0, 47.0, 208.0, 14.0), 11.0, false, false);
    let preview = label(marker, rect(60.0, 27.0, 208.0, 16.0), 11.0, false, true);
    let close_handler = CloseHandler::new(marker, app.clone());
    let close_image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("xmark"),
        None,
    )
    .ok_or("macOS xmark system image is unavailable")?;
    let close = unsafe {
        NSButton::buttonWithImage_target_action(
            &close_image,
            Some(&*close_handler),
            Some(sel!(stopContinuousPaste:)),
            marker,
        )
    };
    close.setFrame(rect(278.0, 50.0, 30.0, 30.0));
    close.setBordered(false);
    close.setContentTintColor(Some(&NSColor::secondaryLabelColor()));

    let mut segments = Vec::with_capacity(8);
    for _ in 0..8 {
        let segment = objc2_app_kit::NSView::initWithFrame(marker.alloc(), NSRect::ZERO);
        segment.setWantsLayer(true);
        if let Some(layer) = segment.layer() {
            layer.setCornerRadius(2.0);
        }
        content.addSubview(&segment);
        segments.push(segment);
    }

    content.addSubview(&effect);
    content.addSubview(&icon_background);
    content.addSubview(&icon);
    content.addSubview(&title);
    content.addSubview(&position);
    content.addSubview(&preview);
    content.addSubview(&close);
    // Keep the progress views above the visual-effect surface.
    for segment in &segments {
        content.addSubview(segment);
    }

    HUD.with(|slot| {
        *slot.borrow_mut() = Some(NativeHud {
            panel,
            _effect: effect,
            icon_background,
            icon,
            title,
            position,
            preview,
            close,
            segments,
            _close_handler: close_handler,
        });
    });
    Ok(())
}

pub fn update(window: &Window, model: &HudModel) -> Result<(), String> {
    HUD.with(|slot| {
        let binding = slot.borrow();
        let hud = binding
            .as_ref()
            .ok_or("native macOS HUD is not installed")?;
        hud.title.setStringValue(&NSString::from_str(&model.title));
        hud.position
            .setStringValue(&NSString::from_str(&model.position));
        hud.position.setHidden(model.complete);
        hud.preview
            .setStringValue(&NSString::from_str(&model.preview));
        hud.close
            .setToolTip(Some(&NSString::from_str(&model.stop_label)));
        let dark = match model.theme {
            crate::settings::ThemePreference::Dark => true,
            crate::settings::ThemePreference::Light => false,
            crate::settings::ThemePreference::System => {
                matches!(window.theme(), Ok(Theme::Dark))
            }
        };
        apply_colors(hud, dark, model.complete);
        layout_segments(hud, model.current, model.total);
        Ok::<(), String>(())
    })
}

pub fn show(window: &Window) -> Result<(), String> {
    HUD.with(|slot| {
        let binding = slot.borrow();
        let hud = binding
            .as_ref()
            .ok_or("native macOS HUD is not installed")?;
        unsafe { &*hud.panel }.orderFrontRegardless();
        Ok::<(), String>(())
    })?;
    let _ = window;
    Ok(())
}

pub fn hide(window: &Window) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

fn apply_colors(hud: &NativeHud, dark: bool, complete: bool) {
    let accent = if complete { COMPLETE } else { ACCENT };
    let soft_alpha = if dark { 0.28 } else { 0.12 };
    if let Some(layer) = hud.icon_background.layer() {
        let value = color(accent, soft_alpha).CGColor();
        layer.setBackgroundColor(Some(&value));
    }
    hud.icon.setContentTintColor(Some(&color(accent, 1.0)));
    hud.title.setTextColor(Some(&NSColor::labelColor()));
    hud.position.setTextColor(Some(&color(ACCENT, 1.0)));
    hud.preview.setTextColor(Some(&NSColor::labelColor()));
}

fn layout_segments(hud: &NativeHud, current: usize, total: usize) {
    let visible = total.clamp(1, 8);
    let gap = 5.0;
    let available = 208.0 - gap * (visible.saturating_sub(1) as f64);
    let width = available / visible as f64;
    for (index, segment) in hud.segments.iter().enumerate() {
        segment.setHidden(index >= visible);
        if index < visible {
            segment.setFrame(rect(60.0 + index as f64 * (width + gap), 15.0, width, 4.0));
            if let Some(layer) = segment.layer() {
                let fill = if index < current {
                    ACCENT
                } else {
                    (0.72, 0.73, 0.78)
                };
                let alpha = if index < current { 1.0 } else { 0.42 };
                let value = color(fill, alpha).CGColor();
                layer.setBackgroundColor(Some(&value));
            }
        }
    }
}

fn label(
    marker: MainThreadMarker,
    frame: NSRect,
    size: f64,
    bold: bool,
    monospaced: bool,
) -> Retained<objc2_app_kit::NSTextField> {
    let value = objc2_app_kit::NSTextField::labelWithString(&NSString::new(), marker);
    value.setFrame(frame);
    value.setMaximumNumberOfLines(1);
    value.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    let font = if monospaced {
        NSFont::userFixedPitchFontOfSize(size).unwrap_or_else(|| NSFont::systemFontOfSize(size))
    } else if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    value.setFont(Some(&font));
    value
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn color(rgb: (f64, f64, f64), alpha: f64) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(rgb.0, rgb.1, rgb.2, alpha)
}
