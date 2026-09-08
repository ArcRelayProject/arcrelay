//! Owner-authenticated, loopback-only bridge for the sandboxed macOS extension.
use super::*;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, Request as HttpRequest, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

#[derive(Clone)]
pub(super) struct Bridge {
    pub(super) service: Arc<SystemFolders>,
    token: String,
    authority: String,
    uploads: Arc<tokio::sync::Semaphore>,
}

impl Bridge {
    #[cfg(any(target_os = "windows", test))]
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn for_webdav(service: Arc<SystemFolders>) -> Self {
        Self {
            service,
            token: String::new(),
            authority: String::new(),
            uploads: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
}

pub(super) fn bridge_directory(root: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        return home.join("Library/Group Containers/group.com.arcrelay.shared/FileProvider");
    }
    root.to_path_buf()
}

pub(super) async fn start(service: Arc<SystemFolders>) -> Result<(), Error> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let directory = bridge_directory(&service.root);
    tokio::fs::create_dir_all(&directory).await?;
    let path = directory.join("connection.json");
    let bytes =
        serde_json::to_vec(&serde_json::json!({ "port": address.port(), "token": token })).unwrap();
    crate::infrastructure::durable_file::replace_private(&path, &bytes)?;
    let bridge = Bridge {
        service: service.clone(),
        token,
        authority: address.to_string(),
        uploads: Arc::new(tokio::sync::Semaphore::new(4)),
    };
    let routes = Router::new()
        .route(
            "/v1/operation",
            post(operation).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/v1/content", get(content).put(upload))
        .layer(DefaultBodyLimit::disable())
        .layer(middleware::from_fn_with_state(bridge.clone(), authenticate))
        .with_state(bridge);
    let stopped = service.stopped.clone();
    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, routes)
            .with_graceful_shutdown(stopped.cancelled_owned())
            .await
        {
            tracing::error!(%error, "system folder bridge stopped");
        }
    });
    Ok(())
}

fn authorized(headers: &HeaderMap, token: &str, authority: &str) -> bool {
    headers.get("host").and_then(|h| h.to_str().ok()) == Some(authority)
        && !headers.contains_key("origin")
        && !headers.contains_key("sec-fetch-site")
        && headers.get("authorization").and_then(|h| h.to_str().ok())
            == Some(format!("Bearer {token}").as_str())
}

async fn authenticate(
    State(bridge): State<Bridge>,
    request: HttpRequest<Body>,
    next: Next,
) -> Response {
    if !authorized(request.headers(), &bridge.token, &bridge.authority) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if request.uri().path() == "/v1/operation"
        && request
            .headers()
            .get("content-length")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.parse::<u64>().ok())
            .is_none_or(|n| n > 64 * 1024)
    {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "notFound" => StatusCode::NOT_FOUND,
            "conflict" | "anchorExpired" => StatusCode::CONFLICT,
            "invalid" => StatusCode::BAD_REQUEST,
            "forbidden" => StatusCode::FORBIDDEN,
            "quota" => StatusCode::INSUFFICIENT_STORAGE,
            "unsupported" => StatusCode::NOT_IMPLEMENTED,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (status, Json(self)).into_response()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Operation {
    domain: String,
    action: String,
    #[serde(default = "root_id")]
    item: String,
    #[serde(default = "root_id")]
    parent: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    revision: String,
    #[serde(default)]
    anchor: u64,
    #[serde(default)]
    recursive: bool,
}
fn root_id() -> String {
    "root".into()
}

async fn operation(
    State(bridge): State<Bridge>,
    Json(op): Json<Operation>,
) -> Result<Json<serde_json::Value>, Error> {
    let service = &bridge.service;
    let result = match op.action.as_str() {
        "stat" => serde_json::to_value(service.stat(&op.domain, &op.item).await?).unwrap(),
        "list" => serde_json::json!({ "items": service.enumerate(&op.domain, &op.item).await? }),
        "workingSet" => {
            let domain = service.domain(&op.domain).await?;
            let index = domain.index.lock().await;
            serde_json::json!({ "items": index.items.values().filter(|i| i.id != "root").collect::<Vec<_>>(), "anchor": index.sequence })
        }
        "changes" => {
            let domain = service.domain(&op.domain).await?;
            let view = domain.view.lock().await.clone();
            if !view.online {
                return Err(Error::unavailable(
                    view.error.unwrap_or_else(|| "device is offline".into()),
                ));
            }
            let index = domain.index.lock().await;
            serde_json::json!({ "changes": index.since(op.anchor)?, "anchor": index.sequence })
        }
        "mkdir" => {
            serde_json::to_value(service.mkdir(&op.domain, &op.parent, &op.name).await?).unwrap()
        }
        "move" => serde_json::to_value(
            service
                .move_item(&op.domain, &op.item, &op.parent, &op.name, &op.revision)
                .await?,
        )
        .unwrap(),
        "delete" => {
            service
                .delete(&op.domain, &op.item, &op.revision, op.recursive)
                .await?;
            serde_json::json!({})
        }
        _ => {
            return Err(Error {
                code: "invalid".into(),
                message: "unknown file operation".into(),
            })
        }
    };
    Ok(Json(result))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ContentQuery {
    pub(super) domain: String,
    pub(super) item: Option<String>,
    #[serde(default = "root_id")]
    pub(super) parent: String,
    #[serde(default)]
    pub(super) name: String,
    #[serde(default)]
    pub(super) revision: String,
}

pub(super) async fn content(
    State(bridge): State<Bridge>,
    Query(query): Query<ContentQuery>,
) -> Result<Response, Error> {
    let id = query
        .item
        .ok_or_else(|| Error::missing("file identity is required"))?;
    let item = bridge.service.stat(&query.domain, &id).await?;
    if item.folder {
        return Err(Error::conflict("cannot download a directory as a file"));
    }
    if !query.revision.is_empty() && query.revision != item.revision {
        return Err(Error::conflict(
            "requested file version is no longer available",
        ));
    }
    let header = STANDARD.encode(serde_json::to_vec(&item).unwrap());
    let length = item.size;
    let stream = futures_util::stream::try_unfold(
        (bridge.service, query.domain, item, 0u64),
        |(service, domain, item, offset)| async move {
            if offset >= item.size {
                return Ok::<_, std::io::Error>(None);
            }
            let bytes = service
                .read(
                    &domain,
                    &item,
                    offset,
                    arcrelay_protocol::remote_files::MAX_REMOTE_RANGE_BYTES,
                )
                .await
                .map_err(std::io::Error::other)?;
            if bytes.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "remote read ended early",
                ));
            }
            let next = offset + bytes.len() as u64;
            Ok(Some((bytes, (service, domain, item, next))))
        },
    );
    Ok(Response::builder()
        .status(200)
        .header("content-length", length)
        .header("content-type", "application/octet-stream")
        .header("x-arcrelay-item", header)
        .body(Body::from_stream(stream))
        .unwrap())
}

