use std::backtrace::Backtrace;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing_appender::non_blocking::{ErrorCounter, NonBlocking, WorkerGuard};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{reload, EnvFilter};
use uuid::Uuid;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub const LOG_SCHEMA_VERSION: u32 = 1;
pub const MAX_LOG_FILE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_LOG_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
pub const LOG_RETENTION_DAYS: u64 = 7;
const LOG_QUEUE_LINES: usize = 8_192;
const LOG_FILE_PREFIX: &str = "desktop-";
const LOG_FILE_EXTENSION: &str = "jsonl";

static STATE: OnceLock<Arc<ObservabilityState>> = OnceLock::new();

pub struct ObservabilityGuard {
    _worker: Option<WorkerGuard>,
}

struct ObservabilityState {
    enabled: bool,
    run_id: String,
    started_at_ms: u64,
    log_directory: PathBuf,
    current_log_file: Arc<RwLock<PathBuf>>,
    submitted_lines: Arc<AtomicU64>,
    processed_lines: Arc<AtomicU64>,
    error_counter: Option<ErrorCounter>,
    default_filter: String,
    current_filter: RwLock<String>,
    detailed_until_ms: AtomicU64,
    filter_generation: AtomicU64,
    reload_filter: Arc<dyn Fn(EnvFilter) -> Result<(), String> + Send + Sync>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub enabled: bool,
    pub run_id: String,
    pub started_at_ms: u64,
    pub log_directory: String,
    pub current_log_file: Option<String>,
    pub dropped_lines: usize,
    pub schema_version: u32,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    pub retention_days: u64,
    pub current_filter: String,
    pub detailed_until_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticBundleInfo {
    pub path: String,
    pub file_name: String,
    pub log_file_count: usize,
    pub size_bytes: u64,
}

#[derive(Clone, Copy)]
struct RetentionPolicy {
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_age: Duration,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_file_bytes: MAX_LOG_FILE_BYTES,
            max_total_bytes: MAX_LOG_TOTAL_BYTES,
            max_age: Duration::from_secs(LOG_RETENTION_DAYS * 24 * 60 * 60),
        }
    }
}

