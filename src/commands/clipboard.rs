use super::*;
use arcrelay_core::domain::clipboard::{
    ClipboardSummary, ClipboardTimelinePage, ClipboardTimelinePosition, ClipboardTimelineQuery,
};

static CLIPBOARD_THUMBNAILS: OnceLock<Mutex<ClipboardThumbnailCache>> = OnceLock::new();
static CLIPBOARD_IMAGE_PREVIEWS: OnceLock<Mutex<ClipboardImagePreviewCache>> = OnceLock::new();
static CONTINUOUS_PASTE: OnceLock<Mutex<ContinuousPasteQueue>> = OnceLock::new();

const CLIPBOARD_THUMBNAIL_LIMIT: usize = 24;
const CLIPBOARD_THUMBNAIL_CACHE_BYTES: usize = 32 * 1024 * 1024;
const CLIPBOARD_IMAGE_PREVIEW_LIMIT: usize = 4;
const CLIPBOARD_IMAGE_PREVIEW_CACHE_BYTES: usize = 64 * 1024 * 1024;
const CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION: u32 = 4_096;
static CLIPBOARD_CACHE_GENERATION: AtomicU64 = AtomicU64::new(0);
static CLIPBOARD_PREVIEW_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct ContinuousPasteQueue {
    ids: Vec<u64>,
    index: usize,
    in_flight: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ContinuousPasteProgress {
    pub current: usize,
    pub total: usize,
    pub active: bool,
}

fn continuous_paste_queue() -> &'static Mutex<ContinuousPasteQueue> {
    CONTINUOUS_PASTE.get_or_init(|| Mutex::new(ContinuousPasteQueue::default()))
}

#[derive(Clone)]
struct CachedClipboardImage {
    path: String,
    bytes: usize,
}

#[derive(Default)]
struct ClipboardThumbnailCache {
    order: std::collections::VecDeque<u64>,
    values: HashMap<u64, CachedClipboardImage>,
    total_bytes: usize,
}

#[derive(Default)]
struct ClipboardImagePreviewCache {
    order: std::collections::VecDeque<u64>,
    values: HashMap<u64, CachedClipboardImage>,
    total_bytes: usize,
}

impl ClipboardThumbnailCache {
    fn get(&mut self, id: u64) -> Option<String> {
        let value = self.values.get(&id)?.path.clone();
        self.order.retain(|current| *current != id);
        self.order.push_back(id);
        Some(value)
    }

    fn insert(&mut self, id: u64, value: CachedClipboardImage) {
        if let Some(previous) = self.values.remove(&id) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.bytes);
            remove_cached_clipboard_image(&previous);
        }
        self.values.insert(id, value);
        self.order.retain(|current| *current != id);
        self.order.push_back(id);
        self.total_bytes = self.values.values().map(|image| image.bytes).sum();
        while self.order.len() > CLIPBOARD_THUMBNAIL_LIMIT
            || self.total_bytes > CLIPBOARD_THUMBNAIL_CACHE_BYTES
        {
            if let Some(expired) = self.order.pop_front() {
                if let Some(expired_value) = self.values.remove(&expired) {
                    self.total_bytes = self.total_bytes.saturating_sub(expired_value.bytes);
                    remove_cached_clipboard_image(&expired_value);
                }
            }
        }
    }

    fn clear(&mut self) {
        for value in self.values.values() {
            remove_cached_clipboard_image(value);
        }
        self.order.clear();
        self.values.clear();
        self.total_bytes = 0;
    }
}

impl ClipboardImagePreviewCache {
    fn get(&mut self, id: u64) -> Option<String> {
        let value = self.values.get(&id)?.path.clone();
        self.order.retain(|current| *current != id);
        self.order.push_back(id);
        Some(value)
    }

