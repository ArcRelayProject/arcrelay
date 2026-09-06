use std::collections::VecDeque;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::ManuallyDrop;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{implement, Error, Ref, BOOL, HRESULT};
use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL,
    OLE_E_ADVISENOTSUPPORTED, STG_E_ACCESSDENIED, STG_E_INVALIDFUNCTION, STG_E_READFAULT, S_FALSE,
    S_OK,
};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL};
use windows::Win32::System::Com::{
    IAdviseSink, IBindCtx, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
    ISequentialStream_Impl, IStream, IStream_Impl, DATADIR_GET, DVASPECT_CONTENT, FORMATETC,
    LOCKTYPE, STATFLAG, STATSTG, STGC, STGMEDIUM, STGMEDIUM_0, STGTY_STREAM, STREAM_SEEK,
    STREAM_SEEK_CUR, STREAM_SEEK_END, STREAM_SEEK_SET, TYMED_HGLOBAL, TYMED_ISTREAM,
};
use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, OleUninitialize, DROPEFFECT,
    DROPEFFECT_COPY,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{
    IDataObjectAsyncCapability, IDataObjectAsyncCapability_Impl, SHCreateStdEnumFmtEtc,
    CFSTR_FILECONTENTS, CFSTR_FILEDESCRIPTORW, FD_ATTRIBUTES, FD_FILESIZE, FD_PROGRESSUI,
    FILEDESCRIPTORW,
};

use arcrelay_protocol::remote_files::RemoteFileKind;
use tauri::{AppHandle, WebviewWindow};

use crate::clipboard_sync::{
    ClipboardSyncManager, RemoteFileProgressCallback, RemoteFileStreamReceiver,
    RemoteFileTransferProgress,
};

use super::{
    finish_remote_file_transfer, register_remote_file_transfer, remote_file_progress_callback,
};

const MAX_VIRTUAL_DRAG_ENTRIES: usize = 10_000;
const MAX_WINDOWS_VIRTUAL_PATH_UNITS: usize = 259;

#[derive(Clone)]
struct VirtualEntry {
    virtual_path: String,
    remote_path: String,
    kind: RemoteFileKind,
    size: u64,
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
    size: u64,
) -> Result<(), String> {
    let entries =
        collect_virtual_entries(&manager, &peer_id, &share_id, &relative_path, kind, size).await?;
    let (completed, receiver) = tokio::sync::oneshot::channel();

    app.clone()
        .run_on_main_thread(move || {
            let result = run_drag(app, window, manager, peer_id, share_id, entries);
            let _ = completed.send(result);
        })
        .map_err(|error| error.to_string())?;

    receiver
        .await
        .map_err(|_| "Windows remote file drag ended unexpectedly".to_string())?
}

async fn collect_virtual_entries(
    manager: &Arc<ClipboardSyncManager>,
    peer_id: &str,
    share_id: &str,
    relative_path: &str,
    kind: RemoteFileKind,
    size: u64,
) -> Result<Vec<VirtualEntry>, String> {
    let root_name = remote_name(relative_path)?.to_string();
    let mut entries = vec![VirtualEntry {
        virtual_path: root_name.clone(),
        remote_path: relative_path.to_string(),
        kind,
        size,
    }];
    if kind == RemoteFileKind::File {
        validate_virtual_path(&root_name)?;
        return Ok(entries);
    }

    let mut directories = VecDeque::from([(relative_path.to_string(), root_name)]);
    while let Some((remote_parent, virtual_parent)) = directories.pop_front() {
        let children = manager
            .list_remote_directory_all(peer_id, share_id, &remote_parent)
            .await?;
        for entry in children {
            if entries.len() >= MAX_VIRTUAL_DRAG_ENTRIES {
                return Err("folder has too many entries for Windows virtual drag export".into());
            }
            let virtual_path = format!("{virtual_parent}\\{}", entry.name);
            validate_virtual_path(&virtual_path)?;
            if entry.kind == RemoteFileKind::Folder {
                directories.push_back((entry.relative_path.clone(), virtual_path.clone()));
            }
            entries.push(VirtualEntry {
                virtual_path,
                remote_path: entry.relative_path,
                kind: entry.kind,
                size: entry.size,
            });
        }
    }
    Ok(entries)
}