pub fn init() -> ObservabilityGuard {
    let run_id = Uuid::new_v4().to_string();
    let started_at_ms = unix_ms();
    let log_directory = default_log_directory();
    let current_log_file = Arc::new(RwLock::new(PathBuf::new()));
    let submitted_lines = Arc::new(AtomicU64::new(0));
    let processed_lines = Arc::new(AtomicU64::new(0));
    let mut worker = None;
    let mut writer = None;
    let mut error_counter = None;
    let mut disk_error = None;

    let file_writer = RotatingJsonLineWriter::new(
        log_directory.clone(),
        run_id.clone(),
        started_at_ms,
        current_log_file.clone(),
        processed_lines.clone(),
        RetentionPolicy::default(),
    );
    if let Ok(file_writer) = file_writer {
        let (non_blocking, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
            .buffered_lines_limit(LOG_QUEUE_LINES)
            .lossy(true)
            .thread_name("arcrelay-log-writer")
            .finish(file_writer);
        error_counter = Some(non_blocking.error_counter());
        writer = Some(TrackedNonBlocking {
            inner: non_blocking,
            submitted_lines: submitted_lines.clone(),
        });
        worker = Some(guard);
    } else if let Err(error) = file_writer {
        disk_error = Some(error.to_string());
    }

    let filter_source = std::env::var("ARCRELAY_LOG")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    let (filter, effective_filter) = match EnvFilter::try_new(&filter_source) {
        Ok(filter) => (filter, filter_source.clone()),
        Err(_) => (EnvFilter::new("info"), "info".to_string()),
    };
    let (filter_layer, reload_handle) = reload::Layer::new(filter);
    let reload_filter: Arc<dyn Fn(EnvFilter) -> Result<(), String> + Send + Sync> =
        Arc::new(move |filter| {
            reload_handle
                .reload(filter)
                .map_err(|error| error.to_string())
        });
    let console_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_target(true)
        .with_thread_names(true);
    let file_layer = writer.as_ref().map(|writer| {
        tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_ansi(false)
            .with_target(true)
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .with_writer(writer.clone())
    });
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(console_layer)
        .with(file_layer)
        .init();

    let state = Arc::new(ObservabilityState {
        enabled: writer.is_some(),
        run_id: run_id.clone(),
        started_at_ms,
        log_directory: log_directory.clone(),
        current_log_file,
        submitted_lines,
        processed_lines,
        error_counter,
        default_filter: effective_filter.clone(),
        current_filter: RwLock::new(effective_filter.clone()),
        detailed_until_ms: AtomicU64::new(0),
        filter_generation: AtomicU64::new(0),
        reload_filter,
    });
    let _ = STATE.set(state);
    install_panic_hook();

    tracing::info!(
        event = "app.started",
        schema_version = LOG_SCHEMA_VERSION,
        run_id = %run_id,
        app_version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        pid = std::process::id(),
        disk_logging = disk_error.is_none(),
        filter = %effective_filter,
        "ArcRelay desktop started"
    );
    if let Some(error) = disk_error {
        tracing::error!(
            event = "logging.disk_unavailable",
            %error,
            "disk logging is unavailable; continuing with console logging"
        );
    }

    ObservabilityGuard { _worker: worker }
}

pub fn status() -> LogStatus {
    let Some(state) = STATE.get() else {
        return LogStatus {
            enabled: false,
            run_id: String::new(),
            started_at_ms: 0,
            log_directory: default_log_directory().display().to_string(),
            current_log_file: None,
            dropped_lines: 0,
            schema_version: LOG_SCHEMA_VERSION,
            max_file_bytes: MAX_LOG_FILE_BYTES,
            max_total_bytes: MAX_LOG_TOTAL_BYTES,
            retention_days: LOG_RETENTION_DAYS,
            current_filter: "info".into(),
            detailed_until_ms: None,
        };
    };
    let current = state
        .current_log_file
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    LogStatus {
        enabled: state.enabled,
        run_id: state.run_id.clone(),
        started_at_ms: state.started_at_ms,
        log_directory: state.log_directory.display().to_string(),
        current_log_file: (!current.as_os_str().is_empty()).then(|| current.display().to_string()),
        dropped_lines: state
            .error_counter
            .as_ref()
            .map_or(0, ErrorCounter::dropped_lines),
        schema_version: LOG_SCHEMA_VERSION,
        max_file_bytes: MAX_LOG_FILE_BYTES,
        max_total_bytes: MAX_LOG_TOTAL_BYTES,
        retention_days: LOG_RETENTION_DAYS,
        current_filter: state
            .current_filter
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone(),
        detailed_until_ms: match state.detailed_until_ms.load(Ordering::Acquire) {
            0 => None,
            value => Some(value),
        },
    }
}

pub fn set_detailed_logging(enabled: bool) -> Result<LogStatus, String> {
    let state = STATE
        .get()
        .ok_or_else(|| "logging is not initialized".to_string())?
        .clone();
    let generation = state.filter_generation.fetch_add(1, Ordering::AcqRel) + 1;
    if !enabled {
        let filter =
            EnvFilter::try_new(&state.default_filter).map_err(|error| error.to_string())?;
        (state.reload_filter)(filter)?;
        *state
            .current_filter
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state.default_filter.clone();
        state.detailed_until_ms.store(0, Ordering::Release);
        tracing::info!(
            event = "logging.detailed_disabled",
            "temporary detailed logging disabled"
        );
        return Ok(status());
    }

    let detailed_filter = format!(
        "{},arcrelay_desktop::arc_input=debug,arcrelay_network=debug,arcrelay_input=debug",
        state.default_filter
    );
    let filter = EnvFilter::try_new(&detailed_filter).map_err(|error| error.to_string())?;
    (state.reload_filter)(filter)?;
    *state
        .current_filter
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = detailed_filter.clone();
    let until_ms = unix_ms().saturating_add(15 * 60 * 1_000);
    state.detailed_until_ms.store(until_ms, Ordering::Release);
    tracing::info!(
        event = "logging.detailed_enabled",
        duration_minutes = 15_u64,
        "temporary detailed logging enabled"
    );

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        if state.filter_generation.load(Ordering::Acquire) != generation {
            return;
        }
        let Ok(filter) = EnvFilter::try_new(&state.default_filter) else {
            return;
        };
        if (state.reload_filter)(filter).is_ok() {
            *state
                .current_filter
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = state.default_filter.clone();
            state.detailed_until_ms.store(0, Ordering::Release);
            tracing::info!(
                event = "logging.detailed_expired",
                "temporary detailed logging expired"
            );
        }
    });
    Ok(status())
}