    fn insert(&mut self, id: u64, value: CachedClipboardImage) {
        if let Some(previous) = self.values.remove(&id) {
            self.total_bytes = self.total_bytes.saturating_sub(previous.bytes);
            remove_cached_clipboard_image(&previous);
        }
        self.order.retain(|current| *current != id);
        if value.bytes > CLIPBOARD_IMAGE_PREVIEW_CACHE_BYTES {
            remove_cached_clipboard_image(&value);
            return;
        }
        self.total_bytes = self.total_bytes.saturating_add(value.bytes);
        self.values.insert(id, value);
        self.order.push_back(id);
        while self.order.len() > CLIPBOARD_IMAGE_PREVIEW_LIMIT
            || self.total_bytes > CLIPBOARD_IMAGE_PREVIEW_CACHE_BYTES
        {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            if let Some(expired_value) = self.values.remove(&expired) {
                self.total_bytes = self.total_bytes.saturating_sub(expired_value.bytes);
                remove_cached_clipboard_image(&expired_value);
            }
        }
    }

    fn clear(&mut self) {
        for value in self.values.values() {
            remove_cached_clipboard_image(value);
        }
        self.order.clear();
        self.values.clear();
        self.total_bytes = 0;
    }
}

fn remove_cached_clipboard_image(value: &CachedClipboardImage) {
    if let Err(error) = std::fs::remove_file(&value.path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::debug!(
                event = "clipboard.preview_cache.remove_failed",
                %error,
                "could not remove cached clipboard image"
            );
        }
    }
}

fn clipboard_preview_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|directory| directory.join("clipboard-previews"))
        .map_err(|error| error.to_string())
}

pub fn prepare_clipboard_preview_cache(app: &AppHandle) -> Result<(), String> {
    let directory = clipboard_preview_directory(app)?;
    if directory.exists() {
        if let Err(error) = std::fs::remove_dir_all(&directory) {
            tracing::warn!(
                event = "clipboard.preview_cache.cleanup_failed",
                %error,
                "could not clear stale clipboard previews"
            );
        }
    }
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    app.asset_protocol_scope()
        .allow_directory(&directory, true)
        .map_err(|error| error.to_string())
}

fn persist_clipboard_image(
    directory: &Path,
    id: u64,
    kind: &str,
    bytes: Vec<u8>,
) -> Result<CachedClipboardImage, String> {
    std::fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let sequence = CLIPBOARD_PREVIEW_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = directory.join(format!("{id}-{kind}-{sequence}.png"));
    std::fs::write(&path, &bytes).map_err(|error| error.to_string())?;
    Ok(CachedClipboardImage {
        path: path.to_string_lossy().into_owned(),
        bytes: bytes.len(),
    })
}

