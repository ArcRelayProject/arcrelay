use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApp, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSPasteboardItem, NSView,
};
use objc2_foundation::{NSArray, NSData, NSOperationQueue, NSPoint, NSRect, NSString, NSURL};

use super::{Completion, NativeDragOutcome, NativeDragPayload};

thread_local! {
    // AppKit drag sources and views are main-thread-only. Retain the source
    // explicitly until AppKit's final callback, independent of IPC lifetimes.
    static SOURCES: RefCell<HashMap<String, Retained<ClipboardDragSource>>> = RefCell::new(HashMap::new());
}

struct SourceIvars {
    id: String,
    completion: RefCell<Option<Completion>>,
    cancelled: Arc<AtomicBool>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArcRelayClipboardDragSource"]
    #[ivars = SourceIvars]
    struct ClipboardDragSource;

    unsafe impl NSObjectProtocol for ClipboardDragSource {}

    unsafe impl NSDraggingSource for ClipboardDragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            if self.ivars().cancelled.load(Ordering::Acquire) {
                NSDragOperation::None
            } else {
                NSDragOperation::Copy
            }
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
            // Release on the next main-queue turn, after this delegate call.
            let id = self.ivars().id.clone();
            let completion = RefCell::new(self.ivars().completion.borrow_mut().take());
            let outcome = if operation == NSDragOperation::None {
                NativeDragOutcome::Cancelled
            } else {
                NativeDragOutcome::Dropped
            };
            let release = block2::RcBlock::new(move || {
                SOURCES.with(|sources| sources.borrow_mut().remove(&id));
                if let Some(completion) = completion.borrow_mut().take() {
                    // The caller may hide or destroy the source window. It
                    // must run after AppKit's ending callback has unwound.
                    completion(outcome);
                }
            });
            unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&release) };
        }
    }
);

impl ClipboardDragSource {
    fn new(
        id: String,
        completion: Completion,
        cancelled: Arc<AtomicBool>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SourceIvars {
            id,
            completion: RefCell::new(Some(completion)),
            cancelled,
        });
        unsafe { msg_send![super(this), init] }
    }
}

pub(super) fn start(
    window: &tauri::WebviewWindow,
    payload: NativeDragPayload,
    completion: Completion,
) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "clipboard drag must start on the main thread".to_string())?;
    if NSEvent::pressedMouseButtons() & 1 == 0 {
        completion(NativeDragOutcome::Cancelled);
        return Ok(());
    }
    let ns_view_pointer = window.ns_view().map_err(|error| error.to_string())?;
    let ns_view = unsafe { &*ns_view_pointer.cast::<NSView>() };
    let ns_window = ns_view
        .window()
        .ok_or_else(|| "clipboard drag window is unavailable".to_string())?;
    let content_view = ns_window
        .contentView()
        .ok_or_else(|| "clipboard drag content view is unavailable".to_string())?;
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::from_vec(payload.preview_png))
        .ok_or_else(|| "clipboard drag preview is invalid".to_string())?;
    let cursor = ns_window.mouseLocationOutsideOfEventStream();
    let image_size = image.size();
    let frame = NSRect::new(
        NSPoint::new(
            cursor.x - image_size.width / 2.0,
            cursor.y - image_size.height / 2.0,
        ),
        image_size,
    );
    let mut items = Vec::new();
    if payload.files.is_empty() {
        let pasteboard = NSPasteboardItem::new();
        if let Some(text) = payload.text {
            if !pasteboard.setString_forType(
                &NSString::from_str(&text),
                &NSString::from_str("public.utf8-plain-text"),
            ) {
                return Err("could not prepare clipboard drag text".into());
            }
        }
        if let Some(html) = payload.html {
            if !pasteboard.setData_forType(
                &NSData::from_vec(html.into_bytes()),
                &NSString::from_str("public.html"),
            ) {
                return Err("could not prepare clipboard drag HTML".into());
            }
        }
        items.push(NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            &ProtocolObject::from_retained(pasteboard),
        ));
    } else {
        for path in payload.files {
            let file_path = CString::new(path.as_os_str().as_bytes())
                .map_err(|_| "clipboard drag path contains a null byte".to_string())?;
            // Preserve filesystem bytes and let NSURL escape spaces, # and %.
            let url = unsafe {
                NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                    NonNull::new(file_path.as_ptr().cast_mut()).expect("CString is non-null"),
                    // File URLs also represent directories. The backend has
                    // already checked the filesystem; do not stat on the UI
                    // thread merely to append a directory separator.
                    false,
                    None,
                )
            };
            items.push(NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                &ProtocolObject::from_retained(url),
            ));
        }
    }
    for item in &items {
        unsafe { item.setDraggingFrame_contents(frame, Some(&*image)) };
    }
    let items = NSArray::from_retained_slice(&items);
    let current = NSApp(mtm).currentEvent();
    let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDragged,
        cursor,
        current.as_ref().map_or(NSEventModifierFlags::empty(), |event| event.modifierFlags()),
        current.as_ref().map_or(0.0, |event| event.timestamp()),
        ns_window.windowNumber(),
        None,
        0,
        1,
        1.0,
    ).ok_or_else(|| "could not create clipboard drag event".to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let source = ClipboardDragSource::new(id.clone(), completion, payload.cancelled, mtm);
    SOURCES.with(|sources| sources.borrow_mut().insert(id, source.clone()));
    let source = ProtocolObject::<dyn NSDraggingSource>::from_retained(source);
    payload.offered.store(true, Ordering::Release);
    let session = content_view.beginDraggingSessionWithItems_event_source(&items, &event, &source);
    session.setAnimatesToStartingPositionsOnCancelOrFail(true);
    Ok(())
}