pub fn flush() {
    let Some(state) = STATE.get() else {
        return;
    };
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        let submitted = state.submitted_lines.load(Ordering::Acquire);
        let processed = state.processed_lines.load(Ordering::Acquire);
        let dropped = state
            .error_counter
            .as_ref()
            .map_or(0, ErrorCounter::dropped_lines) as u64;
        if processed.saturating_add(dropped) >= submitted || Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn start_health_reporter() {
    tauri::async_runtime::spawn(async move {
        let mut previous_dropped = 0_usize;
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let status = status();
            let current_size_bytes = status
                .current_log_file
                .as_deref()
                .and_then(|path| fs::metadata(path).ok())
                .map_or(0, |metadata| metadata.len());
            if status.dropped_lines > previous_dropped {
                tracing::warn!(
                    event = "logging.lines_dropped",
                    dropped_lines = status.dropped_lines,
                    dropped_since_last_report = status.dropped_lines - previous_dropped,
                    current_size_bytes,
                    "desktop log writer dropped lines"
                );
            } else {
                tracing::info!(
                    event = "logging.health",
                    disk_logging = status.enabled,
                    dropped_lines = status.dropped_lines,
                    current_size_bytes,
                    "desktop log writer health"
                );
            }
            previous_dropped = status.dropped_lines;
        }
    });
}

pub fn log_directory() -> PathBuf {
    STATE
        .get()
        .map(|state| state.log_directory.clone())
        .unwrap_or_else(default_log_directory)
}

pub fn diagnostics_directory() -> PathBuf {
    log_directory().join("diagnostics")
}

#[cfg(target_os = "macos")]
pub fn ensure_diagnostics_directory() -> io::Result<PathBuf> {
    let directory = diagnostics_directory();
    fs::create_dir_all(&directory)?;
    harden_directory(&directory)?;
    Ok(directory)
}

pub fn pseudonym(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("peer-{}", hex_prefix(&digest, 12))
}

pub fn sanitize_text(value: &str, max_chars: usize) -> String {
    let mut sanitized = value.replace('\0', "");
    if let Some(home) = dirs::home_dir().and_then(|path| path.to_str().map(str::to_owned)) {
        if !home.is_empty() {
            sanitized = sanitized.replace(&home, "<home>");
        }
    }
    if sanitized.chars().count() > max_chars {
        sanitized = sanitized.chars().take(max_chars).collect::<String>();
        sanitized.push_str("…[truncated]");
    }
    sanitized
}

pub fn export_diagnostic_bundle<T: Serialize>(
    snapshot: &T,
) -> Result<DiagnosticBundleInfo, String> {
    flush();
    let state = STATE
        .get()
        .ok_or_else(|| "logging is not initialized".to_string())?;
    fs::create_dir_all(state.log_directory.join("exports")).map_err(|error| error.to_string())?;
    harden_directory(&state.log_directory.join("exports")).map_err(|error| error.to_string())?;

    let short_run = state.run_id.chars().take(8).collect::<String>();
    let file_name = format!("ArcRelay-diagnostics-{}-{short_run}.zip", unix_ms());
    let destination = state.log_directory.join("exports").join(&file_name);
    let snapshot = serde_json::to_value(snapshot).map_err(|error| error.to_string())?;
    let sanitized_snapshot = sanitize_snapshot(snapshot);
    let topology = sanitized_snapshot
        .pointer("/configuration/layout")
        .cloned()
        .unwrap_or(Value::Null);
    let log_files = collect_log_files(&state.log_directory).map_err(|error| error.to_string())?;
    let hang_files =
        collect_diagnostic_files(&diagnostics_directory()).map_err(|error| error.to_string())?;
    let manifest = json!({
        "schemaVersion": LOG_SCHEMA_VERSION,
        "generatedAtMs": unix_ms(),
        "appVersion": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "runId": state.run_id,
        "logFiles": log_files.iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy()).collect::<Vec<_>>(),
        "droppedLogLines": state.error_counter.as_ref().map_or(0, ErrorCounter::dropped_lines),
        "privacy": {
            "identifiers": "pseudonymized",
            "networkAddresses": "address-family-only",
            "content": "excluded"
        }
    });
    write_bundle(
        &destination,
        &manifest,
        &sanitized_snapshot,
        &topology,
        &log_files,
        &hang_files,
    )
    .map_err(|error| error.to_string())?;
    harden_file(&destination).map_err(|error| error.to_string())?;
    let size_bytes = destination
        .metadata()
        .map_err(|error| error.to_string())?
        .len();

    tracing::info!(
        event = "diagnostics.bundle_exported",
        log_file_count = log_files.len(),
        size_bytes,
        "diagnostic bundle exported"
    );
    Ok(DiagnosticBundleInfo {
        path: destination.display().to_string(),
        file_name,
        log_file_count: log_files.len(),
        size_bytes,
    })
}

