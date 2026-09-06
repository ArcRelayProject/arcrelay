use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use arcrelay_transfer::{NearbyPeer, TransferSnapshot, TransferStatus};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const INBOX_VERSION: u32 = 1;
const MAX_SHARE_FILES: usize = 256;
const MAX_SHARE_BYTES: u64 = 16 * 1024 * 1024 * 1024 * 1024;
const MAX_REQUEST_BYTES: u64 = 1024 * 1024;
const REQUEST_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1000;
pub const OPEN_SYSTEM_SHARE_EVENT: &str = "open-system-share-request";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum SystemShareSource {
    CommandLine,
    Tray,
    WindowsContextMenu,
    WindowsShareTarget,
    MacosShareExtension,
    LinuxFileManager,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemShareItem {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemShareRequest {
    pub id: String,
    pub source: SystemShareSource,
    pub created_at_ms: i64,
    pub target_peer_id: Option<String>,
    pub items: Vec<SystemShareItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredRequest {
    #[serde(flatten)]
    view: SystemShareRequest,
    #[serde(default)]
    owned_paths: Vec<PathBuf>,
    #[serde(default)]
    response_path: Option<PathBuf>,
    #[serde(default)]
    transfer_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredInbox {
    version: u32,
    requests: Vec<StoredRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeShareRequest {
    version: u32,
    id: String,
    source: SystemShareSource,
    created_at_ms: i64,
    target_peer_id: Option<String>,
    files: Vec<NativeShareFile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeShareFile {
    path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SharePeerCache {
    version: u32,
    updated_at_ms: i64,
    peers: Vec<SharePeer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SharePeer {
    id: String,
    name: String,
    platform: String,
    model: String,
    paired: bool,
    last_seen_at_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeShareResponse<'a> {
    version: u32,
    request_id: &'a str,
    status: &'a str,
    peer_id: Option<&'a str>,
    peer_name: Option<&'a str>,
    message: Option<&'a str>,
}

pub struct SystemShareService {
    root: PathBuf,
    inbox_path: PathBuf,
    backup_path: PathBuf,
    external_roots: Vec<PathBuf>,
    inbox: Mutex<StoredInbox>,
    last_peer_cache: Mutex<Option<Vec<SharePeer>>>,
}

impl SystemShareService {
    pub fn for_user() -> io::Result<Arc<Self>> {
        let config = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ArcRelay");
        Self::new(config.join("system-share"), platform_external_roots())
    }

    fn new(root: PathBuf, mut external_roots: Vec<PathBuf>) -> io::Result<Arc<Self>> {
        fs::create_dir_all(root.join("external"))?;
        fs::create_dir_all(root.join("staging"))?;
        external_roots.insert(0, root.join("external"));
        external_roots.sort();
        external_roots.dedup();
        let inbox_path = root.join("inbox.json");
        let backup_path = root.join("inbox.backup.json");
        let inbox = load_inbox(&inbox_path, &backup_path)?;
        let service = Arc::new(Self {
            root,
            inbox_path,
            backup_path,
            external_roots,
            inbox: Mutex::new(inbox),
            last_peer_cache: Mutex::new(None),
        });
        Ok(service)
    }

    pub fn initialize(&self) -> io::Result<usize> {
        let imported = self.scan_external_requests()?;
        self.prune_expired()?;
        Ok(imported)
    }

    pub fn handle_activation_args<I>(&self, args: I) -> io::Result<usize>
    where
        I: IntoIterator<Item = OsString>,
    {
        let args: Vec<String> = args
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        let mut source = SystemShareSource::CommandLine;
        let mut target_peer_id = None;
        let mut request_files = Vec::new();
        let mut shared_paths = Vec::new();
        let mut index = usize::from(!args.is_empty());
        while index < args.len() {
            match args[index].as_str() {
                "--arcrelay-share-source" if index + 1 < args.len() => {
                    source = parse_source(&args[index + 1]).unwrap_or(source);
                    index += 2;
                }
                "--arcrelay-share-target" if index + 1 < args.len() => {
                    target_peer_id = validate_target(Some(args[index + 1].clone()))?;
                    index += 2;
                }
                "--arcrelay-share-request" if index + 1 < args.len() => {
                    request_files.push(PathBuf::from(&args[index + 1]));
                    index += 2;
                }
                "--arcrelay-share" => {
                    shared_paths.extend(args[index + 1..].iter().map(PathBuf::from));
                    break;
                }
                _ => index += 1,
            }
        }
        let mut added = 0;
        for request_file in request_files {
            if self.import_request_file(&request_file)? {
                added += 1;
            }
        }
        if !shared_paths.is_empty() {
            self.enqueue_paths(source, target_peer_id, shared_paths)?;
            added += 1;
        }
        Ok(added)
    }

    pub fn pending(&self) -> Vec<SystemShareRequest> {
        self.inbox()
            .requests
            .iter()
            .filter(|request| request.transfer_id.is_none())
            .map(|request| request.view.clone())
            .collect()
    }

    pub fn validate_pending_requests(&self, ids: &[String], paths: &[PathBuf]) -> io::Result<()> {
        if ids.is_empty() || ids.len() > MAX_SHARE_FILES {
            return Err(invalid_data("system share request identifiers are invalid"));
        }
        let ids: HashSet<_> = ids.iter().collect();
        if ids.len() > MAX_SHARE_FILES {
            return Err(invalid_data("system share request identifiers are invalid"));
        }
        let paths: HashSet<_> = paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        let inbox = self.inbox();
        for id in ids {
            let request = inbox
                .requests
                .iter()
                .find(|request| &request.view.id == id && request.transfer_id.is_none())
                .ok_or_else(|| {
                    invalid_data("system share request is unavailable or already submitted")
                })?;
            if !request
                .view
                .items
                .iter()
                .any(|item| paths.contains(&item.path))
            {
                return Err(invalid_data(
                    "transfer does not contain files from the system share request",
                ));
            }
        }
        Ok(())
    }

    pub fn enqueue_paths(
        &self,
        source: SystemShareSource,
        target_peer_id: Option<String>,
        paths: Vec<PathBuf>,
    ) -> io::Result<SystemShareRequest> {
        let view = SystemShareRequest {
            id: uuid::Uuid::new_v4().to_string(),
            source,
            created_at_ms: now_ms(),
            target_peer_id: validate_target(target_peer_id)?,
            items: inspect_share_paths(paths)?,
        };
        let mut inbox = self.inbox();
        inbox.requests.push(StoredRequest {
            view: view.clone(),
            owned_paths: Vec::new(),
            response_path: None,
            transfer_id: None,
        });
        self.persist_locked(&inbox)?;
        Ok(view)
    }

    pub fn submit(&self, id: &str, transfer_id: &str, peer_id: &str) -> io::Result<()> {
        self.submit_many(&[id.to_owned()], transfer_id, peer_id)
    }

    pub fn submit_many(&self, ids: &[String], transfer_id: &str, peer_id: &str) -> io::Result<()> {
        if transfer_id.trim().is_empty() || peer_id.trim().is_empty() {
            return Err(invalid_data("transfer and peer identifiers are required"));
        }
        let ids: HashSet<_> = ids.iter().map(String::as_str).collect();
        if ids.is_empty() || ids.len() > MAX_SHARE_FILES {
            return Err(invalid_data("system share request identifiers are invalid"));
        }
        let mut inbox = self.inbox();
        for id in &ids {
            let request = inbox
                .requests
                .iter()
                .find(|request| request.view.id == *id)
                .ok_or_else(|| invalid_data("system share request is unavailable"))?;
            if let Some(existing) = request.transfer_id.as_deref() {
                if existing != transfer_id
                    || request.view.target_peer_id.as_deref() != Some(peer_id)
                {
                    return Err(invalid_data(
                        "system share request is already linked to another transfer",
                    ));
                }
            }
        }
        let mut responses = Vec::new();
        for request in inbox
            .requests
            .iter_mut()
            .filter(|request| ids.contains(request.view.id.as_str()))
        {
            request.transfer_id = Some(transfer_id.to_owned());
            request.view.target_peer_id = Some(peer_id.to_owned());
            responses.push((request.view.id.clone(), request.response_path.clone()));
        }
        self.persist_locked(&inbox)?;
        drop(inbox);
        let peer_name = self
            .last_peer_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_deref()
            .unwrap_or_default()
            .iter()
            .find(|peer| peer.id == peer_id)
            .map(|peer| peer.name.clone());
        for (id, response_path) in responses {
            if let Some(path) = response_path {
                write_response(
                    &path,
                    NativeShareResponse {
                        version: INBOX_VERSION,
                        request_id: &id,
                        status: "submitted",
                        peer_id: Some(peer_id),
                        peer_name: peer_name.as_deref(),
                        message: None,
                    },
                )?;
            }
        }
        Ok(())
    }

    pub fn discard(&self, id: &str) -> io::Result<()> {
        let mut inbox = self.inbox();
        let index = inbox
            .requests
            .iter()
            .position(|request| request.view.id == id && request.transfer_id.is_none())
            .ok_or_else(|| {
                invalid_data("system share request is unavailable or already submitted")
            })?;
        let request = inbox.requests.remove(index);
        self.persist_locked(&inbox)?;
        drop(inbox);
        self.cleanup_owned(&request);
        Ok(())
    }

    pub fn observe_transfers(&self, snapshot: &TransferSnapshot) -> io::Result<bool> {
        let mut inbox = self.inbox();
        let mut completed = Vec::new();
        let mut requeued = false;
        let mut changed = false;
        for request in &mut inbox.requests {
            let Some(transfer_id) = request.transfer_id.as_deref() else {
                continue;
            };
            let Some(transfer) = snapshot
                .transfers
                .iter()
                .find(|item| item.id == transfer_id)
            else {
                continue;
            };
            match transfer.status {
                TransferStatus::Completed => completed.push(request.view.id.clone()),
                TransferStatus::Failed | TransferStatus::Rejected | TransferStatus::Cancelled => {
                    if let Some(path) = &request.response_path {
                        let _ = write_response(
                            path,
                            NativeShareResponse {
                                version: INBOX_VERSION,
                                request_id: &request.view.id,
                                status: "failed",
                                peer_id: request.view.target_peer_id.as_deref(),
                                peer_name: None,
                                message: transfer.error_message.as_deref(),
                            },
                        );
                    }
                    request.transfer_id = None;
                    requeued = true;
                    changed = true;
                }
                _ => {}
            }
        }
        let completed_ids: HashSet<_> = completed.iter().cloned().collect();
        let removed: Vec<_> = inbox
            .requests
            .iter()
            .filter(|request| completed_ids.contains(&request.view.id))
            .cloned()
            .collect();
        if !removed.is_empty() {
            inbox
                .requests
                .retain(|request| !completed_ids.contains(&request.view.id));
            changed = true;
        }
        if changed {
            self.persist_locked(&inbox)?;
        }
        drop(inbox);
        for request in removed {
            if let Some(path) = &request.response_path {
                let _ = write_response(
                    path,
                    NativeShareResponse {
                        version: INBOX_VERSION,
                        request_id: &request.view.id,
                        status: "completed",
                        peer_id: request.view.target_peer_id.as_deref(),
                        peer_name: None,
                        message: None,
                    },
                );
            }
            self.cleanup_owned(&request);
        }
        Ok(requeued)
    }

    pub fn update_peer_cache(&self, peers: &[NearbyPeer]) -> io::Result<()> {
        let mut next: Vec<_> = peers
            .iter()
            .map(|peer| SharePeer {
                id: peer.id.clone(),
                name: peer.name.clone(),
                platform: peer.platform.clone(),
                model: peer.model.clone(),
                paired: peer.paired,
                last_seen_at_ms: peer.last_seen_at_ms,
            })
            .collect();
        next.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
        let mut previous = self
            .last_peer_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if previous.as_ref() == Some(&next) {
            return Ok(());
        }
        let cache = SharePeerCache {
            version: INBOX_VERSION,
            updated_at_ms: now_ms(),
            peers: next.clone(),
        };
        atomic_json_write(&self.root.join("peers.json"), &cache)?;
        for root in &self.external_roots {
            if let Err(error) = fs::create_dir_all(root)
                .and_then(|_| atomic_json_write(&root.join("peers.json"), &cache))
            {
                tracing::debug!(%error, path = %root.display(), "could not publish native share peer cache");
            }
        }
        *previous = Some(next);
        Ok(())
    }

    pub fn scan_external_requests(&self) -> io::Result<usize> {
        let mut request_files = Vec::new();
        for root in &self.external_roots {
            collect_request_files(root, 3, &mut request_files)?;
        }
        let mut imported = 0;
        for path in request_files {
            match self.import_request_file(&path) {
                Ok(true) => imported += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, path = %path.display(), "ignored invalid native share request");
                    let invalid = path.with_file_name("request.invalid.json");
                    if let Err(rename_error) = fs::rename(&path, &invalid) {
                        tracing::debug!(%rename_error, path = %path.display(), "could not quarantine invalid native share request");
                    }
                }
            }
        }
        Ok(imported)
    }

    fn import_request_file(&self, path: &Path) -> io::Result<bool> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_REQUEST_BYTES
        {
            return Err(invalid_data("native share request file is invalid"));
        }
        let canonical_request = path.canonicalize()?;
        if !self.is_external_path(&canonical_request) {
            return Err(invalid_data(
                "native share request is outside the handoff directory",
            ));
        }
        let native: NativeShareRequest =
            serde_json::from_slice(&fs::read(&canonical_request)?).map_err(invalid_json)?;
        if native.version != INBOX_VERSION || uuid::Uuid::parse_str(&native.id).is_err() {
            return Err(invalid_data(
                "native share request version or identifier is invalid",
            ));
        }
        let target_peer_id = validate_target(native.target_peer_id)?;
        let paths: Vec<_> = native.files.into_iter().map(|file| file.path).collect();
        let items = inspect_share_paths(paths)?;
        let owned_paths: Vec<_> = items
            .iter()
            .map(|item| PathBuf::from(&item.path))
            .filter(|owned| self.is_external_path(owned))
            .collect();
        if owned_paths.len() != items.len() {
            return Err(invalid_data(
                "native share payload is outside the handoff directory",
            ));
        }
        let response_path = canonical_request
            .parent()
            .map(|parent| parent.join("response.json"));
        let mut inbox = self.inbox();
        let exists = inbox
            .requests
            .iter()
            .any(|request| request.view.id == native.id);
        if !exists {
            inbox.requests.push(StoredRequest {
                view: SystemShareRequest {
                    id: native.id,
                    source: native.source,
                    created_at_ms: native.created_at_ms.clamp(1, now_ms()),
                    target_peer_id,
                    items,
                },
                owned_paths,
                response_path,
                transfer_id: None,
            });
            self.persist_locked(&inbox)?;
        }
        drop(inbox);
        fs::remove_file(&canonical_request)?;
        Ok(!exists)
    }

    fn prune_expired(&self) -> io::Result<()> {
        let cutoff = now_ms().saturating_sub(REQUEST_RETENTION_MS);
        let mut inbox = self.inbox();
        let expired: Vec<_> = inbox
            .requests
            .iter()
            .filter(|request| request.view.created_at_ms < cutoff)
            .cloned()
            .collect();
        if expired.is_empty() {
            return Ok(());
        }
        let ids: HashSet<_> = expired
            .iter()
            .map(|request| request.view.id.as_str())
            .collect();
        inbox
            .requests
            .retain(|request| !ids.contains(request.view.id.as_str()));
        self.persist_locked(&inbox)?;
        drop(inbox);
        for request in expired {
            self.cleanup_owned(&request);
        }
        Ok(())
    }

    fn cleanup_owned(&self, request: &StoredRequest) {
        for path in &request.owned_paths {
            if self.is_external_path(path) {
                if let Err(error) = fs::remove_file(path) {
                    if error.kind() != io::ErrorKind::NotFound {
                        tracing::warn!(%error, path = %path.display(), "could not remove system share staged file");
                    }
                }
            }
        }
    }

    fn is_external_path(&self, path: &Path) -> bool {
        self.external_roots
            .iter()
            .any(|root| path.starts_with(root))
    }

    fn persist_locked(&self, inbox: &StoredInbox) -> io::Result<()> {
        durable_json_write(&self.inbox_path, &self.backup_path, inbox)
    }

    fn inbox(&self) -> MutexGuard<'_, StoredInbox> {
        self.inbox
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn announce_pending(app: &AppHandle, service: &SystemShareService) {
    let pending = service.pending();
    if pending.is_empty() {
        return;
    }
    crate::windowing::request_main_window(app.clone());
    if let Some(window) = app.get_webview_window("main") {
        for request in pending {
            let _ = window.emit(OPEN_SYSTEM_SHARE_EVENT, request);
        }
    }
}

pub fn start_external_monitor(app: AppHandle, service: Arc<SystemShareService>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(750));
        loop {
            interval.tick().await;
            let scanner = service.clone();
            let imported = tokio::task::spawn_blocking(move || scanner.scan_external_requests())
                .await
                .unwrap_or_else(|error| Err(io::Error::other(error)));
            match imported {
                Ok(count) if count > 0 => announce_pending(&app, &service),
                Ok(_) => {}
                Err(error) => tracing::debug!(%error, "native share handoff scan failed"),
            }
        }
    });
}

#[cfg(target_os = "linux")]
pub fn install_linux_user_integrations() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let quoted = executable.to_string_lossy().replace('"', "\\\"");
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".local/share")))
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "user data directory unavailable")
        })?;
    let files = [
        (
            data_home.join("applications/arcrelay-share.desktop"),
            include_str!("../integrations/linux/arcrelay-share.desktop")
                .replace("@ARCRELAY_EXE@", &quoted),
        ),
        (
            data_home.join("kio/servicemenus/arcrelay-share.desktop"),
            include_str!("../integrations/linux/arcrelay-dolphin.desktop")
                .replace("@ARCRELAY_EXE@", &quoted),
        ),
        (
            data_home.join("nemo/actions/arcrelay-share.nemo_action"),
            include_str!("../integrations/linux/arcrelay-share.nemo_action")
                .replace("@ARCRELAY_EXE@", &quoted),
        ),
        (
            data_home.join("nautilus-python/extensions/arcrelay_share.py"),
            include_str!("../integrations/linux/arcrelay_share.py").replace(
                "@ARCRELAY_EXE_JSON@",
                &serde_json::to_string(&executable.to_string_lossy()).unwrap(),
            ),
        ),
    ];
    for (path, contents) in files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::read_to_string(&path).ok().as_deref() != Some(contents.as_str()) {
            fs::write(path, contents)?;
        }
    }
    Ok(())
}

