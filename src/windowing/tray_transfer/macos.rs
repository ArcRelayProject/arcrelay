use super::*;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSDragOperation, NSDraggingDestination, NSDraggingInfo, NSEvent,
    NSPasteboardTypeFileURL, NSView,
};
use objc2_foundation::{NSArray, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString, NSURL};

mod panel_type {
    use tauri::Manager;
    use tauri_nspanel::tauri_panel;
    tauri_panel! {
        panel!(TrayTransferPanel {
            config: { can_become_key_window: true, can_become_main_window: false }
        })
    }
}
use panel_type::TrayTransferPanel;

struct DropViewIvars {
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArcRelayTrayDropView"]
    #[ivars = DropViewIvars]
    struct DropView;

    unsafe impl NSObjectProtocol for DropView {}

    unsafe impl NSDraggingDestination for DropView {
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if accepts(sender) {
                enter(&self.ivars().app);
                NSDragOperation::Copy
            } else { NSDragOperation::None }
        }
        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            if accepts(sender) { NSDragOperation::Copy } else { NSDragOperation::None }
        }
        #[unsafe(method(draggingExited:))]
        fn dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) { leave(&self.ivars().app); }
        #[unsafe(method(draggingEnded:))]
        fn dragging_ended(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) { leave(&self.ivars().app); }
        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_drop(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool { accepts(sender) }
        #[unsafe(method(performDragOperation:))]
        fn perform_drop(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            let paths = file_paths(sender);
            if accepts(sender) && !paths.is_empty() {
                receive(&self.ivars().app, paths);
                true
            } else { false }
        }
    }

    impl DropView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool { true }
        // Preserve tray-icon's target/action and right-click menu handling.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) { if let Some(button) = unsafe { self.superview() } { button.mouseDown(event); } }
        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) { if let Some(button) = unsafe { self.superview() } { button.mouseUp(event); } }
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) { if let Some(button) = unsafe { self.superview() } { button.rightMouseDown(event); } }
        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) { if let Some(button) = unsafe { self.superview() } { button.rightMouseUp(event); } }
    }
);

fn accepts(sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
    sender
        .draggingSourceOperationMask()
        .contains(NSDragOperation::Copy)
        && !file_paths(sender).is_empty()
}

fn file_paths(sender: &ProtocolObject<dyn NSDraggingInfo>) -> Vec<String> {
    let Some(items) = sender.draggingPasteboard().pasteboardItems() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let value = item.stringForType(unsafe { NSPasteboardTypeFileURL })?;
            file_url_path(&value)
        })
        .collect()
}

fn file_url_path(value: &NSString) -> Option<String> {
    let url = NSURL::URLWithString(value)?;
    url.isFileURL()
        .then(|| url.path())
        .flatten()
        .map(|path| path.to_string())
}

pub(super) fn install(app: &AppHandle) -> tauri::Result<()> {
    let handle = app.clone();
    let Some(tray) = app.tray_by_id("main-tray") else {
        return Ok(());
    };
    tray.with_inner_tray_icon(move |tray| {
        let mtm = MainThreadMarker::new().expect("tray hook runs on AppKit thread");
        let Some(item) = tray.ns_status_item() else {
            return;
        };
        let Some(button) = item.button(mtm) else {
            return;
        };
        let allocated = DropView::alloc(mtm).set_ivars(DropViewIvars { app: handle });
        let view: Retained<DropView> =
            unsafe { msg_send![super(allocated), initWithFrame: button.bounds()] };
        view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        view.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        button.addSubview(&view); // The button retains its drop target for its lifetime.
    })
}

pub(super) fn configure_panel(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let panel = window.to_panel::<TrayTransferPanel>()?;
    panel.set_level(20);
    panel.set_floating_panel(true);
    panel.set_has_shadow(true);
    panel.set_opaque(false);
    panel.set_transparent(true);
    panel.set_corner_radius(14.0);
    panel.set_hides_on_deactivate(false);
    panel.set_released_when_closed(false);
    panel.set_becomes_key_only_if_needed(false);
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().value());
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .full_screen_auxiliary()
            .value(),
    );
    let content = panel.content_view();
    content.setWantsLayer(true);
    if let Some(layer) = content.layer() {
        layer.setCornerRadius(14.0);
        layer.setMasksToBounds(true);
    }
    panel.hide();
    Ok(())
}

pub(super) fn show(app: &AppHandle, focus: bool) -> tauri::Result<()> {
    if let Ok(panel) = app.get_webview_panel(LABEL) {
        if focus {
            panel.show_and_make_key();
        } else {
            panel.show();
        }
    }
    Ok(())
}

pub(super) fn hide(app: &AppHandle) -> tauri::Result<()> {
    if let Ok(panel) = app.get_webview_panel(LABEL) {
        panel.hide();
    }
    Ok(())
}

pub(super) fn position(app: &AppHandle) -> tauri::Result<()> {
    let height = {
        let runtime = app.state::<Runtime>();
        let state = runtime.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.height < 170.0 {
            220.0
        } else {
            state.height
        }
    };
    let handle = app.clone();
    if let Some(tray) = app.tray_by_id("main-tray") {
        tray.with_inner_tray_icon(move |tray| {
            let mtm = MainThreadMarker::new().expect("panel positioning runs on AppKit thread");
            let Some(item) = tray.ns_status_item() else {
                return;
            };
            let Some(button) = item.button(mtm) else {
                return;
            };
            let Some(tray_window) = button.window() else {
                return;
            };
            let Some(screen) = tray_window.screen() else {
                return;
            };
            let Ok(panel) = handle.get_webview_panel(LABEL) else {
                return;
            };
            let Some(window) = panel.content_view().window() else {
                return;
            };
            // Use AppKit points end-to-end: no mixed-DPI or top/bottom coordinate conversion.
            let anchor = tray_window.frame();
            let visible = screen.visibleFrame();
            let width = WIDTH.min(visible.size.width - 16.0);
            let height = height.min(visible.size.height - 16.0);
            let x = (anchor.origin.x + anchor.size.width / 2.0 - width / 2.0).clamp(
                visible.origin.x + 8.0,
                visible.origin.x + visible.size.width - width - 8.0,
            );
            let top = (anchor.origin.y - 6.0).min(visible.origin.y + visible.size.height - 6.0);
            let y = (top - height).max(visible.origin.y + 8.0);
            window.setFrame_display(
                NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
                true,
            );
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_file_urls_decode_escaped_names_and_reject_web_links() {
        assert_eq!(
            file_url_path(&NSString::from_str("file:///tmp/plan%20%231.zip")),
            Some("/tmp/plan #1.zip".into())
        );
        assert_eq!(
            file_url_path(&NSString::from_str("https://example.com/file.zip")),
            None
        );
    }
}
