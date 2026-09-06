use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApp, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSEvent, NSEventModifierFlags, NSEventType, NSFilePromiseProvider,
    NSFilePromiseProviderDelegate, NSImage, NSPasteboardWriting, NSView,
};
use objc2_foundation::{
    NSArray, NSError, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use tauri::{AppHandle, WebviewWindow};

use arcrelay_protocol::remote_files::RemoteFileKind;

use crate::clipboard_sync::ClipboardSyncManager;

use super::{
    finish_remote_file_transfer, register_remote_file_transfer, remote_file_progress_callback,
};

static ACTIVE_PROMISES: OnceLock<Mutex<HashMap<String, Retained<RemoteFilePromiseDelegate>>>> =
    OnceLock::new();

fn active_promises() -> &'static Mutex<HashMap<String, Retained<RemoteFilePromiseDelegate>>> {
    ACTIVE_PROMISES.get_or_init(|| Mutex::new(HashMap::new()))
}

struct RemoteFilePromiseIvars {
    id: String,
    app: AppHandle,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    name: String,
    directory_path: String,
    claimed: AtomicBool,
    operation_queue: Retained<NSOperationQueue>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "ArcRelayRemoteFilePromiseDelegate"]
    #[ivars = RemoteFilePromiseIvars]
    struct RemoteFilePromiseDelegate;

    unsafe impl NSObjectProtocol for RemoteFilePromiseDelegate {}

    unsafe impl NSFilePromiseProviderDelegate for RemoteFilePromiseDelegate {
        #[unsafe(method_id(filePromiseProvider:fileNameForType:))]
        fn promised_file_name(
            &self,
            _provider: &NSFilePromiseProvider,
            _file_type: &NSString,
        ) -> Retained<NSString> {
            NSString::from_str(&self.ivars().name)
        }

        #[unsafe(method(filePromiseProvider:writePromiseToURL:completionHandler:))]
        fn fulfill_promise(
            &self,
            _provider: &NSFilePromiseProvider,
            destination_url: &NSURL,
            completion_handler: &block2::DynBlock<dyn Fn(*mut NSError)>,
        ) {
            fulfill_remote_file_promise(self, destination_url, completion_handler);
        }

        #[unsafe(method_id(operationQueueForFilePromiseProvider:))]
        fn promise_operation_queue(
            &self,
            _provider: &NSFilePromiseProvider,
        ) -> Retained<NSOperationQueue> {
            self.ivars().operation_queue.clone()
        }
    }
);

fn fulfill_remote_file_promise(
    delegate: &RemoteFilePromiseDelegate,
    destination_url: &NSURL,
    completion_handler: &block2::DynBlock<dyn Fn(*mut NSError)>,
) {
    let ivars = delegate.ivars();
    ivars.claimed.store(true, Ordering::Release);
    let destination = destination_url
        .path()
        .map(|path| PathBuf::from(path.to_string()));

    let result = if let Some(destination) = destination {
        let session = register_remote_file_transfer(
            &ivars.app,
            "download",
            ivars.name.clone(),
            ivars.peer_id.clone(),
            ivars.share_id.clone(),
            ivars.directory_path.clone(),
        );
        let progress = remote_file_progress_callback(ivars.app.clone(), session.id.clone());
        let result =
            tauri::async_runtime::block_on(ivars.manager.download_remote_file_with_progress(
                &ivars.peer_id,
                ivars.share_id.clone(),
                ivars.relative_path.clone(),
                destination,
                Some(progress),
            ));
        finish_remote_file_transfer(
            &ivars.app,
            &session.id,
            result.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        result.map(|_| ())
    } else {
        Err("drag target is not a local folder".to_string())
    };

    if let Err(error) = result {
        tracing::warn!(%error, "remote file promise download failed");
        let domain = NSString::from_str("com.arcrelay.remote-file-promise");
        let ns_error = unsafe { NSError::errorWithDomain_code_userInfo(&domain, 1, None) };
        completion_handler.call((Retained::as_ptr(&ns_error).cast_mut(),));
    } else {
        completion_handler.call((std::ptr::null_mut(),));
    }

    // NSFilePromiseProvider keeps a weak delegate. Release our strong global
    // reference just after this Objective-C delegate call has returned.
    schedule_promise_release(ivars.id.clone());
}

impl RemoteFilePromiseDelegate {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: String,
        app: AppHandle,
        manager: Arc<ClipboardSyncManager>,
        peer_id: String,
        share_id: String,
        relative_path: String,
        name: String,
        directory_path: String,
    ) -> Retained<Self> {
        let operation_queue = NSOperationQueue::init(NSOperationQueue::alloc());
        operation_queue.setMaxConcurrentOperationCount(1);
        operation_queue.setName(Some(&NSString::from_str("ArcRelay remote file promise")));
        let this = Self::alloc().set_ivars(RemoteFilePromiseIvars {
            id,
            app,
            manager,
            peer_id,
            share_id,
            relative_path,
            name,
            directory_path,
            claimed: AtomicBool::new(false),
            operation_queue,
        });
        unsafe { msg_send![super(this), init] }
    }
}

struct RemoteFileDragSourceIvars {
    promise_id: String,
    ended: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ArcRelayRemoteFileDragSource"]
    #[ivars = RemoteFileDragSourceIvars]
    struct RemoteFileDragSource;

    unsafe impl NSObjectProtocol for RemoteFileDragSource {}

