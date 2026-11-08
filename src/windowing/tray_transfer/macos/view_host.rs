use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};

/// Install the native drop ancestor without replacing AppKit's content view.
pub(super) fn attach_drop_destination(content: &NSView, destination: &NSView) {
    let resize_mask =
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable;
    destination.setFrame(content.bounds());
    destination.setAutoresizingMask(resize_mask);
    let window = content.window();
    let responder = window.as_ref().and_then(|window| window.firstResponder());
    let children = content.subviews();
    // Replacing NSWindow.contentView detaches Wry's root and can expose a nil
    // contentView to reentrant frontend resize commands. Keep that root in place
    // and insert only an ancestor of its existing WebView children.
    content.addSubview(destination);
    for child in children {
        child.setAutoresizingMask(resize_mask);
        destination.addSubview(&child);
    }
    if let (Some(window), Some(responder)) = (window, responder) {
        window.makeFirstResponder(Some(&responder));
    }
}