fn default_log_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("ArcRelay")
        .join("logs")
        .join("desktop")
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|location| {
                format!(
                    "{}:{}:{}",
                    location.file(),
                    location.line(),
                    location.column()
                )
            })
            .unwrap_or_else(|| "unknown".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-string panic payload");
        let backtrace = Backtrace::force_capture().to_string();
        tracing::error!(
            event = "app.panic",
            location = %sanitize_text(&location, 1_024),
            detail = %sanitize_text(payload, 4_096),
            backtrace = %sanitize_text(&backtrace, 32_768),
            "ArcRelay desktop panicked"
        );
        flush();
        previous(info);
    }));
}

struct RotatingJsonLineWriter {
    directory: PathBuf,
    run_id: String,
    started_at_ms: u64,
    segment: u32,
    file: File,
    bytes_written: u64,
    current_path: Arc<RwLock<PathBuf>>,
    policy: RetentionPolicy,
    pending: Vec<u8>,
    enrichment_started: Instant,
    processed_lines: Arc<AtomicU64>,
}

impl RotatingJsonLineWriter {
    fn new(
        directory: PathBuf,
        run_id: String,
        started_at_ms: u64,
        current_path: Arc<RwLock<PathBuf>>,
        processed_lines: Arc<AtomicU64>,
        policy: RetentionPolicy,
    ) -> io::Result<Self> {
        fs::create_dir_all(&directory)?;
        harden_directory(&directory)?;
        prune_log_directory(&directory, None, policy)?;
        let (file, path, bytes_written) = open_segment(&directory, started_at_ms, 0)?;
        *current_path
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = path;
        Ok(Self {
            directory,
            run_id,
            started_at_ms,
            segment: 0,
            file,
            bytes_written,
            current_path,
            policy,
            pending: Vec::with_capacity(8 * 1024),
            enrichment_started: Instant::now(),
            processed_lines,
        })
    }

    fn write_line(&mut self, line: &[u8]) -> io::Result<()> {
        let mut enriched = match serde_json::from_slice::<Value>(line) {
            Ok(Value::Object(map)) => Value::Object(map),
            _ => json!({"message": sanitize_text(&String::from_utf8_lossy(line), 16_384)}),
        };
        if let Value::Object(map) = &mut enriched {
            map.insert("schema_version".into(), Value::from(LOG_SCHEMA_VERSION));
            map.insert("run_id".into(), Value::from(self.run_id.clone()));
            map.insert("app_version".into(), Value::from(env!("CARGO_PKG_VERSION")));
            map.insert("process".into(), Value::from("desktop"));
            map.insert("pid".into(), Value::from(std::process::id()));
            map.insert(
                "mono_ms".into(),
                Value::from(self.enrichment_started.elapsed().as_millis() as u64),
            );
            sanitize_log_value(map);
        }
        let mut bytes = serde_json::to_vec(&enriched).map_err(io::Error::other)?;
        bytes.push(b'\n');
        if self.bytes_written > 0
            && self.bytes_written.saturating_add(bytes.len() as u64) > self.policy.max_file_bytes
        {
            self.rotate()?;
        }
        self.file.write_all(&bytes)?;
        self.bytes_written = self.bytes_written.saturating_add(bytes.len() as u64);
        self.processed_lines.fetch_add(1, Ordering::Release);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        self.segment = self.segment.saturating_add(1);
        let (file, path, bytes_written) =
            open_segment(&self.directory, self.started_at_ms, self.segment)?;
        self.file = file;
        self.bytes_written = bytes_written;
        *self
            .current_path
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = path.clone();
        prune_log_directory(&self.directory, Some(&path), self.policy)
    }
}