fn remote_name(path: &str) -> Result<&str, String> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| "remote path has no usable name".to_string())
}

fn validate_virtual_path(path: &str) -> Result<(), String> {
    let units = OsStr::new(path).encode_wide().count();
    if units > MAX_WINDOWS_VIRTUAL_PATH_UNITS {
        Err(format!("Windows virtual drag path is too long: {path}"))
    } else {
        Ok(())
    }
}

fn run_drag(
    app: AppHandle,
    _window: WebviewWindow,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    entries: Vec<VirtualEntry>,
) -> Result<(), String> {
    unsafe { OleInitialize(None) }.map_err(|error| error.to_string())?;
    let result = (|| -> windows::core::Result<HRESULT> {
        let data_object: IDataObject =
            RemoteFileDataObject::new(app, manager, peer_id, share_id, entries)?.into();
        let drop_source: IDropSource = RemoteFileDropSource.into();
        let mut effect = DROPEFFECT::default();
        Ok(unsafe { DoDragDrop(&data_object, &drop_source, DROPEFFECT_COPY, &mut effect) })
    })();
    unsafe { OleUninitialize() };

    match result.map_err(|error| error.to_string())? {
        DRAGDROP_S_DROP | DRAGDROP_S_CANCEL => Ok(()),
        result if result.is_err() => Err(Error::from_hresult(result).to_string()),
        _ => Ok(()),
    }
}

#[implement(IDropSource)]
struct RemoteFileDropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for RemoteFileDropSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape_pressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (key_state & MK_LBUTTON) == MODIFIERKEYS_FLAGS(0) {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[implement(IDataObject, IDataObjectAsyncCapability)]
struct RemoteFileDataObject {
    app: AppHandle,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    entries: Vec<VirtualEntry>,
    descriptor_format: u16,
    contents_format: u16,
    async_mode: AtomicBool,
    in_operation: AtomicBool,
}

impl RemoteFileDataObject {
    fn new(
        app: AppHandle,
        manager: Arc<ClipboardSyncManager>,
        peer_id: String,
        share_id: String,
        entries: Vec<VirtualEntry>,
    ) -> windows::core::Result<Self> {
        let descriptor_format = unsafe { RegisterClipboardFormatW(CFSTR_FILEDESCRIPTORW) };
        let contents_format = unsafe { RegisterClipboardFormatW(CFSTR_FILECONTENTS) };
        if descriptor_format == 0 || contents_format == 0 {
            return Err(Error::from_win32());
        }
        Ok(Self {
            app,
            manager,
            peer_id,
            share_id,
            entries,
            descriptor_format: descriptor_format as u16,
            contents_format: contents_format as u16,
            async_mode: AtomicBool::new(true),
            in_operation: AtomicBool::new(false),
        })
    }