fn inspect_share_paths(paths: Vec<PathBuf>) -> io::Result<Vec<SystemShareItem>> {
    if paths.is_empty() || paths.len() > MAX_SHARE_FILES {
        return Err(invalid_data(
            "a share request must contain between 1 and 256 files",
        ));
    }
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        let link_metadata = fs::symlink_metadata(&path)?;
        if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
            return Err(invalid_data("system share accepts regular files only"));
        }
        let canonical = path.canonicalize()?;
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let metadata = fs::metadata(&canonical)?;
        total = total
            .checked_add(metadata.len())
            .ok_or_else(|| invalid_data("shared file size overflow"))?;
        if total > MAX_SHARE_BYTES {
            return Err(invalid_data("shared files exceed the transfer size limit"));
        }
        let name = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid_data("shared file name is invalid"))?
            .to_owned();
        items.push(SystemShareItem {
            path: canonical.to_string_lossy().into_owned(),
            name,
            size: metadata.len(),
            modified_at_ms: metadata.modified().ok().and_then(system_time_ms),
        });
    }
    if items.is_empty() {
        return Err(invalid_data("share request contains no unique files"));
    }
    Ok(items)
}

fn validate_target(target: Option<String>) -> io::Result<Option<String>> {
    target
        .map(|target| {
            let target = target.trim();
            if target.is_empty() || target.len() > 512 || target.chars().any(char::is_control) {
                Err(invalid_data("target device identifier is invalid"))
            } else {
                Ok(target.to_owned())
            }
        })
        .transpose()
}