impl Write for RotatingJsonLineWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buffer);
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line = self.pending.drain(..=index).collect::<Vec<_>>();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            if !line.is_empty() {
                self.write_line(line)?;
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let pending = std::mem::take(&mut self.pending);
            self.write_line(&pending)?;
        }
        self.file.flush()
    }
}

#[derive(Clone)]
struct TrackedNonBlocking {
    inner: NonBlocking,
    submitted_lines: Arc<AtomicU64>,
}

impl Write for TrackedNonBlocking {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let lines = buffer.iter().filter(|byte| **byte == b'\n').count() as u64;
        let result = self.inner.write(buffer);
        if result.is_ok() && lines > 0 {
            self.submitted_lines.fetch_add(lines, Ordering::Release);
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'a> MakeWriter<'a> for TrackedNonBlocking {
    type Writer = TrackedNonBlocking;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn open_segment(
    directory: &Path,
    started_at_ms: u64,
    segment: u32,
) -> io::Result<(File, PathBuf, u64)> {
    let path = directory.join(format!(
        "{LOG_FILE_PREFIX}{started_at_ms}-{}-{segment:03}.{LOG_FILE_EXTENSION}",
        std::process::id()
    ));
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    harden_file(&path)?;
    let length = file.metadata()?.len();
    Ok((file, path, length))
}

fn prune_log_directory(
    directory: &Path,
    current: Option<&Path>,
    policy: RetentionPolicy,
) -> io::Result<()> {
    let now = SystemTime::now();
    let mut files = collect_log_files(directory)?
        .into_iter()
        .filter_map(|path| {
            let metadata = path.metadata().ok()?;
            Some((
                path,
                metadata.len(),
                metadata.modified().unwrap_or(UNIX_EPOCH),
            ))
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(_, _, modified)| *modified);

    for (path, _, modified) in &files {
        if current.is_some_and(|current| current == path) {
            continue;
        }
        if now.duration_since(*modified).unwrap_or_default() > policy.max_age {
            let _ = fs::remove_file(path);
        }
    }

    files.retain(|(path, _, _)| path.exists());
    let mut total = files.iter().map(|(_, size, _)| *size).sum::<u64>();
    for (path, size, _) in files {
        if total <= policy.max_total_bytes {
            break;
        }
        if current.is_some_and(|current| current == path) {
            continue;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
    Ok(())
}

fn collect_log_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !directory.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let is_log = path.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == LOG_FILE_EXTENSION)
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(LOG_FILE_PREFIX));
        if is_log {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn collect_diagnostic_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    if !directory.exists() {
        return Ok(files);
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn write_bundle(
    destination: &Path,
    manifest: &Value,
    snapshot: &Value,
    topology: &Value,
    log_files: &[PathBuf],
    diagnostic_files: &[PathBuf],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::create(destination)?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o600);
    write_json_entry(&mut zip, "manifest.json", manifest, options)?;
    write_json_entry(&mut zip, "runtime-snapshot.json", snapshot, options)?;
    write_json_entry(&mut zip, "topology.json", topology, options)?;
    zip.start_file("README.md", options)?;
    zip.write_all(DIAGNOSTIC_README.as_bytes())?;
    for path in log_files {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        zip.start_file(format!("logs/{name}"), options)?;
        copy_file_into(path, &mut zip)?;
    }
    for path in diagnostic_files {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        zip.start_file(format!("hang-samples/{name}"), options)?;
        copy_diagnostic_text_into(path, &mut zip)?;
    }
    zip.finish()?;
    Ok(())
}

fn write_json_entry<W: Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    name: &str,
    value: &Value,
    options: SimpleFileOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    zip.start_file(name, options)?;
    serde_json::to_writer_pretty(zip, value)?;
    Ok(())
}

fn copy_file_into(path: &Path, writer: &mut impl Write) -> io::Result<()> {
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        writer.write_all(&buffer[..read])?;
    }
}

fn copy_diagnostic_text_into(path: &Path, writer: &mut impl Write) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    // macOS `sample` output can contain absolute binary and source paths. Keep
    // enough data for stack analysis while applying the same home-path boundary
    // as structured logs and preventing an unexpectedly large artifact.
    let sanitized = sanitize_text(&text, 16 * 1024 * 1024);
    writer.write_all(sanitized.as_bytes())
}

fn sanitize_snapshot(mut value: Value) -> Value {
    sanitize_snapshot_value(&mut value, None);
    value
}

fn sanitize_snapshot_value(value: &mut Value, parent_key: Option<&str>) {
    match value {
        Value::Object(map) => {
            let keys = map.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    let normalized = key.to_ascii_lowercase();
                    if normalized.contains("certificate")
                        || normalized.contains("token")
                        || normalized.contains("secret")
                        || normalized.contains("pairingcode")
                    {
                        *child = Value::String("[redacted]".into());
                    } else if normalized == "addresses" {
                        let families = child
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(address_family)
                            .collect::<Vec<_>>();
                        *child = Value::Array(families.into_iter().map(Value::from).collect());
                    } else if matches!(
                        normalized.as_str(),
                        "serviceinstanceid"
                            | "deviceid"
                            | "controller"
                            | "sourcedeviceid"
                            | "targetdeviceid"
                    ) {
                        if let Some(identifier) = child.as_str() {
                            *child = Value::String(pseudonym(identifier));
                        }
                    } else {
                        sanitize_snapshot_value(child, Some(&key));
                    }
                }
            }
            if parent_key.is_some_and(|key| key == "remoteOperatingSystems") {
                let entries = std::mem::take(map);
                *map = entries
                    .into_iter()
                    .map(|(key, value)| (pseudonym(&key), value))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect();
            }
        }
        Value::Array(values) => {
            let identifier_list =
                parent_key.is_some_and(|key| matches!(key, "discoveredPeers" | "connectedPeers"));
            for value in values {
                if identifier_list {
                    if let Some(identifier) = value.as_str() {
                        *value = Value::String(pseudonym(identifier));
                    }
                } else {
                    sanitize_snapshot_value(value, parent_key);
                }
            }
        }
        Value::String(text) => *text = sanitize_text(text, 16_384),
        _ => {}
    }
}