    fn descriptor_format_etc(&self) -> FORMATETC {
        FORMATETC {
            cfFormat: self.descriptor_format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    fn contents_format_etc(&self) -> FORMATETC {
        FORMATETC {
            cfFormat: self.contents_format,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0 as u32,
            lindex: -1,
            tymed: TYMED_ISTREAM.0 as u32,
        }
    }

    fn supports_descriptor(&self, format: &FORMATETC) -> bool {
        format.cfFormat == self.descriptor_format
            && format.dwAspect == DVASPECT_CONTENT.0 as u32
            && format.tymed & TYMED_HGLOBAL.0 as u32 != 0
    }

    fn content_entry(&self, format: &FORMATETC) -> Option<&VirtualEntry> {
        if format.cfFormat != self.contents_format
            || format.dwAspect != DVASPECT_CONTENT.0 as u32
            || format.tymed & TYMED_ISTREAM.0 as u32 == 0
            || format.lindex < 0
        {
            return None;
        }
        self.entries
            .get(format.lindex as usize)
            .filter(|entry| entry.kind == RemoteFileKind::File)
    }

    fn file_group_descriptor(&self) -> windows::core::Result<STGMEDIUM> {
        let header_size = std::mem::size_of::<u32>();
        let descriptor_size = std::mem::size_of::<FILEDESCRIPTORW>();
        let allocation_size = header_size
            .checked_add(descriptor_size.saturating_mul(self.entries.len()))
            .ok_or_else(|| Error::new(STG_E_INVALIDFUNCTION, ""))?;
        let handle = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, allocation_size) }?;
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return Err(Error::from_win32());
        }
        unsafe {
            (pointer as *mut u32).write_unaligned(self.entries.len() as u32);
            for (index, entry) in self.entries.iter().enumerate() {
                let descriptor = descriptor_for_entry(entry)?;
                let destination = (pointer as *mut u8)
                    .add(header_size + descriptor_size * index)
                    .cast::<FILEDESCRIPTORW>();
                destination.write_unaligned(descriptor);
            }
            let _ = GlobalUnlock(handle);
        }
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: handle },
            pUnkForRelease: ManuallyDrop::new(None),
        })
    }

    fn stream_for_entry(&self, entry: &VirtualEntry) -> STGMEDIUM {
        let stream: IStream = RemoteFileStream::new(
            self.app.clone(),
            self.manager.clone(),
            self.peer_id.clone(),
            self.share_id.clone(),
            entry.remote_path.clone(),
            remote_name(&entry.remote_path)
                .unwrap_or("remote file")
                .to_string(),
            entry.size,
        )
        .into();
        STGMEDIUM {
            tymed: TYMED_ISTREAM.0 as u32,
            u: STGMEDIUM_0 {
                pstm: ManuallyDrop::new(Some(stream)),
            },
            pUnkForRelease: ManuallyDrop::new(None),
        }
    }
}

fn descriptor_for_entry(entry: &VirtualEntry) -> windows::core::Result<FILEDESCRIPTORW> {
    let encoded = OsStr::new(&entry.virtual_path)
        .encode_wide()
        .collect::<Vec<_>>();
    if encoded.len() > MAX_WINDOWS_VIRTUAL_PATH_UNITS {
        return Err(Error::new(
            STG_E_INVALIDFUNCTION,
            "Windows virtual file path is too long",
        ));
    }
    let mut file_name = [0_u16; 260];
    file_name[..encoded.len()].copy_from_slice(&encoded);
    let (flags, attributes, size_high, size_low) = match entry.kind {
        RemoteFileKind::Folder => (FD_ATTRIBUTES.0 as u32, FILE_ATTRIBUTE_DIRECTORY.0, 0, 0),
        RemoteFileKind::File => (
            (FD_ATTRIBUTES.0 | FD_FILESIZE.0 | FD_PROGRESSUI.0) as u32,
            FILE_ATTRIBUTE_NORMAL.0,
            (entry.size >> 32) as u32,
            entry.size as u32,
        ),
    };
    Ok(FILEDESCRIPTORW {
        dwFlags: flags,
        dwFileAttributes: attributes,
        nFileSizeHigh: size_high,
        nFileSizeLow: size_low,
        cFileName: file_name,
        ..Default::default()
    })
}

#[allow(non_snake_case)]
impl IDataObject_Impl for RemoteFileDataObject_Impl {
    fn GetData(&self, format: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
        let Some(format) = (unsafe { format.as_ref() }) else {
            return Err(Error::new(DV_E_FORMATETC, ""));
        };
        if self.supports_descriptor(format) {
            self.file_group_descriptor()
        } else if let Some(entry) = self.content_entry(format) {
            Ok(self.stream_for_entry(entry))
        } else {
            Err(Error::new(DV_E_FORMATETC, ""))
        }
    }

    fn GetDataHere(
        &self,
        _format: *const FORMATETC,
        _medium: *mut STGMEDIUM,
    ) -> windows::core::Result<()> {
        Err(Error::new(DV_E_FORMATETC, ""))
    }

    fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
        let Some(format) = (unsafe { format.as_ref() }) else {
            return DV_E_FORMATETC;
        };
        if self.supports_descriptor(format) || self.content_entry(format).is_some() {
            S_OK
        } else {
            DV_E_FORMATETC
        }
    }

    fn GetCanonicalFormatEtc(&self, _input: *const FORMATETC, output: *mut FORMATETC) -> HRESULT {
        if let Some(output) = unsafe { output.as_mut() } {
            output.ptd = std::ptr::null_mut();
        }
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _format: *const FORMATETC,
        _medium: *const STGMEDIUM,
        _release: BOOL,
    ) -> windows::core::Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn EnumFormatEtc(&self, direction: u32) -> windows::core::Result<IEnumFORMATETC> {
        if direction != DATADIR_GET.0 as u32 {
            return Err(Error::new(E_NOTIMPL, ""));
        }
        unsafe {
            SHCreateStdEnumFmtEtc(&[self.descriptor_format_etc(), self.contents_format_etc()])
        }
    }

    fn DAdvise(
        &self,
        _format: *const FORMATETC,
        _flags: u32,
        _sink: Ref<'_, IAdviseSink>,
    ) -> windows::core::Result<u32> {
        Err(Error::new(OLE_E_ADVISENOTSUPPORTED, ""))
    }

    fn DUnadvise(&self, _connection: u32) -> windows::core::Result<()> {
        Err(Error::new(OLE_E_ADVISENOTSUPPORTED, ""))
    }

    fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
        Err(Error::new(OLE_E_ADVISENOTSUPPORTED, ""))
    }
}

#[allow(non_snake_case)]
impl IDataObjectAsyncCapability_Impl for RemoteFileDataObject_Impl {
    fn SetAsyncMode(&self, enabled: BOOL) -> windows::core::Result<()> {
        self.async_mode.store(enabled.as_bool(), Ordering::Release);
        Ok(())
    }

    fn GetAsyncMode(&self) -> windows::core::Result<BOOL> {
        Ok(BOOL::from(self.async_mode.load(Ordering::Acquire)))
    }

    fn StartOperation(&self, _context: Ref<'_, IBindCtx>) -> windows::core::Result<()> {
        self.in_operation.store(true, Ordering::Release);
        Ok(())
    }

    fn InOperation(&self) -> windows::core::Result<BOOL> {
        Ok(BOOL::from(self.in_operation.load(Ordering::Acquire)))
    }

    fn EndOperation(
        &self,
        _result: HRESULT,
        _context: Ref<'_, IBindCtx>,
        _effects: u32,
    ) -> windows::core::Result<()> {
        self.in_operation.store(false, Ordering::Release);
        Ok(())
    }
}

struct RemoteStreamState {
    receiver: Option<RemoteFileStreamReceiver>,
    cache: Option<File>,
    cache_path: Option<PathBuf>,
    progress: Option<RemoteFileProgressCallback>,
    session_id: Option<String>,
    position: u64,
    cached: u64,
    complete: bool,
    reported: bool,
    error: Option<String>,
}

impl Default for RemoteStreamState {
    fn default() -> Self {
        Self {
            receiver: None,
            cache: None,
            cache_path: None,
            progress: None,
            session_id: None,
            position: 0,
            cached: 0,
            complete: false,
            reported: false,
            error: None,
        }
    }
}

#[implement(IStream)]
struct RemoteFileStream {
    app: AppHandle,
    manager: Arc<ClipboardSyncManager>,
    peer_id: String,
    share_id: String,
    relative_path: String,
    name: String,
    size: u64,
    state: Mutex<RemoteStreamState>,
}

impl RemoteFileStream {
    fn new(
        app: AppHandle,
        manager: Arc<ClipboardSyncManager>,
        peer_id: String,
        share_id: String,
        relative_path: String,
        name: String,
        size: u64,
    ) -> Self {
        Self {
            app,
            manager,
            peer_id,
            share_id,
            relative_path,
            name,
            size,
            state: Mutex::new(RemoteStreamState::default()),
        }
    }

