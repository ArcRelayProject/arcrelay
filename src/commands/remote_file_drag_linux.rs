use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gtk::glib::{self, ObjectExt, Propagation, SignalHandlerId};
use gtk::prelude::{DragContextExtManual, PixbufLoaderExt, WidgetExt, WidgetExtManual};
use gtk::{gdk, gdk_pixbuf};
use tauri::{AppHandle, WebviewWindow};

use crate::clipboard_sync::ClipboardSyncManager;

use super::{
    finish_remote_file_transfer, register_remote_file_transfer, remote_file_progress_callback,
};

struct LazyDownload {
    app: AppHandle,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    destination: PathBuf,
    result: Option<Result<PathBuf, String>>,
}

impl LazyDownload {
    fn download(&mut self) -> Result<PathBuf, String> {
        if let Some(result) = &self.result {
            return result.clone();
        }
        let name = self
            .relative_path
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("remote file")
            .to_string();
        let directory_path = self
            .relative_path
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_default();
        let session = register_remote_file_transfer(
            &self.app,
            "download",
            name,
            self.peer_id.clone(),
            self.share_id.clone(),
            directory_path,
        );
        let progress = remote_file_progress_callback(self.app.clone(), session.id.clone());
        let result =
            tauri::async_runtime::block_on(self.manager.download_remote_file_with_progress(
                &self.peer_id,
                self.share_id.clone(),
                self.relative_path.clone(),
                self.destination.clone(),
                Some(progress),
            ));
        finish_remote_file_transfer(
            &self.app,
            &session.id,
            result.as_ref().map(|_| ()).map_err(Clone::clone),
        );
        self.result = Some(result.clone());
        result
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn start_remote_file_promise_drag(
    app: AppHandle,
    window: WebviewWindow,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    icon_path: PathBuf,
) -> Result<(), String> {
    let destination = std::env::temp_dir()
        .join("ArcRelay")
        .join("remote-drag")
        .join(uuid::Uuid::new_v4().to_string());
    let (ended, ended_receiver) = tokio::sync::oneshot::channel();
    let (started, started_receiver) = tokio::sync::oneshot::channel();
    let main_app = app.clone();

    app.run_on_main_thread(move || {
        let result = start_drag_on_main_thread(
            main_app,
            window,
            manager,
            peer_id,
            share_id,
            relative_path,
            destination,
            icon_path,
            ended,
        );
        let _ = started.send(result);
    })
    .map_err(|error| error.to_string())?;

    started_receiver
        .await
        .map_err(|_| "failed to start Linux remote file drag".to_string())??;
    ended_receiver
        .await
        .map_err(|_| "Linux remote file drag ended unexpectedly".to_string())?
}

#[allow(clippy::too_many_arguments)]
fn start_drag_on_main_thread(
    app: AppHandle,
    window: WebviewWindow,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    destination: PathBuf,
    icon_path: PathBuf,
    ended: tokio::sync::oneshot::Sender<Result<(), String>>,
) -> Result<(), String> {
    let window = window.gtk_window().map_err(|error| error.to_string())?;
    let download = Rc::new(RefCell::new(LazyDownload {
        app,
        manager,
        peer_id,
        share_id,
        relative_path,
        destination,
        result: None,
    }));
    let ended = Rc::new(RefCell::new(Some(ended)));
    let handlers = Rc::new(RefCell::new(Vec::<SignalHandlerId>::new()));

    window.drag_source_set(gdk::ModifierType::BUTTON1_MASK, &[], gdk::DragAction::COPY);
    window.drag_source_add_uri_targets();

    let data_download = download.clone();
    handlers
        .borrow_mut()
        .push(window.connect_drag_data_get(move |_, _, selection, _, _| {
            match data_download.borrow_mut().download() {
                Ok(path) => match glib::filename_to_uri(path, None) {
                    Ok(uri) => {
                        selection.set_uris(&[uri.as_str()]);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "failed to create Linux drag file URI");
                    }
                },
                Err(error) => {
                    tracing::warn!(%error, "Linux deferred remote drag download failed");
                }
            }
        }));

    let failed_window = window.clone();
    let failed_handlers = handlers.clone();
    let failed_ended = ended.clone();
    let failed_download = download.clone();
    handlers
        .borrow_mut()
        .push(window.connect_drag_failed(move |_, _, _| {
            finish_drag(
                &failed_window,
                &failed_handlers,
                &failed_ended,
                failed_download.borrow().result.clone(),
            );
            Propagation::Proceed
        }));

    let end_window = window.clone();
    let end_handlers = handlers.clone();
    let end_ended = ended.clone();
    let end_download = download.clone();
    handlers
        .borrow_mut()
        .push(window.connect_drag_end(move |_, _| {
            finish_drag(
                &end_window,
                &end_handlers,
                &end_ended,
                end_download.borrow().result.clone(),
            );
        }));

    let target_list = window
        .drag_source_get_target_list()
        .ok_or_else(|| "Linux drag target list is empty".to_string())?;
    let context = window
        .drag_begin_with_coordinates(
            &target_list,
            gdk::DragAction::COPY,
            gdk::ffi::GDK_BUTTON1_MASK as i32,
            None,
            -1,
            -1,
        )
        .ok_or_else(|| "failed to start Linux remote file drag".to_string())?;
    if let Some(icon) = load_drag_icon(&icon_path) {
        context.drag_set_icon_pixbuf(&icon, 0, 0);
    }
    Ok(())
}

fn load_drag_icon(path: &std::path::Path) -> Option<gdk_pixbuf::Pixbuf> {
    let bytes = std::fs::read(path).ok()?;
    let loader = gdk_pixbuf::PixbufLoader::new();
    loader.write(&bytes).ok()?;
    loader.close().ok()?;
    loader.pixbuf()
}

fn finish_drag(
    window: &gtk::ApplicationWindow,
    handlers: &Rc<RefCell<Vec<SignalHandlerId>>>,
    ended: &Rc<RefCell<Option<tokio::sync::oneshot::Sender<Result<(), String>>>>>,
    result: Option<Result<PathBuf, String>>,
) {
    if let Some(sender) = ended.borrow_mut().take() {
        let _ = sender.send(result.map(|value| value.map(|_| ())).unwrap_or(Ok(())));
    }
    let window = window.clone();
    let handlers = handlers.clone();
    glib::idle_add_local_once(move || {
        for handler in handlers.borrow_mut().drain(..) {
            window.disconnect(handler);
        }
        window.drag_source_unset();
    });
}
