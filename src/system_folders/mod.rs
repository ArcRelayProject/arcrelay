//! Native cloud-folder coordination. QUIC and trust stay in the existing network
//! stack; platform adapters only translate filesystem requests and local copies.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_variables))]
mod index;
#[cfg(target_os = "macos")]
mod macos;
mod server;
mod store;
#[cfg(any(target_os = "windows", test))]
mod webdav;
#[cfg(target_os = "windows")]
mod windows;

use crate::clipboard_sync::ClipboardSyncManager;
use arcrelay_protocol::remote_files::{
    RemoteFileErrorCode, RemoteFileKind, RemoteFileRequest as Request, RemoteFileResponse,
};
use index::{Index, Item};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SystemFolder {
    pub id: String,
    pub peer_id: String,
    pub share_id: String,
    pub name: String,
    pub online: bool,
    #[serde(default)]
    pub registered: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub recovery_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "unavailable".into(),
            message: message.into(),
        }
    }
    fn missing(message: impl Into<String>) -> Self {
        Self {
            code: "notFound".into(),
            message: message.into(),
        }
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self {
            code: "conflict".into(),
            message: message.into(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::unavailable(e.to_string())
    }
}
impl From<arcrelay_protocol::remote_files::RemoteFileError> for Error {
    fn from(e: arcrelay_protocol::remote_files::RemoteFileError) -> Self {
        let code = match e.code {
            RemoteFileErrorCode::NotFound => "notFound",
            RemoteFileErrorCode::Conflict => "conflict",
            RemoteFileErrorCode::PermissionDenied => "forbidden",
            RemoteFileErrorCode::ResourceExhausted => "quota",
            RemoteFileErrorCode::InvalidArgument => "invalid",
            RemoteFileErrorCode::FailedPrecondition => "unsupported",
            _ => "unavailable",
        };
        Self {
            code: code.into(),
            message: e.message,
        }
    }
}

pub(super) struct Domain {
    view: Mutex<SystemFolder>,
    index: Mutex<Index>,
    operations: Mutex<()>,
    persistence: Mutex<()>,
    store: store::Store,
}
impl Domain {
    async fn persist(&self) -> Result<(), Error> {
        let _guard = self.persistence.lock().await;
        let delta = self.store.delta(&*self.index.lock().await);
        if let Some((sequence, changes, enumerated, snapshot)) = delta {
            self.store
                .save(sequence, changes, enumerated, snapshot)
                .await?;
        }
        Ok(())
    }
}

pub struct SystemFolders {
    manager: Arc<ClipboardSyncManager>,
    root: PathBuf,
    domains: Mutex<HashMap<String, Arc<Domain>>>,
    registration: Mutex<()>,
    active_recoveries: std::sync::Mutex<HashSet<PathBuf>>,
    stopped: tokio_util::sync::CancellationToken,
}

/// Synchronous desktop construction must not create timers, tasks or sockets.
pub fn empty_slot() -> Arc<tokio::sync::OnceCell<Arc<SystemFolders>>> {
    Arc::new(tokio::sync::OnceCell::new())
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.into()
    } else {
        format!("{parent}/{name}")
    }
}
fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.starts_with(".arcrelay-upload-")
        || name.len() > 255
        || name.contains(['/', '\\'])
        || name.chars().any(char::is_control)
    {
        return Err(Error {
            code: "invalid".into(),
            message: "invalid filename".into(),
        });
    }
    Ok(())
}