struct ActiveRecovery {
    service: Arc<SystemFolders>,
    path: PathBuf,
}
impl Drop for ActiveRecovery {
    fn drop(&mut self) {
        self.service
            .active_recoveries
            .lock()
            .unwrap()
            .remove(&self.path);
    }
}

pub(super) async fn upload(
    State(bridge): State<Bridge>,
    Query(query): Query<ContentQuery>,
    body: Body,
) -> Result<Json<Item>, Error> {
    let _slot = bridge
        .uploads
        .acquire()
        .await
        .map_err(|_| Error::unavailable("file provider stopped"))?;
    bridge.service.domain(&query.domain).await?;
    validate_name(&query.name)?;
    let recovery_root = bridge.service.root.join("recovery");
    tokio::fs::create_dir_all(&recovery_root).await?;
    // Bound retry accumulation. The OS still owns the original dirty document;
    // do not evict an earlier failed save just to accept another one.
    let mut entries = tokio::fs::read_dir(&recovery_root).await?;
    let mut count = 0;
    while entries.next_entry().await?.is_some() {
        count += 1;
        if count >= 64 {
            return Err(Error {
                code: "quota".into(),
                message: "recovery storage has 64 pending saves; recover them before retrying"
                    .into(),
            });
        }
    }
    let recovery = recovery_root.join(uuid::Uuid::new_v4().to_string());
    bridge
        .service
        .active_recoveries
        .lock()
        .unwrap()
        .insert(recovery.clone());
    let _active_recovery = ActiveRecovery {
        service: bridge.service.clone(),
        path: recovery.clone(),
    };
    tokio::fs::create_dir_all(&recovery).await?;
    let metadata = recovery.join("save.json");
    let manifest = |phase: &str| {
        serde_json::to_vec(&serde_json::json!({ "request": query, "phase": phase })).unwrap()
    };
    crate::infrastructure::durable_file::replace_private(&metadata, &manifest("incomplete"))?;
    let source = recovery.join("contents");
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&source).await?;
    let mut stream = body.into_data_stream();
    let mut size = 0u64;
    while let Some(chunk) = tokio::time::timeout(Duration::from_secs(30), stream.next())
        .await
        .map_err(|_| {
            Error::unavailable("local upload interrupted; partial recovery copy retained")
        })?
    {
        let chunk = chunk.map_err(|e| Error::unavailable(e.to_string()))?;
        size = size.saturating_add(chunk.len() as u64);
        if size > arcrelay_protocol::remote_files::MAX_REMOTE_FILE_CONTENT_SIZE {
            return Err(Error {
                code: "quota".into(),
                message: "file exceeds the transfer size limit".into(),
            });
        }
        file.write_all(&chunk).await?;
    }
    file.sync_all().await?;
    drop(file);
    crate::infrastructure::durable_file::replace_private(
        &metadata,
        &manifest("complete-pending-remote-acknowledgement"),
    )?;
    let item = bridge
        .service
        .save(
            &query.domain,
            query.item.as_deref(),
            &query.parent,
            &query.name,
            &query.revision,
            &source,
        )
        .await?;
    // Delete only this request's acknowledged staging copy. Native local copies
    // and failed/interrupted saves are never removed by this bridge.
    // Cleanup failures must not turn an acknowledged remote save into a retry.
    let _ = crate::infrastructure::durable_file::replace_private(&metadata, &manifest("committed"));
    let _ = tokio::fs::remove_file(source).await;
    let _ = tokio::fs::remove_file(metadata).await;
    let _ = tokio::fs::remove_dir(recovery).await;
    Ok(Json(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_origins_wrong_hosts_and_missing_credentials_are_rejected() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "127.0.0.1:1234".parse().unwrap());
        assert!(!authorized(&headers, "secret", "127.0.0.1:1234"));
        headers.insert("authorization", "Bearer secret".parse().unwrap());
        assert!(authorized(&headers, "secret", "127.0.0.1:1234"));
        headers.insert("origin", "https://example.com".parse().unwrap());
        assert!(!authorized(&headers, "secret", "127.0.0.1:1234"));
        headers.remove("origin");
        assert!(!authorized(&headers, "secret", "attacker.example:1234"));
    }
}