fn parse_source(value: &str) -> Option<SystemShareSource> {
    match value {
        "command-line" => Some(SystemShareSource::CommandLine),
        "windows-context-menu" => Some(SystemShareSource::WindowsContextMenu),
        "windows-share-target" => Some(SystemShareSource::WindowsShareTarget),
        "macos-share-extension" => Some(SystemShareSource::MacosShareExtension),
        "linux-file-manager" => Some(SystemShareSource::LinuxFileManager),
        _ => None,
    }
}

fn platform_external_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .map(|home| home.join("Library/Group Containers/group.com.arcrelay.shared/ShareInbox"))
            .into_iter()
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

fn collect_request_files(root: &Path, depth: usize, output: &mut Vec<PathBuf>) -> io::Result<()> {
    if depth == 0 || !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        if metadata.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_request_files(&entry.path(), depth - 1, output)?;
        } else if metadata.is_file() && entry.file_name() == "request.json" {
            output.push(entry.path());
        }
    }
    Ok(())
}

fn load_inbox(path: &Path, backup: &Path) -> io::Result<StoredInbox> {
    let mut first_error = None;
    for candidate in [path, backup] {
        match fs::read(candidate) {
            Ok(bytes) => {
                let inbox: StoredInbox = match serde_json::from_slice(&bytes) {
                    Ok(inbox) => inbox,
                    Err(error) => {
                        first_error.get_or_insert_with(|| invalid_json(error));
                        continue;
                    }
                };
                if inbox.version != INBOX_VERSION {
                    first_error.get_or_insert_with(|| {
                        invalid_data("unsupported system share inbox version")
                    });
                    continue;
                }
                return Ok(inbox);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                first_error.get_or_insert(error);
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(StoredInbox {
        version: INBOX_VERSION,
        requests: Vec::new(),
    })
}

fn durable_json_write(path: &Path, backup: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_data("persistence path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".inbox-{}.tmp", uuid::Uuid::new_v4()));
    let bytes = serde_json::to_vec_pretty(value).map_err(invalid_json)?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let had_original = path.exists();
    if had_original {
        let _ = fs::remove_file(backup);
        fs::rename(path, backup)?;
    }
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            if had_original {
                let _ = fs::rename(backup, path);
            }
            let _ = fs::remove_file(temporary);
            Err(error)
        }
    }
}

fn atomic_json_write(path: &Path, value: &impl Serialize) -> io::Result<()> {
    durable_json_write(path, &path.with_extension("backup.json"), value)
}

fn write_response(path: &Path, response: NativeShareResponse<'_>) -> io::Result<()> {
    atomic_json_write(path, &response)
}

fn now_ms() -> i64 {
    system_time_ms(SystemTime::now()).unwrap_or(0)
}

fn system_time_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn invalid_json(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_requests_are_durable_and_deduplicated() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("one.txt");
        fs::write(&file, b"one").unwrap();
        let root = directory.path().join("system-share");
        let service = SystemShareService::new(root.clone(), vec![]).unwrap();
        service
            .handle_activation_args([
                OsString::from("ArcRelay"),
                OsString::from("--arcrelay-share"),
                file.clone().into_os_string(),
                file.clone().into_os_string(),
            ])
            .unwrap();
        assert_eq!(service.pending().len(), 1);
        assert_eq!(service.pending()[0].items.len(), 1);
        drop(service);

        let restarted = SystemShareService::new(root, vec![]).unwrap();
        assert_eq!(restarted.pending().len(), 1);
        assert_eq!(restarted.pending()[0].items[0].name, "one.txt");
    }

    #[test]
    fn native_handoff_must_keep_owned_files_inside_the_external_root() {
        let directory = tempfile::tempdir().unwrap();
        let external = directory.path().join("external");
        let request_dir = external.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&request_dir).unwrap();
        let outside = directory.path().join("outside.txt");
        fs::write(&outside, b"no").unwrap();
        fs::write(
            request_dir.join("request.json"),
            serde_json::json!({
                "version": 1,
                "id": uuid::Uuid::new_v4().to_string(),
                "source": "windowsShareTarget",
                "createdAtMs": now_ms(),
                "targetPeerId": null,
                "files": [{"path": outside}],
            })
            .to_string(),
        )
        .unwrap();
        let service =
            SystemShareService::new(directory.path().join("state"), vec![external]).unwrap();
        service.scan_external_requests().unwrap();
        assert!(service.pending().is_empty());
    }

    #[test]
    fn durable_inbox_recovers_from_the_previous_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("system-share");
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let service = SystemShareService::new(root.clone(), vec![]).unwrap();
        service
            .enqueue_paths(SystemShareSource::CommandLine, None, vec![first])
            .unwrap();
        service
            .enqueue_paths(SystemShareSource::CommandLine, None, vec![second])
            .unwrap();
        drop(service);
        fs::write(root.join("inbox.json"), b"not json").unwrap();

        let recovered = SystemShareService::new(root, vec![]).unwrap();
        assert_eq!(recovered.pending().len(), 1);
        assert_eq!(recovered.pending()[0].items[0].name, "first.txt");
    }

    #[test]
    fn batch_submission_validates_every_request_before_persisting() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.txt");
        let second = directory.path().join("second.txt");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let service =
            SystemShareService::new(directory.path().join("system-share"), vec![]).unwrap();
        let first = service
            .enqueue_paths(SystemShareSource::CommandLine, None, vec![first])
            .unwrap();
        let second = service
            .enqueue_paths(SystemShareSource::CommandLine, None, vec![second])
            .unwrap();

        assert!(service
            .submit_many(&[first.id.clone(), "missing".into()], "transfer", "peer")
            .is_err());
        assert_eq!(service.pending().len(), 2);
        service
            .submit_many(&[first.id, second.id], "transfer", "peer")
            .unwrap();
        assert!(service.pending().is_empty());
    }
}