// Staging copies become actionable after the upload ends; interrupted copies
// from a previous process are discoverable after restarting.
async fn pending_recoveries(
    root: &Path,
    active: &std::sync::Mutex<HashSet<PathBuf>>,
) -> HashMap<String, u32> {
    let mut counts = HashMap::new();
    let Ok(mut entries) = tokio::fs::read_dir(root).await else {
        return counts;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Ok(bytes) = tokio::fs::read(path.join("save.json")).await else {
            continue;
        };
        if active.lock().unwrap().contains(&path) {
            continue;
        }
        let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            continue;
        };
        if !matches!(
            manifest["phase"].as_str(),
            Some("incomplete" | "complete-pending-remote-acknowledgement")
        ) {
            continue;
        }
        if let Some(domain) = manifest["request"]["domain"].as_str() {
            *counts.entry(domain.to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

impl SystemFolders {
    pub async fn start(
        manager: Arc<ClipboardSyncManager>,
        root: PathBuf,
    ) -> Result<Arc<Self>, Error> {
        tokio::fs::create_dir_all(&root).await?;
        let service = Arc::new(Self {
            manager,
            root,
            domains: Mutex::new(HashMap::new()),
            registration: Mutex::new(()),
            active_recoveries: std::sync::Mutex::new(HashSet::new()),
            stopped: tokio_util::sync::CancellationToken::new(),
        });
        let saved = service.root.join("folders.json");
        let views: Vec<SystemFolder> = match tokio::fs::read(saved).await {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| Error::unavailable(e.to_string()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        for mut view in views {
            uuid::Uuid::parse_str(&view.id)
                .map_err(|_| Error::unavailable("invalid saved folder identity"))?;
            let legacy = service.root.join(format!("{}.json", view.id));
            let fallback = match tokio::fs::read(&legacy).await {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map_err(|error| Error::unavailable(error.to_string()))?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Index::root(&view.name, false)
                }
                Err(error) => return Err(error.into()),
            };
            let (store, index) =
                store::Store::open(&service.root.join(format!("{}.sqlite3", view.id)), fallback)
                    .await?;
            view.online = false;
            service.domains.lock().await.insert(
                view.id.clone(),
                Arc::new(Domain {
                    view: Mutex::new(view),
                    index: Mutex::new(index),
                    operations: Mutex::new(()),
                    persistence: Mutex::new(()),
                    store,
                }),
            );
        }
        #[cfg(target_os = "macos")]
        server::start(service.clone()).await?;
        #[cfg(target_os = "windows")]
        webdav::start(service.clone()).await?;
        // Windows locations and their loopback endpoint are persistent. Do not
        // re-register on startup: a temporarily offline peer must not disconnect
        // an existing Explorer location or delay the desktop runtime.
        let domains: Vec<_> = service.domains.lock().await.values().cloned().collect();
        for domain in domains {
            let service = service.clone();
            tokio::spawn(async move { service.watch_domain(domain).await });
        }
        Ok(service)
    }

    pub async fn list(&self) -> Vec<SystemFolder> {
        let domains: Vec<_> = self.domains.lock().await.values().cloned().collect();
        let mut views = Vec::new();
        for domain in domains {
            views.push(domain.view.lock().await.clone());
        }
        let recoveries =
            pending_recoveries(&self.root.join("recovery"), &self.active_recoveries).await;
        for view in &mut views {
            view.recovery_count = recoveries.get(&view.id).copied().unwrap_or(0);
        }
        views.sort_by(|a, b| a.name.cmp(&b.name));
        views
    }

    async fn persist_folders(&self) -> Result<(), Error> {
        let bytes = serde_json::to_vec(&self.list().await)
            .map_err(|e| Error::unavailable(e.to_string()))?;
        let path = self.root.join("folders.json");
        tokio::task::spawn_blocking(move || {
            crate::infrastructure::durable_file::replace_private(&path, &bytes)
        })
        .await
        .map_err(|e| Error::unavailable(e.to_string()))??;
        Ok(())
    }

    pub async fn add(self: &Arc<Self>, peer: String, share: String) -> Result<SystemFolder, Error> {
        let _guard = self.registration.lock().await;
        if let Some(mut view) = self
            .list()
            .await
            .into_iter()
            .find(|v| v.peer_id == peer && v.share_id == share)
        {
            self.register(&view).await?;
            view.registered = true;
            view.error = None;
            *self.domain(&view.id).await?.view.lock().await = view.clone();
            self.persist_folders().await?;
            return Ok(view);
        }
        let shares = self
            .manager
            .remote_file_request(&peer, Request::ListShares)
            .await?;
        let share_info = shares
            .shares
            .into_iter()
            .find(|s| s.id == share)
            .ok_or_else(|| Error::missing("shared folder is unavailable"))?;
        self.manager
            .remote_file_request(
                &peer,
                Request::Stat {
                    share_id: share.clone(),
                    relative_path: String::new(),
                },
            )
            .await?;
        let peer_name = self
            .manager
            .remote_file_devices()
            .await
            .into_iter()
            .find(|d| d.id == peer)
            .map(|d| d.name)
            .unwrap_or_else(|| "Device".into());
        let mut view = SystemFolder {
            id: uuid::Uuid::new_v4().to_string(),
            peer_id: peer,
            share_id: share,
            name: format!("{} · {peer_name}", share_info.name),
            online: true,
            registered: false,
            error: None,
            recovery_count: 0,
        };
        let (store, index) = store::Store::open(
            &self.root.join(format!("{}.sqlite3", view.id)),
            Index::root(&share_info.name, share_info.writable),
        )
        .await?;
        let domain = Arc::new(Domain {
            view: Mutex::new(view.clone()),
            index: Mutex::new(index),
            operations: Mutex::new(()),
            persistence: Mutex::new(()),
            store,
        });
        domain.persist().await?;
        self.domains.lock().await.insert(view.id.clone(), domain);
        let watcher = self.clone();
        let watched_domain = self.domain(&view.id).await?;
        tokio::spawn(async move { watcher.watch_domain(watched_domain).await });
        // Persist before OS registration so a crash cannot orphan a provider.
        self.persist_folders().await?;
        if let Err(error) = self.register(&view).await {
            self.domain(&view.id).await?.view.lock().await.error = Some(error.message.clone());
            self.persist_folders().await?;
            return Err(error);
        }
        view.registered = true;
        *self.domain(&view.id).await?.view.lock().await = view.clone();
        self.persist_folders().await?;
        Ok(view)
    }

    async fn register(self: &Arc<Self>, view: &SystemFolder) -> Result<(), Error> {
        #[cfg(target_os = "macos")]
        return macos::register(view).await;
        #[cfg(target_os = "windows")]
        return windows::register(&self.root, view).await;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = view;
            Err(Error {
                code: "unsupported".into(),
                message: "system folders require macOS 13+ or Windows with WebClient".into(),
            })
        }
    }

    pub async fn open(&self, id: &str) -> Result<(), Error> {
        let view = self.domain(id).await?.view.lock().await.clone();
        #[cfg(target_os = "macos")]
        return macos::open(&view).await;
        #[cfg(target_os = "windows")]
        return windows::open(&self.root, &view).await;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = view;
            Err(Error::unavailable(
                "native cloud folders are unavailable on this platform",
            ))
        }
    }

    pub async fn remove(&self, id: &str) -> Result<(), Error> {
        let _guard = self.registration.lock().await;
        let view = self.domain(id).await?.view.lock().await.clone();
        #[cfg(target_os = "macos")]
        macos::remove(&view).await?;
        #[cfg(target_os = "windows")]
        windows::remove(&view).await?;
        self.domains.lock().await.remove(id);
        self.persist_folders().await
    }

    async fn domain(&self, id: &str) -> Result<Arc<Domain>, Error> {
        self.domains
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| Error::missing("system folder was removed"))
    }

    async fn request(
        &self,
        view: &SystemFolder,
        request: Request,
    ) -> Result<RemoteFileResponse, Error> {
        tokio::time::timeout(
            Duration::from_secs(15),
            self.manager.remote_file_request(&view.peer_id, request),
        )
        .await
        .map_err(|_| Error::unavailable("remote device is not responding"))?
        .map_err(Into::into)
    }

    async fn stat_path(
        &self,
        view: &SystemFolder,
        parent: &str,
        path: &str,
        id: &str,
    ) -> Result<Item, Error> {
        let response = self
            .request(
                view,
                Request::Stat {
                    share_id: view.share_id.clone(),
                    relative_path: path.into(),
                },
            )
            .await?;
        let entry = response
            .entry
            .ok_or_else(|| Error::unavailable("missing file metadata"))?;
        Ok(Item {
            id: if id.is_empty() {
                entry.id.clone()
            } else {
                id.into()
            },
            source_id: entry.id,
            parent_id: parent.into(),
            name: entry.name,
            path: entry.relative_path,
            folder: entry.kind == RemoteFileKind::Folder,
            size: entry.size,
            modified_at_ms: entry.modified_at_ms,
            revision: response.revision,
            writable: true,
        })
    }

    pub(super) async fn stat(&self, domain_id: &str, item_id: &str) -> Result<Item, Error> {
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        let view = domain.view.lock().await.clone();
        let old = domain.index.lock().await.item(item_id)?;
        let mut item = self
            .stat_path(&view, &old.parent_id, &old.path, &old.id)
            .await?;
        item.writable = domain.index.lock().await.item("root")?.writable;
        let item = domain.index.lock().await.upsert(item);
        domain.persist().await?;
        Ok(item)
    }

    pub(super) async fn enumerate(
        &self,
        domain_id: &str,
        parent_id: &str,
    ) -> Result<Vec<Item>, Error> {
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        self.enumerate_locked(&domain, parent_id).await
    }

    async fn enumerate_locked(&self, domain: &Domain, parent_id: &str) -> Result<Vec<Item>, Error> {
        let view = domain.view.lock().await.clone();
        let parent = domain.index.lock().await.item(parent_id)?;
        let mut cursor = None;
        let mut items = Vec::new();
        loop {
            let response = self
                .request(
                    &view,
                    Request::ListDirectory {
                        share_id: view.share_id.clone(),
                        relative_path: parent.path.clone(),
                        cursor,
                        limit: 200,
                        search: None,
                        sort_key: Default::default(),
                        sort_direction: Default::default(),
                    },
                )
                .await?;
            for entry in response.entries {
                if entry.revision.is_empty() || entry.id.is_empty() {
                    return Err(Error::unavailable(
                        "remote listing omitted an item identity or revision",
                    ));
                }
                items.push(Item {
                    id: entry.id.clone(),
                    source_id: entry.id,
                    parent_id: parent_id.into(),
                    name: entry.name,
                    path: entry.relative_path,
                    folder: entry.kind == RemoteFileKind::Folder,
                    size: entry.size,
                    modified_at_ms: entry.modified_at_ms,
                    revision: entry.revision,
                    writable: parent.writable,
                });
                if items.len() > 100_000 {
                    return Err(Error {
                        code: "quota".into(),
                        message: "system folder contains too many items".into(),
                    });
                }
            }
            cursor = response.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        let seen: HashSet<_> = items.iter().map(|item| item.path.clone()).collect();
        let mut index = domain.index.lock().await;
        let removed: Vec<_> = index
            .items
            .values()
            .filter(|item| {
                item.id != "root" && item.parent_id == parent_id && !seen.contains(&item.path)
            })
            .map(|i| i.id.clone())
            .collect();
        for id in removed {
            index.remove(&id);
        }
        let items = items.into_iter().map(|item| index.upsert(item)).collect();
        index.enumerated.insert(parent_id.into());
        drop(index);
        domain.persist().await?;
        Ok(items)
    }

    pub(super) async fn read(
        &self,
        domain_id: &str,
        item: &Item,
        offset: u64,
        length: u32,
    ) -> Result<Vec<u8>, Error> {
        let domain = self.domain(domain_id).await?;
        let view = domain.view.lock().await.clone();
        Ok(self
            .request(
                &view,
                Request::ReadRange {
                    share_id: view.share_id.clone(),
                    relative_path: item.path.clone(),
                    offset,
                    length,
                    revision: item.revision.clone(),
                },
            )
            .await?
            .range_data)
    }

    pub(super) async fn save(
        &self,
        domain_id: &str,
        item_id: Option<&str>,
        parent_id: &str,
        name: &str,
        expected: &str,
        source: &Path,
    ) -> Result<Item, Error> {
        validate_name(name)?;
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        let view = domain.view.lock().await.clone();
        let parent = domain.index.lock().await.item(parent_id)?;
        let target = if let Some(id) = item_id {
            domain.index.lock().await.item(id)?.path
        } else {
            join(&parent.path, name)
        };
        let (remote_parent, remote_name) = target.rsplit_once('/').unwrap_or(("", &target));
        let request = Request::ConditionalUpload {
            share_id: view.share_id.clone(),
            relative_path: remote_parent.into(),
            name: remote_name.into(),
            size: tokio::fs::metadata(source).await?.len(),
            expected_revision: expected.into(),
        };
        let response = tokio::time::timeout(
            Duration::from_secs(300),
            self.manager
                .upload_system_file(&view.peer_id, request, source.into()),
        )
        .await
        .map_err(|_| Error::unavailable("save timed out; the local copy is retained"))??;
        let entry = response
            .entry
            .ok_or_else(|| Error::unavailable("save was not acknowledged"))?;
        let resolved_parent = if let Some(id) = item_id {
            domain.index.lock().await.item(id)?.parent_id
        } else {
            parent_id.into()
        };
        let item = domain.index.lock().await.upsert(Item {
            id: item_id
                .map(str::to_owned)
                .unwrap_or_else(|| entry.id.clone()),
            source_id: entry.id,
            parent_id: resolved_parent,
            name: entry.name,
            path: entry.relative_path,
            folder: false,
            size: entry.size,
            modified_at_ms: entry.modified_at_ms,
            revision: response.revision,
            writable: parent.writable,
        });
        domain.persist().await?;
        Ok(item)
    }

    pub(super) async fn mkdir(
        &self,
        domain_id: &str,
        parent_id: &str,
        name: &str,
    ) -> Result<Item, Error> {
        validate_name(name)?;
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        let view = domain.view.lock().await.clone();
        let parent = domain.index.lock().await.item(parent_id)?;
        let path = join(&parent.path, name);
        match self.stat_path(&view, parent_id, &path, "").await {
            Ok(_) => {
                return Err(Error::conflict(
                    "a file or folder with this name already exists",
                ))
            }
            Err(e) if e.code == "notFound" => (),
            Err(e) => return Err(e),
        }
        self.request(
            &view,
            Request::CreateDirectory {
                share_id: view.share_id.clone(),
                relative_path: parent.path,
                name: name.into(),
            },
        )
        .await?;
        let mut item = self.stat_path(&view, parent_id, &path, "").await?;
        item.writable = parent.writable;
        let item = domain.index.lock().await.upsert(item);
        domain.persist().await?;
        Ok(item)
    }

    pub(super) async fn move_item(
        &self,
        domain_id: &str,
        item_id: &str,
        parent_id: &str,
        name: &str,
        expected: &str,
    ) -> Result<Item, Error> {
        validate_name(name)?;
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        let view = domain.view.lock().await.clone();
        let old = domain.index.lock().await.item(item_id)?;
        if old.id == "root" {
            return Err(Error::conflict("cannot move the shared root"));
        }
        let current = self
            .stat_path(&view, &old.parent_id, &old.path, &old.id)
            .await?;
        if current.revision != expected {
            return Err(Error::conflict(
                "file changed elsewhere; local changes are retained",
            ));
        }
        let parent = domain.index.lock().await.item(parent_id)?;
        self.request(
            &view,
            Request::Move {
                share_id: view.share_id.clone(),
                relative_path: old.path,
                destination_path: join(&parent.path, name),
                overwrite: false,
                expected_revision: expected.into(),
            },
        )
        .await?;
        domain.index.lock().await.moved(item_id, &parent, name)?;
        domain.persist().await?;
        drop(_guard);
        self.stat(domain_id, item_id).await
    }

    pub(super) async fn delete(
        &self,
        domain_id: &str,
        item_id: &str,
        expected: &str,
        recursive: bool,
    ) -> Result<(), Error> {
        let domain = self.domain(domain_id).await?;
        let _guard = domain.operations.lock().await;
        let view = domain.view.lock().await.clone();
        let old = domain.index.lock().await.item(item_id)?;
        if old.id == "root" {
            return Err(Error::conflict("cannot delete the shared root"));
        }
        match self
            .stat_path(&view, &old.parent_id, &old.path, &old.id)
            .await
        {
            Ok(current) if current.revision != expected => {
                return Err(Error::conflict(
                    "file changed elsewhere; deletion was not applied",
                ))
            }
            Ok(_) => {
                self.request(
                    &view,
                    Request::ConditionalDelete {
                        share_id: view.share_id.clone(),
                        relative_path: old.path,
                        expected_revision: expected.into(),
                        recursive,
                    },
                )
                .await?;
            }
            Err(e) if e.code == "notFound" => (),
            Err(e) => return Err(e),
        }
        domain.index.lock().await.remove(item_id);
        domain.persist().await
    }

    async fn watch_domain(self: Arc<Self>, domain: Arc<Domain>) {
        let mut epoch = String::new();
        let mut sequence = 0;
        loop {
            if self.stopped.is_cancelled() {
                break;
            }
            let view = domain.view.lock().await.clone();
            if self.domains.lock().await.get(&view.id).is_none() {
                break;
            }
            let before = domain.index.lock().await.sequence;
            let result: Result<(String, u64), Error> = async {
                let response = tokio::time::timeout(
                    Duration::from_secs(40),
                    self.manager.remote_file_request(
                        &view.peer_id,
                        Request::WatchChanges {
                            share_id: view.share_id.clone(),
                            epoch: epoch.clone(),
                            after_sequence: sequence,
                        },
                    ),
                )
                .await
                .map_err(|_| Error::unavailable("remote change feed timed out"))??;
                if response.change_reset {
                    let shares = self.request(&view, Request::ListShares).await?;
                    let share = shares
                        .shares
                        .iter()
                        .find(|s| s.id == view.share_id)
                        .ok_or_else(|| Error::missing("sharing permission was removed"))?;
                    let mut index = domain.index.lock().await;
                    let items: Vec<_> = index.items.values().cloned().collect();
                    for mut item in items {
                        item.writable = share.writable;
                        index.upsert(item);
                    }
                    let directories: Vec<_> = index.enumerated.iter().cloned().collect();
                    drop(index);
                    for parent in directories {
                        if domain.index.lock().await.items.contains_key(&parent) {
                            self.enumerate(&view.id, &parent).await?;
                        }
                    }
                } else {
                    let mut directories = HashSet::new();
                    let mut index = domain.index.lock().await;
                    for path in response.changed_directories {
                        if let Some(id) = index.id_for_path(&path) {
                            if index.enumerated.contains(&id) {
                                directories.insert(id);
                            }
                        }
                    }
                    drop(index);
                    for parent in directories {
                        self.enumerate(&view.id, &parent).await?;
                    }
                }
                Ok((response.change_epoch, response.change_sequence))
            }
            .await;
            let mut state = domain.view.lock().await;
            let was_online = state.online;
            state.online = result.is_ok();
            if state.registered {
                state.error = result.as_ref().err().map(|e| e.message.clone());
            }
            drop(state);
            if let Ok((next_epoch, next_sequence)) = result {
                epoch = next_epoch;
                sequence = next_sequence;
                if before != domain.index.lock().await.sequence || !was_online {
                    #[cfg(target_os = "macos")]
                    macos::signal(&view, !was_online);
                }
            } else {
                tokio::select! {
                    _ = self.stopped.cancelled() => break,
                    _ = tokio::time::sleep(Duration::from_secs(2)) => (),
                }
            }
        }
    }

    pub fn shutdown(&self) {
        self.stopped.cancel();
    }

    pub async fn open_recovery(&self) -> Result<(), Error> {
        let path = self.root.join("recovery");
        tokio::fs::create_dir_all(&path).await?;
        #[cfg(target_os = "macos")]
        {
            tokio::process::Command::new("/usr/bin/open")
                .arg("--")
                .arg(path)
                .status()
                .await?;
        }
        #[cfg(target_os = "windows")]
        {
            tokio::process::Command::new("explorer.exe")
                .arg(path)
                .spawn()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod construction_tests {
    #[test]
    fn system_folder_slot_constructs_without_tokio() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        assert!(super::empty_slot().get().is_none());
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[tokio::test]
    async fn recovery_status_excludes_active_and_committed_uploads() {
        let root =
            std::env::temp_dir().join(format!("arcrelay-recovery-test-{}", uuid::Uuid::new_v4()));
        let active = std::sync::Mutex::new(HashSet::new());
        assert!(pending_recoveries(&root, &active).await.is_empty());
        for (name, phase, domain) in [
            ("failed", "complete-pending-remote-acknowledgement", "a"),
            ("interrupted", "incomplete", "a"),
            ("active", "incomplete", "a"),
            ("committed", "committed", "a"),
            ("other", "incomplete", "b"),
        ] {
            let path = root.join(name);
            tokio::fs::create_dir_all(&path).await.unwrap();
            tokio::fs::write(
                path.join("save.json"),
                serde_json::to_vec(
                    &serde_json::json!({"phase": phase, "request": {"domain": domain}}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        }
        active.lock().unwrap().insert(root.join("active"));
        let counts = pending_recoveries(&root, &active).await;
        assert_eq!(counts.get("a"), Some(&2));
        assert_eq!(counts.get("b"), Some(&1));
        // A canceled upload or a process restart releases the active guard.
        active.lock().unwrap().clear();
        assert_eq!(pending_recoveries(&root, &active).await.get("a"), Some(&3));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