fn sanitize_log_value(map: &mut serde_json::Map<String, Value>) {
    for (key, value) in map.iter_mut() {
        let normalized = key.to_ascii_lowercase();
        if normalized.contains("token")
            || normalized.contains("secret")
            || normalized.contains("certificate")
            || normalized.contains("pairing_code")
            || normalized == "hid_usage"
            || normalized == "text_commit"
            || normalized == "clipboard_content"
        {
            *value = Value::String("[redacted]".into());
            continue;
        }
        if matches!(
            normalized.as_str(),
            "peer_id"
                | "device_id"
                | "service_instance_id"
                | "source_peer"
                | "target_peer"
                | "controller_id"
        ) {
            if let Some(identifier) = value.as_str() {
                if !identifier.starts_with("peer-") {
                    *value = Value::String(pseudonym(identifier));
                }
            }
            continue;
        }
        if matches!(normalized.as_str(), "share_id" | "path" | "file_path") {
            *value = Value::String("[redacted]".into());
            continue;
        }
        if normalized.contains("address")
            || matches!(normalized.as_str(), "remote" | "host" | "source_ip")
        {
            if let Some(address) = value.as_str() {
                if let Some(family) = parsed_address_family(address) {
                    *value = Value::String(family.into());
                    continue;
                }
            }
        }
        match value {
            Value::String(text) => *text = sanitize_text(text, 32_768),
            Value::Object(object) => sanitize_log_value(object),
            Value::Array(values) => {
                for value in values {
                    if let Value::String(text) = value {
                        *text = sanitize_text(text, 32_768);
                    }
                }
            }
            _ => {}
        }
    }
}

fn address_family(value: &str) -> &'static str {
    if value.parse::<std::net::Ipv4Addr>().is_ok() {
        "ipv4"
    } else if value.parse::<std::net::Ipv6Addr>().is_ok() {
        "ipv6"
    } else {
        "network-address"
    }
}

fn parsed_address_family(value: &str) -> Option<&'static str> {
    if let Ok(address) = value.parse::<std::net::IpAddr>() {
        return Some(if address.is_ipv4() { "ipv4" } else { "ipv6" });
    }
    value
        .parse::<std::net::SocketAddr>()
        .ok()
        .map(|address| if address.is_ipv4() { "ipv4" } else { "ipv6" })
}