    fn ensure_started(&self, state: &mut RemoteStreamState) -> Result<(), String> {
        if state.receiver.is_some() || state.complete {
            return Ok(());
        }
        let cache_directory = std::env::temp_dir()
            .join("ArcRelay")
            .join("remote-file-streams");
        std::fs::create_dir_all(&cache_directory).map_err(|error| error.to_string())?;
        let cache_path = cache_directory.join(format!("{}.part", uuid::Uuid::new_v4()));
        let cache = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&cache_path)
            .map_err(|error| error.to_string())?;
        let receiver = tauri::async_runtime::block_on(self.manager.stream_remote_file(
            &self.peer_id,
            self.share_id.clone(),
            self.relative_path.clone(),
        ))?;
        let directory_path = self
            .relative_path
            .rsplit_once('/')
            .map(|(parent, _)| parent.to_string())
            .unwrap_or_default();
        let session = register_remote_file_transfer(
            &self.app,
            "download",
            self.name.clone(),
            self.peer_id.clone(),
            self.share_id.clone(),
            directory_path,
        );
        state.progress = Some(remote_file_progress_callback(
            self.app.clone(),
            session.id.clone(),
        ));
        state.session_id = Some(session.id);
        state.receiver = Some(receiver);
        state.cache = Some(cache);
        state.cache_path = Some(cache_path);
        Ok(())
    }

    fn receive_next(&self, state: &mut RemoteStreamState) -> Result<(), String> {
        let next = state
            .receiver
            .as_mut()
            .and_then(RemoteFileStreamReceiver::blocking_recv);
        match next {
            Some(Ok(chunk)) => {
                if let Some(cache) = state.cache.as_mut() {
                    cache
                        .seek(SeekFrom::End(0))
                        .and_then(|_| cache.write_all(&chunk))
                        .map_err(|error| error.to_string())?;
                }
                state.cached = state.cached.saturating_add(chunk.len() as u64);
                if let Some(progress) = &state.progress {
                    progress(RemoteFileTransferProgress {
                        bytes_transferred: state.cached,
                        total_bytes: self.size,
                        files_transferred: usize::from(state.cached == self.size),
                        total_files: 1,
                        current_name: self.name.clone(),
                    });
                }
                Ok(())
            }
            Some(Err(error)) => {
                state.error = Some(error.clone());
                state.complete = true;
                self.report_completion(state, Err(error));
                Err(state.error.clone().unwrap_or_default())
            }
            None => {
                state.complete = true;
                if state.cached == self.size {
                    self.report_completion(state, Ok(()));
                    Ok(())
                } else {
                    let error = format!(
                        "download ended early: expected {} bytes, received {} bytes",
                        self.size, state.cached
                    );
                    state.error = Some(error.clone());
                    self.report_completion(state, Err(error.clone()));
                    Err(error)
                }
            }
        }
    }

    fn report_completion(&self, state: &mut RemoteStreamState, result: Result<(), String>) {
        if state.reported {
            return;
        }
        state.reported = true;
        if let Some(session_id) = &state.session_id {
            finish_remote_file_transfer(&self.app, session_id, result);
        }
    }

    fn read_into(&self, output: &mut [u8]) -> Result<usize, String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if output.is_empty() || state.position >= self.size {
            return Ok(0);
        }
        self.ensure_started(&mut state)?;
        let target = state
            .position
            .saturating_add(output.len() as u64)
            .min(self.size);
        while state.cached < target && !state.complete {
            self.receive_next(&mut state)?;
        }
        if let Some(error) = &state.error {
            return Err(error.clone());
        }
        let available = state.cached.saturating_sub(state.position);
        let count = available.min(output.len() as u64) as usize;
        let position = state.position;
        let cache = state
            .cache
            .as_mut()
            .ok_or_else(|| "remote file cache is not ready".to_string())?;
        cache
            .seek(SeekFrom::Start(position))
            .and_then(|_| cache.read_exact(&mut output[..count]))
            .map_err(|error| error.to_string())?;
        state.position = state.position.saturating_add(count as u64);
        Ok(count)
    }
}