    unsafe impl NSDraggingSource for RemoteFileDragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation_mask(
            &self,
            session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            session.setAnimatesToStartingPositionsOnCancelOrFail(true);
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn dragging_ended(
            &self,
            _session: &NSDraggingSession,
            _point: NSPoint,
            operation: NSDragOperation,
        ) {
            if operation == NSDragOperation::None {
                active_promises()
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&self.ivars().promise_id);
            } else {
                schedule_unclaimed_promise_cleanup(self.ivars().promise_id.clone());
            }
            if let Some(ended) = self
                .ivars()
                .ended
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take()
            {
                let _ = ended.send(());
            }
        }
    }
);

impl RemoteFileDragSource {
    fn new(
        promise_id: String,
        ended: tokio::sync::oneshot::Sender<()>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(RemoteFileDragSourceIvars {
            promise_id,
            ended: Mutex::new(Some(ended)),
        });
        unsafe { msg_send![super(this), init] }
    }
}

fn schedule_unclaimed_promise_cleanup(promise_id: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10 * 60)).await;
        let mut promises = active_promises()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let should_remove = promises
            .get(&promise_id)
            .is_some_and(|promise| !promise.ivars().claimed.load(Ordering::Acquire));
        if should_remove {
            promises.remove(&promise_id);
        }
    });
}

fn schedule_promise_release(promise_id: String) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        active_promises()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&promise_id);
    });
}

#[allow(clippy::too_many_arguments)]
pub async fn start_remote_file_promise_drag(
    app: AppHandle,
    window: WebviewWindow,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    kind: RemoteFileKind,
    icon_path: PathBuf,
) -> Result<(), String> {
    let name = relative_path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("remote file")
        .to_string();
    let directory_path = relative_path
        .rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default();
    let promise_id = uuid::Uuid::new_v4().to_string();
    let (ended_tx, ended_rx) = tokio::sync::oneshot::channel();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let main_app = app.clone();
    let main_promise_id = promise_id.clone();

    app.run_on_main_thread(move || {
        let result = unsafe {
            start_drag_on_main_thread(
                main_app,
                window,
                manager,
                peer_id,
                share_id,
                relative_path,
                name,
                directory_path,
                kind,
                icon_path,
                main_promise_id,
                ended_tx,
            )
        };
        let _ = started_tx.send(result);
    })
    .map_err(|error| error.to_string())?;

    started_rx
        .await
        .map_err(|_| "failed to start remote file drag".to_string())??;
    ended_rx
        .await
        .map_err(|_| "remote file drag ended unexpectedly".to_string())
}

#[allow(clippy::too_many_arguments)]
unsafe fn start_drag_on_main_thread(
    app: AppHandle,
    window: WebviewWindow,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    name: String,
    directory_path: String,
    kind: RemoteFileKind,
    icon_path: PathBuf,
    promise_id: String,
    ended: tokio::sync::oneshot::Sender<()>,
) -> Result<(), String> {
    let mtm =
        MainThreadMarker::new().ok_or_else(|| "drag must start on the main thread".to_string())?;
    let ns_view_pointer = window.ns_view().map_err(|error| error.to_string())?;
    let ns_view = unsafe { &*ns_view_pointer.cast::<NSView>() };
    let ns_window = ns_view
        .window()
        .ok_or_else(|| "remote file window not found".to_string())?;
    let content_view = ns_window
        .contentView()
        .ok_or_else(|| "remote file content view not found".to_string())?;
    let current_position = ns_window.mouseLocationOutsideOfEventStream();

    let image = NSImage::initByReferencingFile(
        NSImage::alloc(),
        &NSString::from_str(&icon_path.to_string_lossy()),
    )
    .ok_or_else(|| "failed to create remote file drag preview".to_string())?;
    let image_size: NSSize = image.size();
    let image_rect = NSRect::new(
        NSPoint::new(
            current_position.x - image_size.width / 2.0,
            current_position.y - image_size.height / 2.0,
        ),
        image_size,
    );

    let delegate = RemoteFilePromiseDelegate::new(
        promise_id.clone(),
        app,
        manager,
        peer_id,
        share_id,
        relative_path,
        name,
        directory_path,
    );
    let file_type = NSString::from_str(match kind {
        RemoteFileKind::File => "public.data",
        RemoteFileKind::Folder => "public.folder",
    });
    let provider = NSFilePromiseProvider::initWithFileType_delegate(
        NSFilePromiseProvider::alloc(),
        &file_type,
        ProtocolObject::from_ref(&*delegate),
    );
    active_promises()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(promise_id.clone(), delegate);

    let provider = ProtocolObject::<dyn NSPasteboardWriting>::from_retained(provider);
    let dragging_item =
        NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), &provider);
    unsafe { dragging_item.setDraggingFrame_contents(image_rect, Some(&*image)) };
    let dragging_items = NSArray::from_retained_slice(&[dragging_item]);

    let timestamp = NSApp(mtm)
        .currentEvent()
        .map(|event| event.timestamp())
        .unwrap_or(0.0);
    let drag_event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        NSEventType::LeftMouseDragged,
        current_position,
        NSEventModifierFlags::empty(),
        timestamp,
        ns_window.windowNumber(),
        None,
        0,
        1,
        1.0,
    )
    .ok_or_else(|| "failed to create remote file drag event".to_string())?;
    let source = RemoteFileDragSource::new(promise_id, ended, mtm);
    let source = ProtocolObject::<dyn NSDraggingSource>::from_retained(source);
    content_view.beginDraggingSessionWithItems_event_source(&dragging_items, &drag_event, &source);
    Ok(())
}
