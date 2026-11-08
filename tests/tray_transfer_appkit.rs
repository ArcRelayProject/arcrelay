//! Native regression checks; no Tokio runtime or packaged app is required.

#[cfg(target_os = "macos")]
#[path = "../src/windowing/tray_transfer/macos/view_host.rs"]
mod view_host;

#[cfg(target_os = "macos")]
#[link(name = "WebKit", kind = "framework")]
extern "C" {}

#[cfg(target_os = "macos")]
fn main() {
    use objc2::{rc::Retained, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new().expect("AppKit regression runs on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let frame = NSRect::new(NSPoint::ZERO, NSSize::new(440.0, 220.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    let content = window.contentView().expect("new window has a content view");
    // Use a real WKWebView while keeping the regression independent of Tauri
    // startup, the network, and any user files.
    let class = objc2::runtime::AnyClass::get(c"WKWebView").expect("WebKit is loaded");
    let webview: Retained<NSView> = unsafe {
        let allocated: objc2::rc::Allocated<NSView> = objc2::msg_send![class, alloc];
        objc2::msg_send![allocated, initWithFrame: content.bounds()]
    };
    content.addSubview(&webview);
    assert!(window.makeFirstResponder(Some(&webview)));
    let destination = NSView::initWithFrame(NSView::alloc(mtm), content.bounds());

    view_host::attach_drop_destination(&content, &destination);

    let attached = window
        .contentView()
        .expect("installing a drop target must not clear contentView");
    assert!(
        std::ptr::eq(Retained::as_ptr(&attached), Retained::as_ptr(&content)),
        "the original content view must remain the window root"
    );
    let parent = unsafe { webview.superview() }.expect("webview remains attached");
    assert!(
        std::ptr::eq(Retained::as_ptr(&parent), Retained::as_ptr(&destination)),
        "drop destination must be the webview's ancestor"
    );
    assert!(
        webview.window().is_some(),
        "webview remains in the native window"
    );
    let responder = window
        .firstResponder()
        .expect("WebView keeps keyboard focus");
    assert!(std::ptr::eq(
        Retained::as_ptr(&responder).cast::<NSView>(),
        Retained::as_ptr(&webview)
    ));

    for height in [520.0, 640.0, 220.0] {
        window.setFrame_display(
            NSRect::new(NSPoint::ZERO, NSSize::new(440.0, height)),
            false,
        );
        let attached = window
            .contentView()
            .expect("resizing must not clear contentView");
        assert!(std::ptr::eq(
            Retained::as_ptr(&attached),
            Retained::as_ptr(&content)
        ));
        assert_eq!(destination.frame().size, content.bounds().size);
        assert!(webview.window().is_some());
    }
    // Closing a retained panel must still leave its content readable for a
    // subsequent reopen/resize command.
    window.orderOut(None);
    assert!(window.contentView().is_some());
    println!("AppKit transfer drop host: construction, resize and hide passed without Tokio");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