fn hex_prefix(bytes: &[u8], characters: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(characters);
    for byte in bytes {
        result.push(HEX[(byte >> 4) as usize] as char);
        if result.len() == characters {
            break;
        }
        result.push(HEX[(byte & 0x0f) as usize] as char);
        if result.len() == characters {
            break;
        }
    }
    result
}

fn harden_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn harden_file(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

const DIAGNOSTIC_README: &str = r#"# ArcRelay desktop diagnostic bundle

The JSONL files use one JSON object per line. Start with `manifest.json`, then group
events by `run_id`, `event`, `control_epoch`, `topology_revision`, and `sequence`.

Useful queries:

```sh
jq -s 'sort_by(.timestamp)' logs/*.jsonl
jq -s '[.[] | select(.event | startswith("input.handoff"))]' logs/*.jsonl
jq -s '[.[] | select(.level == "WARN" or .level == "ERROR")]' logs/*.jsonl
```

Device identifiers and network addresses in snapshots are pseudonymized. Key values,
committed text, clipboard content, identity material, pairing databases, and tokens are
not included.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_writer_enriches_and_rotates_json_lines() {
        let directory = tempfile::tempdir().unwrap();
        let current = Arc::new(RwLock::new(PathBuf::new()));
        let mut writer = RotatingJsonLineWriter::new(
            directory.path().to_path_buf(),
            "test-run".into(),
            42,
            current,
            Arc::new(AtomicU64::new(0)),
            RetentionPolicy {
                max_file_bytes: 240,
                max_total_bytes: 4_096,
                max_age: Duration::from_secs(60),
            },
        )
        .unwrap();
        writer
            .write_all(b"{\"event\":\"first\",\"message\":\"ok\"}\n")
            .unwrap();
        writer.write_all(b"{\"event\":\"second\",\"message\":\"a message long enough to rotate the tiny test file\"}\n").unwrap();
        writer.flush().unwrap();

        let files = collect_log_files(directory.path()).unwrap();
        assert!(files.len() >= 2);
        let first = fs::read_to_string(&files[0]).unwrap();
        let value: Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
        assert_eq!(value["schema_version"], LOG_SCHEMA_VERSION);
        assert_eq!(value["run_id"], "test-run");
        assert_eq!(value["process"], "desktop");
    }

    #[test]
    fn tracing_json_reaches_disk_before_the_flush_barrier_completes() {
        let directory = tempfile::tempdir().unwrap();
        let current = Arc::new(RwLock::new(PathBuf::new()));
        let submitted = Arc::new(AtomicU64::new(0));
        let processed = Arc::new(AtomicU64::new(0));
        let writer = RotatingJsonLineWriter::new(
            directory.path().to_path_buf(),
            "barrier-run".into(),
            43,
            current.clone(),
            processed.clone(),
            RetentionPolicy::default(),
        )
        .unwrap();
        let (non_blocking, guard) =
            tracing_appender::non_blocking::NonBlockingBuilder::default().finish(writer);
        let tracked = TrackedNonBlocking {
            inner: non_blocking,
            submitted_lines: submitted.clone(),
        };
        let subscriber = tracing_subscriber::fmt()
            .json()
            .flatten_event(true)
            .with_writer(tracked)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(event = "test.flush_barrier", value = 7, "barrier test");
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while processed.load(Ordering::Acquire) < submitted.load(Ordering::Acquire)
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(submitted.load(Ordering::Acquire), 1);
        assert_eq!(processed.load(Ordering::Acquire), 1);
        drop(guard);
        let path = current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let line = fs::read_to_string(path).unwrap();
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["event"], "test.flush_barrier");
        assert_eq!(value["run_id"], "barrier-run");
    }

    #[test]
    fn snapshot_sanitization_removes_identity_and_address_material() {
        let sanitized = sanitize_snapshot(json!({
            "serviceInstanceId": "arc-device-1",
            "connectedPeers": ["arc-device-2"],
            "nearbyPeers": [{
                "deviceId": "arc-device-2",
                "addresses": ["192.168.1.8", "fe80::1"],
                "certificateSha256": "secret-fingerprint"
            }]
        }));
        let encoded = serde_json::to_string(&sanitized).unwrap();
        assert!(!encoded.contains("arc-device"));
        assert!(!encoded.contains("192.168"));
        assert!(!encoded.contains("secret-fingerprint"));
        assert!(encoded.contains("peer-"));
        assert!(encoded.contains("ipv4"));
        assert!(encoded.contains("ipv6"));
    }

    #[test]
    fn log_field_sanitization_pseudonymizes_ids_and_addresses() {
        let mut object = serde_json::Map::from_iter([
            ("peer_id".into(), Value::String("arc-device-1".into())),
            ("remote".into(), Value::String("192.168.1.8:8765".into())),
            ("source_ip".into(), Value::String("192.168.1.9".into())),
            ("share_id".into(), Value::String("private-share".into())),
            ("path".into(), Value::String("private/report.pdf".into())),
            ("token".into(), Value::String("secret".into())),
        ]);
        sanitize_log_value(&mut object);
        assert!(object["peer_id"].as_str().unwrap().starts_with("peer-"));
        assert_eq!(object["remote"], "ipv4");
        assert_eq!(object["source_ip"], "ipv4");
        assert_eq!(object["share_id"], "[redacted]");
        assert_eq!(object["path"], "[redacted]");
        assert_eq!(object["token"], "[redacted]");
    }

    #[test]
    fn text_sanitization_hides_home_and_truncates() {
        let home = dirs::home_dir().unwrap();
        let value = format!("{}/private/{}", home.display(), "x".repeat(50));
        let sanitized = sanitize_text(&value, 20);
        assert!(!sanitized.contains(&home.display().to_string()));
        assert!(sanitized.ends_with("…[truncated]"));
    }

    #[test]
    fn diagnostic_bundle_contains_sanitized_context_and_logs() {
        let directory = tempfile::tempdir().unwrap();
        let log_path = directory.path().join("desktop-test.jsonl");
        fs::write(&log_path, "{\"event\":\"input.handoff.failed\"}\n").unwrap();
        let sample_path = directory.path().join("hang.sample.txt");
        let home = dirs::home_dir().unwrap();
        fs::write(&sample_path, format!("{}\nsample", home.display())).unwrap();
        let destination = directory.path().join("bundle.zip");
        let snapshot = sanitize_snapshot(json!({
            "serviceInstanceId": "arc-secret-device",
            "nearbyPeers": [{"addresses": ["10.0.0.4"]}]
        }));
        write_bundle(
            &destination,
            &json!({"schemaVersion": 1}),
            &snapshot,
            &Value::Null,
            &[log_path],
            &[sample_path],
        )
        .unwrap();

        let file = File::open(destination).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert!(archive.by_name("manifest.json").is_ok());
        assert!(archive.by_name("logs/desktop-test.jsonl").is_ok());
        assert!(archive.by_name("hang-samples/hang.sample.txt").is_ok());
        let mut snapshot_entry = archive.by_name("runtime-snapshot.json").unwrap();
        let mut encoded = String::new();
        snapshot_entry.read_to_string(&mut encoded).unwrap();
        assert!(!encoded.contains("arc-secret-device"));
        assert!(!encoded.contains("10.0.0.4"));
        drop(snapshot_entry);
        let mut sample_entry = archive.by_name("hang-samples/hang.sample.txt").unwrap();
        let mut sample = String::new();
        sample_entry.read_to_string(&mut sample).unwrap();
        assert!(!sample.contains(&home.display().to_string()));
        assert!(sample.contains("<home>"));
    }

    #[test]
    fn pruning_never_touches_non_log_artifacts() {
        let directory = tempfile::tempdir().unwrap();
        let artifact = directory.path().join("keep-me.zip");
        fs::write(&artifact, vec![0_u8; 512]).unwrap();
        for index in 0..4 {
            fs::write(
                directory.path().join(format!("desktop-{index}.jsonl")),
                vec![0_u8; 256],
            )
            .unwrap();
        }
        prune_log_directory(
            directory.path(),
            None,
            RetentionPolicy {
                max_file_bytes: 256,
                max_total_bytes: 300,
                max_age: Duration::from_secs(60),
            },
        )
        .unwrap();
        assert!(artifact.exists());
        let remaining = collect_log_files(directory.path()).unwrap();
        assert!(remaining.len() <= 1);
    }
}
