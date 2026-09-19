use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gtk::glib::{self, ControlFlow, ObjectExt, Propagation, SignalHandlerId};
use gtk::prelude::*;
use gtk::{gdk, gdk_pixbuf};

use super::{Completion, NativeDragOutcome, NativeDragPayload};

const FILE_TARGET: u32 = 1;
const TEXT_TARGET: u32 = 2;
const HTML_TARGET: u32 = 3;

struct Session {
    completion: RefCell<Option<Completion>>,
    handlers: RefCell<Vec<SignalHandlerId>>,
    failed: Cell<bool>,
    data_sent: Cell<bool>,
}

fn finish(window: &gtk::ApplicationWindow, session: &Rc<Session>, outcome: NativeDragOutcome) {
    let completion = session.completion.borrow_mut().take();
    if let Some(completion) = completion {
        // Disconnect outside the signal currently dispatching; this also keeps
        // the payload alive until GTK has finished servicing the drop request.
        let window = window.clone();
        let session = session.clone();
        glib::idle_add_local_once(move || {
            for handler in session.handlers.borrow_mut().drain(..) {
                window.disconnect(handler);
            }
            window.drag_source_unset();
            completion(outcome);
        });
    }
}

pub(super) fn start(
    window: &tauri::WebviewWindow,
    payload: NativeDragPayload,
    completion: Completion,
) -> Result<(), String> {
    if !gtk::is_initialized_main_thread() {
        return Err("clipboard drag must start on the GTK main thread".into());
    }
    let window = window.gtk_window().map_err(|error| error.to_string())?;
    if let (Some(native), Some(pointer)) = (
        window.window(),
        window
            .display()
            .default_seat()
            .and_then(|seat| seat.pointer()),
    ) {
        if !native
            .device_position(&pointer)
            .3
            .contains(gdk::ModifierType::BUTTON1_MASK)
        {
            completion(NativeDragOutcome::Cancelled);
            return Ok(());
        }
    }
    let uris = payload
        .files
        .iter()
        .map(|path| glib::filename_to_uri(path, None).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader
        .write(&payload.preview_png)
        .map_err(|error| error.to_string())?;
    loader.close().map_err(|error| error.to_string())?;
    let icon = loader
        .pixbuf()
        .ok_or_else(|| "clipboard drag preview is invalid".to_string())?;
    let targets = gtk::TargetList::new(&[]);
    if !uris.is_empty() {
        targets.add_uri_targets(FILE_TARGET);
    } else {
        if payload.text.is_some() {
            targets.add_text_targets(TEXT_TARGET);
        }
        if payload.html.is_some() {
            targets.add(&gdk::Atom::intern("text/html"), 0, HTML_TARGET);
        }
    }
    window.drag_source_set(gdk::ModifierType::BUTTON1_MASK, &[], gdk::DragAction::COPY);
    window.drag_source_set_target_list(Some(&targets));
    let session = Rc::new(Session {
        completion: RefCell::new(Some(completion)),
        handlers: RefCell::new(Vec::new()),
        failed: Cell::new(false),
        data_sent: Cell::new(false),
    });
    let data_session = session.clone();
    let cancelled = payload.cancelled.clone();
    session
        .handlers
        .borrow_mut()
        .push(
            window.connect_drag_data_get(move |_, _, selection, info, _| {
                if cancelled.load(Ordering::Acquire) {
                    return;
                }
                let sent = match info {
                    FILE_TARGET => {
                        selection.set_uris(&uris.iter().map(|uri| uri.as_str()).collect::<Vec<_>>())
                    }
                    TEXT_TARGET => payload
                        .text
                        .as_deref()
                        .is_some_and(|text| selection.set_text(text)),
                    HTML_TARGET => {
                        if let Some(html) = payload.html.as_deref() {
                            selection.set(&gdk::Atom::intern("text/html"), 8, html.as_bytes());
                            true
                        } else {
                            false
                        }
                    }
                    _ => false,
                };
                data_session
                    .data_sent
                    .set(data_session.data_sent.get() || sent);
            }),
        );
    let failed_session = session.clone();
    session
        .handlers
        .borrow_mut()
        .push(window.connect_drag_failed(move |_, _, _| {
            failed_session.failed.set(true);
            Propagation::Proceed
        }));
    let ended_session = session.clone();
    session
        .handlers
        .borrow_mut()
        .push(window.connect_drag_end(move |window, context| {
            let outcome = if !ended_session.failed.get()
                && ended_session.data_sent.get()
                && context.selected_action().contains(gdk::DragAction::COPY)
            {
                NativeDragOutcome::Dropped
            } else {
                NativeDragOutcome::Cancelled
            };
            finish(window, &ended_session, outcome);
        }));
    let destroyed_session = session.clone();
    session
        .handlers
        .borrow_mut()
        .push(window.connect_destroy(move |window| {
            finish(window, &destroyed_session, NativeDragOutcome::Cancelled);
        }));
    // The argument is the button number, not GDK_BUTTON1_MASK.
    payload.offered.store(true, Ordering::Release);
    let Some(context) =
        window.drag_begin_with_coordinates(&targets, gdk::DragAction::COPY, 1, None, -1, -1)
    else {
        // Start errors are handled by the caller, rather than invoking both its
        // error and completion paths.
        session.completion.borrow_mut().take();
        for handler in session.handlers.borrow_mut().drain(..) {
            window.disconnect(handler);
        }
        window.drag_source_unset();
        return Err("could not start clipboard drag".into());
    };
    context.drag_set_icon_pixbuf(&icon, 0, 0);
    let weak_session = Rc::downgrade(&session);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let Some(session) = weak_session.upgrade() else {
            return ControlFlow::Break;
        };
        if session.completion.borrow().is_none() {
            return ControlFlow::Break;
        }
        if payload.cancelled.load(Ordering::Acquire) {
            context.drag_cancel();
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_uri_escapes_filename_delimiters() {
        let uri = gtk::glib::filename_to_uri("/tmp/你好 image#1%.png", None).unwrap();
        assert!(uri.as_str().starts_with("file:///tmp/"));
        assert!(uri.as_str().contains("%20image%231%25.png"));
        assert!(!uri.as_str().contains('#'));
    }
}