pub fn clear_clipboard_thumbnail_cache() {
    CLIPBOARD_CACHE_GENERATION.fetch_add(1, Ordering::AcqRel);
    if let Some(cache) = CLIPBOARD_THUMBNAILS.get() {
        cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
    if let Some(cache) = CLIPBOARD_IMAGE_PREVIEWS.get() {
        cache
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }
}

#[derive(Debug, Clone, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardCursorInput {
    pub sort_at_ms: i64,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardCursorView {
    pub sort_at_ms: i64,
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItemView {
    pub id: u64,
    pub kind: ClipboardContentKind,
    pub preview: String,
    pub source_app: Option<String>,
    pub first_captured_at_ms: i64,
    pub captured_at_ms: i64,
    pub updated_at_ms: i64,
    pub size_bytes: u64,
    pub character_count: Option<u64>,
    pub item_count: u32,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sensitive: bool,
    pub favorite: bool,
    pub labels: Vec<ClipboardLabel>,
    pub available: bool,
    pub sync_id: String,
    pub source_device_name: Option<String>,
    pub text_syntax: arcrelay_core::domain::clipboard::ClipboardTextSyntax,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardHistoryView {
    pub revision: u64,
    pub entries: Vec<ClipboardItemView>,
    pub next_cursor: Option<ClipboardCursorView>,
    pub total_count: Option<u64>,
}

#[derive(Debug, Clone, serde::Deserialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ClipboardTimelinePositionInput {
    Around { id: u64 },
    Newer { cursor: ClipboardCursorInput },
    Older { cursor: ClipboardCursorInput },
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardTimelineView {
    pub revision: u64,
    pub entries: Vec<ClipboardItemView>,
    pub anchor: Option<ClipboardCursorView>,
    pub newer_cursor: Option<ClipboardCursorView>,
    pub older_cursor: Option<ClipboardCursorView>,
    pub total_count: u64,
}

impl From<ClipboardCursor> for ClipboardCursorView {
    fn from(cursor: ClipboardCursor) -> Self {
        Self {
            sort_at_ms: cursor.sort_at_ms,
            id: cursor.id,
        }
    }
}

impl From<ClipboardSummary> for ClipboardItemView {
    fn from(entry: ClipboardSummary) -> Self {
        Self {
            id: entry.id,
            kind: entry.kind,
            preview: entry.preview,
            source_app: entry.source_app,
            first_captured_at_ms: entry.first_captured_at.timestamp_millis(),
            captured_at_ms: entry.captured_at.timestamp_millis(),
            updated_at_ms: entry
                .last_used_at
                .unwrap_or(entry.captured_at)
                .max(entry.captured_at)
                .timestamp_millis(),
            size_bytes: entry.size_bytes,
            character_count: entry.character_count,
            item_count: entry.item_count,
            width: entry.width,
            height: entry.height,
            sensitive: entry.sensitive,
            favorite: entry.favorite,
            labels: entry.labels,
            available: entry.available,
            sync_id: entry.sync_id,
            source_device_name: entry.source_device_name,
            text_syntax: entry.text_syntax,
        }
    }
}

impl From<ClipboardTimelinePage> for ClipboardTimelineView {
    fn from(page: ClipboardTimelinePage) -> Self {
        Self {
            revision: page.revision,
            entries: page
                .entries
                .into_iter()
                .map(ClipboardItemView::from)
                .collect(),
            anchor: page.anchor.map(ClipboardCursorView::from),
            newer_cursor: page.newer_cursor.map(ClipboardCursorView::from),
            older_cursor: page.older_cursor.map(ClipboardCursorView::from),
            total_count: page.total_count,
        }
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_timeline(
    state: State<'_, DesktopState>,
    position: ClipboardTimelinePositionInput,
    sort_by: ClipboardSortPreference,
    limit: Option<usize>,
) -> Result<Option<ClipboardTimelineView>, String> {
    let cursor = |value: ClipboardCursorInput| ClipboardCursor {
        sort_at_ms: value.sort_at_ms,
        id: value.id,
    };
    let query = ClipboardTimelineQuery {
        position: match position {
            ClipboardTimelinePositionInput::Around { id } => {
                ClipboardTimelinePosition::AroundId(id)
            }
            ClipboardTimelinePositionInput::Newer { cursor: value } => {
                ClipboardTimelinePosition::NewerThan(cursor(value))
            }
            ClipboardTimelinePositionInput::Older { cursor: value } => {
                ClipboardTimelinePosition::OlderThan(cursor(value))
            }
        },
        sort_by: match sort_by {
            ClipboardSortPreference::CreatedAt => ClipboardSortBy::CreatedAt,
            ClipboardSortPreference::UpdatedAt => ClipboardSortBy::UpdatedAt,
        },
        limit: limit.unwrap_or(30).clamp(1, 60),
    };
    state
        .clipboard
        .timeline(query)
        .await
        .map(|page| page.map(ClipboardTimelineView::from))
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_history(
    state: State<'_, DesktopState>,
    search: Option<String>,
    kind: Option<ClipboardContentKind>,
    favorite_only: bool,
    label_ids: Option<Vec<String>>,
    cursor: Option<ClipboardCursorInput>,
    limit: Option<usize>,
) -> Result<ClipboardHistoryView, String> {
    let clipboard = state.clipboard.clone();
    let sort_by = match state.settings.snapshot().clipboard_sort_by {
        ClipboardSortPreference::CreatedAt => ClipboardSortBy::CreatedAt,
        ClipboardSortPreference::UpdatedAt => ClipboardSortBy::UpdatedAt,
    };
    let query = ClipboardQuery {
        include_total_count: cursor.is_none(),
        limit: limit.unwrap_or(60).clamp(1, 120),
        cursor: cursor.map(|value| ClipboardCursor {
            sort_at_ms: value.sort_at_ms,
            id: value.id,
        }),
        search: search.filter(|value| !value.trim().is_empty()),
        kinds: kind.into_iter().collect(),
        favorite_only,
        label_ids: label_ids.unwrap_or_default(),
        sort_by,
    };
    let (revision, page) = tokio::try_join!(clipboard.revision(), clipboard.history(query))
        .map_err(|error| error.to_string())?;
    Ok(ClipboardHistoryView {
        revision,
        entries: page
            .entries
            .into_iter()
            .map(ClipboardItemView::from)
            .collect(),
        next_cursor: page.next_cursor.map(|value| ClipboardCursorView {
            sort_at_ms: value.sort_at_ms,
            id: value.id,
        }),
        total_count: page.total_count,
    })
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_thumbnail(
    app: AppHandle,
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<Option<String>, String> {
    let cache = CLIPBOARD_THUMBNAILS.get_or_init(|| Mutex::new(ClipboardThumbnailCache::default()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(id)
    {
        return Ok(Some(value));
    }
    let generation = CLIPBOARD_CACHE_GENERATION.load(Ordering::Acquire);
    let Some(png) = state
        .clipboard
        .image_png(id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let directory = clipboard_preview_directory(&app)?;
    let cached = tauri::async_runtime::spawn_blocking(move || {
        let encoded = build_clipboard_thumbnail(&png)?;
        persist_clipboard_image(&directory, id, "thumbnail", encoded)
    })
    .await
    .map_err(|error| error.to_string())??;
    let path = cached.path.clone();
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if generation != CLIPBOARD_CACHE_GENERATION.load(Ordering::Acquire) {
        remove_cached_clipboard_image(&cached);
        return Ok(None);
    }
    cache.insert(id, cached);
    Ok(Some(path))
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_image_preview(
    app: AppHandle,
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<Option<String>, String> {
    let cache =
        CLIPBOARD_IMAGE_PREVIEWS.get_or_init(|| Mutex::new(ClipboardImagePreviewCache::default()));
    if let Some(value) = cache
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(id)
    {
        return Ok(Some(value));
    }
    let generation = CLIPBOARD_CACHE_GENERATION.load(Ordering::Acquire);
    let Some(png) = state
        .clipboard
        .image_png(id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let directory = clipboard_preview_directory(&app)?;
    let cached = tauri::async_runtime::spawn_blocking(move || {
        let encoded = build_clipboard_image_preview(&png)?;
        persist_clipboard_image(&directory, id, "preview", encoded)
    })
    .await
    .map_err(|error| error.to_string())??;
    let path = cached.path.clone();
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    if generation != CLIPBOARD_CACHE_GENERATION.load(Ordering::Acquire) {
        remove_cached_clipboard_image(&cached);
        return Ok(None);
    }
    cache.insert(id, cached);
    Ok(Some(path))
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_image_ocr(
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<Option<ClipboardImageOcr>, String> {
    state
        .clipboard
        .image_ocr(id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_html_preview(
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<Option<String>, String> {
    // The UI cache is keyed by content revision. Avoid a second cache keyed
    // only by ID, which would serve pre-edit HTML after synchronized updates.
    state
        .clipboard
        .safe_html_preview(id)
        .await
        .map_err(|error| error.to_string())
}

fn build_clipboard_thumbnail(png: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(png).map_err(|error| error.to_string())?;
    let thumbnail = image.thumbnail(720, 360).to_rgba8();
    encode_png(thumbnail)
}

fn build_clipboard_image_preview(png: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(png).map_err(|error| error.to_string())?;
    let preview = if image.width() > CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION
        || image.height() > CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION
    {
        image.resize(
            CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION,
            CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image
    }
    .to_rgba8();
    encode_png(preview)
}

fn encode_png(image: RgbaImage) -> Result<Vec<u8>, String> {
    let (width, height) = image.dimensions();
    let mut encoded = Vec::new();
    image::codecs::png::PngEncoder::new(&mut encoded)
        .write_image(
            image.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok(encoded)
}

#[cfg(test)]
mod clipboard_image_preview_tests {
    use super::*;

    fn source_png(width: u32, height: u32) -> Vec<u8> {
        let image = RgbaImage::from_pixel(width, height, image::Rgba([36, 94, 201, 255]));
        encode_png(image).expect("encode source image")
    }

    fn decoded_dimensions(bytes: &[u8]) -> (u32, u32) {
        let image = image::load_from_memory(bytes).expect("load preview");
        (image.width(), image.height())
    }

    #[test]
    fn high_resolution_preview_keeps_source_resolution_below_limit() {
        let png = source_png(1_600, 900);
        let thumbnail = build_clipboard_thumbnail(&png).expect("build thumbnail");
        let preview = build_clipboard_image_preview(&png).expect("build image preview");

        assert_eq!(decoded_dimensions(&thumbnail), (640, 360));
        assert_eq!(decoded_dimensions(&preview), (1_600, 900));
    }

    #[test]
    fn high_resolution_preview_bounds_oversized_images() {
        let png = source_png(4_200, 420);
        let preview = build_clipboard_image_preview(&png).expect("build image preview");
        let (width, height) = decoded_dimensions(&preview);

        assert_eq!(width, CLIPBOARD_IMAGE_PREVIEW_MAX_DIMENSION);
        assert!(height <= 410);
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_copy_record(state: State<'_, DesktopState>, id: u64) -> Result<(), String> {
    state
        .clipboard
        .copy_record(id)
        .await
        .map_err(|error| error.to_string())?;
    crate::sound::play(crate::sound::SoundEvent::ClipboardUsed);
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_copy_text(
    state: State<'_, DesktopState>,
    content: String,
) -> Result<(), String> {
    if content.is_empty() {
        return Err("cannot copy empty text".to_string());
    }
    state
        .clipboard
        .set_text(&content)
        .await
        .map_err(|error| error.to_string())?;
    crate::sound::play(crate::sound::SoundEvent::ClipboardUsed);
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_paste_text(
    state: State<'_, DesktopState>,
    app: AppHandle,
    content: String,
) -> Result<(), String> {
    if content.is_empty() {
        return Err("cannot paste empty text".to_string());
    }
    require_input_permission(&state, &app).await?;
    state
        .clipboard
        .set_text(&content)
        .await
        .map_err(|error| error.to_string())?;
    prepare_window_and_wait_for_paste(&app).await?;
    state
        .clipboard
        .paste_prepared(ClipboardContentKind::Text)
        .await
        .map_err(|error| error.to_string())?;
    crate::sound::play(crate::sound::SoundEvent::ClipboardUsed);
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_paste_record(
    state: State<'_, DesktopState>,
    app: AppHandle,
    id: u64,
) -> Result<(), String> {
    paste_clipboard_record(
        &state,
        &app,
        id,
        arcrelay_core::domain::clipboard::ClipboardPasteMode::Source,
    )
    .await
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_paste_record_as(
    state: State<'_, DesktopState>,
    app: AppHandle,
    id: u64,
    mode: arcrelay_core::domain::clipboard::ClipboardPasteMode,
) -> Result<(), String> {
    paste_clipboard_record(&state, &app, id, mode).await
}

async fn paste_clipboard_record(
    state: &DesktopState,
    app: &AppHandle,
    id: u64,
    mode: arcrelay_core::domain::clipboard::ClipboardPasteMode,
) -> Result<(), String> {
    require_input_permission(state, app).await?;
    // Resolve conversions such as image OCR before hiding the panel. If
    // preparation fails, the caller can keep the error visible in the window.
    let kind = state
        .clipboard
        .prepare_record_as(id, mode)
        .await
        .map_err(|error| error.to_string())?;
    prepare_window_and_wait_for_paste(app).await?;
    state
        .clipboard
        .paste_prepared(kind)
        .await
        .map_err(|error| error.to_string())?;
    crate::sound::play(crate::sound::SoundEvent::ClipboardUsed);
    Ok(())
}

async fn prepare_window_and_wait_for_paste(app: &AppHandle) -> Result<(), String> {
    crate::windowing::prepare_clipboard_window_for_paste(app).map_err(|error| error.to_string())?;
    // On macOS the NSPanel may have become key after a click into the search
    // field. Give AppKit time to resign it and restore the previous app before
    // emitting the synthetic paste shortcut. A pinned panel stays visible.
    #[cfg(target_os = "macos")]
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    #[cfg(not(target_os = "macos"))]
    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_paste_records(
    state: State<'_, DesktopState>,
    app: AppHandle,
    ids: Vec<u64>,
) -> Result<usize, String> {
    if ids.is_empty() {
        return Ok(0);
    }
    require_input_permission(&state, &app).await?;

    let first_kind = state
        .clipboard
        .prepare_record_as(
            ids[0],
            arcrelay_core::domain::clipboard::ClipboardPasteMode::Source,
        )
        .await
        .map_err(|error| error.to_string())?;
    prepare_window_and_wait_for_paste(&app).await?;
    state
        .clipboard
        .paste_prepared(first_kind)
        .await
        .map_err(|error| error.to_string())?;

    for id in ids.iter().skip(1) {
        tokio::time::sleep(std::time::Duration::from_millis(90)).await;
        let kind = state
            .clipboard
            .prepare_record_as(
                *id,
                arcrelay_core::domain::clipboard::ClipboardPasteMode::Source,
            )
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        state
            .clipboard
            .paste_prepared(kind)
            .await
            .map_err(|error| error.to_string())?;
    }
    crate::sound::play(crate::sound::SoundEvent::ClipboardUsed);
    Ok(ids.len())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_start_continuous_paste(
    app: AppHandle,
    ids: Vec<u64>,
) -> Result<ContinuousPasteProgress, String> {
    if ids.is_empty() {
        return Err("select at least one clipboard entry".to_string());
    }
    {
        let mut queue = continuous_paste_queue()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.ids = ids;
        queue.index = 0;
        queue.in_flight = false;
    }
    crate::windowing::prepare_clipboard_window_for_paste(&app)
        .map_err(|error| error.to_string())?;
    Ok(continuous_paste_progress())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_stop_continuous_paste() -> ContinuousPasteProgress {
    let mut queue = continuous_paste_queue()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.ids.clear();
    queue.index = 0;
    queue.in_flight = false;
    ContinuousPasteProgress {
        current: 0,
        total: 0,
        active: false,
    }
}

pub fn clipboard_continuous_paste_active() -> bool {
    let queue = continuous_paste_queue()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    queue.index < queue.ids.len()
}

fn continuous_paste_progress() -> ContinuousPasteProgress {
    let queue = continuous_paste_queue()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    ContinuousPasteProgress {
        current: queue.index,
        total: queue.ids.len(),
        active: queue.index < queue.ids.len(),
    }
}

pub async fn paste_next_continuous_record(
    app: AppHandle,
) -> Result<Option<ContinuousPasteProgress>, String> {
    let id = {
        let mut queue = continuous_paste_queue()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if queue.in_flight {
            return Ok(None);
        }
        let id = queue.ids.get(queue.index).copied();
        queue.in_flight = id.is_some();
        id
    };
    let Some(id) = id else {
        return Ok(None);
    };

    let state = app.state::<DesktopState>();
    if let Err(error) = paste_clipboard_record(
        &state,
        &app,
        id,
        arcrelay_core::domain::clipboard::ClipboardPasteMode::Source,
    )
    .await
    {
        continuous_paste_queue()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .in_flight = false;
        return Err(error);
    }

    let progress = {
        let mut queue = continuous_paste_queue()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        queue.index = queue.index.saturating_add(1).min(queue.ids.len());
        queue.in_flight = false;
        let progress = ContinuousPasteProgress {
            current: queue.index,
            total: queue.ids.len(),
            active: queue.index < queue.ids.len(),
        };
        if !progress.active {
            queue.ids.clear();
            queue.index = 0;
        }
        progress
    };
    let _ = app.emit("clipboard-continuous-paste-progress", &progress);
    Ok(Some(progress))
}

async fn require_input_permission(state: &DesktopState, app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let permission = refresh_input_permission_state(state).await?;
        if permission == InputPermissionState::Granted {
            return Ok(());
        }
        crate::windowing::ensure_permission_guide_window(app).map_err(|error| error.to_string())?;
        Err("accessibility permission is required for this operation".to_string())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, app);
        Ok(())
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_delete_record(
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<(), String> {
    state
        .clipboard
        .delete(id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_set_favorite(
    state: State<'_, DesktopState>,
    id: u64,
    favorite: bool,
) -> Result<(), String> {
    state
        .clipboard
        .set_favorite(id, favorite)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_labels(
    state: State<'_, DesktopState>,
) -> Result<Vec<ClipboardLabel>, String> {
    state
        .clipboard
        .labels()
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_create_label(
    state: State<'_, DesktopState>,
    name: String,
    color: String,
) -> Result<ClipboardLabel, String> {
    state
        .clipboard
        .create_label(&name, &color)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_update_label(
    state: State<'_, DesktopState>,
    label_id: String,
    name: String,
    color: String,
) -> Result<(), String> {
    state
        .clipboard
        .update_label(&label_id, &name, &color)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_delete_label(
    state: State<'_, DesktopState>,
    label_id: String,
) -> Result<(), String> {
    state
        .clipboard
        .delete_label(&label_id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_set_labels(
    state: State<'_, DesktopState>,
    id: u64,
    label_ids: Vec<String>,
) -> Result<(), String> {
    state
        .clipboard
        .set_labels(id, label_ids)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_set_label_membership(
    state: State<'_, DesktopState>,
    id: u64,
    label_id: String,
    attached: bool,
) -> Result<(), String> {
    state
        .clipboard
        .set_label_membership(id, &label_id, attached)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub fn set_clipboard_context_menu_open(open: bool) {
    crate::windowing::set_clipboard_context_menu_open(open);
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_edit_text(
    state: State<'_, DesktopState>,
    id: u64,
    content: String,
) -> Result<(), String> {
    state
        .clipboard
        .edit_text(id, content.trim_end())
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_text_content(
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<String, String> {
    state
        .clipboard
        .text_content(id)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_merge_devices(state: State<'_, DesktopState>) -> Result<usize, String> {
    state.clipboard_sync.merge_all().await
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_send_files(
    state: State<'_, DesktopState>,
    id: u64,
    peer_id: String,
) -> Result<String, String> {
    let paths = state
        .clipboard
        .file_paths(id)
        .await
        .map_err(|error| error.to_string())?;
    state
        .transfer()
        .await?
        .clone()
        .send_files(
            peer_id,
            paths.into_iter().map(std::path::PathBuf::from).collect(),
        )
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_clear_history(state: State<'_, DesktopState>) -> Result<(), String> {
    state
        .clipboard
        .clear_history()
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn hide_clipboard_window(app: AppHandle) -> Result<(), String> {
    crate::windowing::hide_clipboard_window(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn get_clipboard_window_pinned() -> bool {
    crate::windowing::clipboard_window_pinned()
}

#[arcrelay_desktop_ipc::command]
pub async fn set_clipboard_window_pinned(app: AppHandle, pinned: bool) -> Result<(), String> {
    crate::windowing::set_clipboard_window_pinned(&app, pinned).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn start_clipboard_window_drag(app: AppHandle) -> Result<(), String> {
    crate::windowing::start_clipboard_window_drag(&app).map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn clipboard_text_segments(
    state: State<'_, DesktopState>,
    id: u64,
) -> Result<arcrelay_core::domain::text_slices::TextSliceModel, String> {
    let generation = state.text_selection.generation();
    let text = state
        .clipboard
        .text_content(id)
        .await
        .map_err(|error| error.to_string())?;
    let service = state.text_selection.clone();
    tauri::async_runtime::spawn_blocking(move || {
        service.model(id, &text, generation).as_ref().clone()
    })
    .await
    .map_err(|error| error.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn clipboard_join_segments(
    state: State<'_, DesktopState>,
    id: u64,
    version: String,
    ids: Vec<String>,
) -> Result<String, crate::ipc::IpcError> {
    use crate::ipc::IntoIpcError as _;

    let text = state
        .clipboard
        .text_content(id)
        .await
        .map_err(crate::ipc::IntoIpcError::into_ipc_error)?;
    state
        .text_selection
        .join(id, &version, &text, &ids)
        .map_err(|error| error.into_ipc_error())
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_clipboard_ipc.rs"));