impl Drop for RemoteFileStream {
    fn drop(&mut self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.session_id.is_some() && !state.reported {
            self.report_completion(
                &mut state,
                Err("dragged file transfer was cancelled".into()),
            );
        }
        state.receiver.take();
        state.cache.take();
        if let Some(cache_path) = state.cache_path.take() {
            let _ = std::fs::remove_file(cache_path);
        }
    }
}

#[allow(non_snake_case)]
impl ISequentialStream_Impl for RemoteFileStream_Impl {
    fn Read(&self, output: *mut std::ffi::c_void, length: u32, read: *mut u32) -> HRESULT {
        if !read.is_null() {
            unsafe { read.write(0) };
        }
        if length > 0 && output.is_null() {
            return STG_E_READFAULT;
        }
        if length == 0 {
            return S_OK;
        }
        let buffer =
            unsafe { std::slice::from_raw_parts_mut(output.cast::<u8>(), length as usize) };
        match self.read_into(buffer) {
            Ok(count) => {
                if !read.is_null() {
                    unsafe { read.write(count as u32) };
                }
                if count == length as usize {
                    S_OK
                } else {
                    S_FALSE
                }
            }
            Err(error) => {
                tracing::warn!(%error, "Windows virtual file stream read failed");
                STG_E_READFAULT
            }
        }
    }

    fn Write(&self, _input: *const std::ffi::c_void, _length: u32, written: *mut u32) -> HRESULT {
        if !written.is_null() {
            unsafe { written.write(0) };
        }
        STG_E_ACCESSDENIED
    }
}

#[allow(non_snake_case)]
impl IStream_Impl for RemoteFileStream_Impl {
    fn Seek(
        &self,
        offset: i64,
        origin: STREAM_SEEK,
        new_position: *mut u64,
    ) -> windows::core::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let base = match origin {
            STREAM_SEEK_SET => 0_i128,
            STREAM_SEEK_CUR => state.position as i128,
            STREAM_SEEK_END => self.size as i128,
            _ => return Err(Error::new(STG_E_INVALIDFUNCTION, "")),
        };
        let position = base
            .checked_add(offset as i128)
            .filter(|position| *position >= 0 && *position <= u64::MAX as i128)
            .ok_or_else(|| Error::new(STG_E_INVALIDFUNCTION, ""))? as u64;
        state.position = position;
        if !new_position.is_null() {
            unsafe { new_position.write(position) };
        }
        Ok(())
    }

    fn SetSize(&self, _size: u64) -> windows::core::Result<()> {
        Err(Error::new(STG_E_ACCESSDENIED, ""))
    }

    fn CopyTo(
        &self,
        _stream: Ref<'_, IStream>,
        _length: u64,
        _read: *mut u64,
        _written: *mut u64,
    ) -> windows::core::Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn Commit(&self, _flags: &STGC) -> windows::core::Result<()> {
        Ok(())
    }

    fn Revert(&self) -> windows::core::Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn LockRegion(
        &self,
        _offset: u64,
        _length: u64,
        _lock_type: &LOCKTYPE,
    ) -> windows::core::Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn UnlockRegion(
        &self,
        _offset: u64,
        _length: u64,
        _lock_type: u32,
    ) -> windows::core::Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn Stat(&self, stat: *mut STATSTG, _flags: &STATFLAG) -> windows::core::Result<()> {
        let Some(stat) = (unsafe { stat.as_mut() }) else {
            return Err(Error::new(STG_E_INVALIDFUNCTION, ""));
        };
        *stat = STATSTG {
            r#type: STGTY_STREAM.0 as u32,
            cbSize: self.size,
            ..Default::default()
        };
        Ok(())
    }

    fn Clone(&self) -> windows::core::Result<IStream> {
        Err(Error::new(E_NOTIMPL, ""))
    }
}
