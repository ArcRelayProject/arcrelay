//! Local-only staging metadata. This module never initializes the transfer service,
//! hashes a file, opens a connection, or grants receiving permissions.
use std::{fs::File, io::BufReader, path::Path};

use arcrelay_transfer::FileKind;
use base64::Engine as _;
use serde::Serialize;

const MAX_FILES: usize = 256;
const MAX_THUMBNAILS: usize = 32;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct TransferDraftFile {
    pub path: String,
    pub canonical_path: Option<String>,
    pub name: String,
    pub size: Option<u64>,
    pub modified_at_ms: Option<u64>,
    pub kind: FileKind,
    pub thumbnail_data_url: Option<String>,
    pub error: Option<String>,
}

pub fn inspect_paths(paths: Vec<String>) -> Result<Vec<TransferDraftFile>, String> {
    if paths.len() > MAX_FILES {
        return Err("at most 256 files can be added at once".into());
    }
    Ok(paths
        .into_iter()
        .enumerate()
        .map(|(index, value)| inspect_file(value, index < MAX_THUMBNAILS))
        .collect())
}

fn inspect_file(value: String, preview: bool) -> TransferDraftFile {
    let path = Path::new(&value);
    let kind = match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "heic" | "avif" => FileKind::Image,
        "pdf" => FileKind::Pdf,
        "zip" | "rar" | "7z" | "tar" | "gz" => FileKind::Archive,
        _ => FileKind::File,
    };
    let mut result = TransferDraftFile {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        path: value.clone(),
        canonical_path: None,
        size: None,
        modified_at_ms: None,
        kind,
        thumbnail_data_url: None,
        error: None,
    };
    let metadata = (|| -> Result<_, String> {
        let metadata = std::fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        // Match the existing transfer service: directories and symlinks are not
        // sendable. Never silently expand directories or follow a leaf symlink.
        if metadata.file_type().is_symlink() {
            return Err("symbolic links are unsupported; add the original file".into());
        }
        if !metadata.is_file() {
            return Err(
                "only regular files are supported; archive folders before adding them".into(),
            );
        }
        let canonical = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
        let file = File::open(path).map_err(|error| error.to_string())?;
        Ok((metadata, canonical, file))
    })();
    match metadata {
        Ok((metadata, canonical, file)) => {
            result.canonical_path = Some(canonical.to_string_lossy().into_owned());
            result.size = Some(metadata.len());
            result.modified_at_ms = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64);
            if preview && kind == FileKind::Image && metadata.len() <= MAX_IMAGE_BYTES {
                // A missing/unsupported preview does not make a readable file invalid.
                result.thumbnail_data_url = thumbnail(file);
            }
        }
        Err(error) => result.error = Some(error),
    }
    result
}

fn thumbnail(file: File) -> Option<String> {
    let mut reader = image::ImageReader::new(BufReader::new(file))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let thumbnail = reader.decode().ok()?.thumbnail(192, 144).to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 75)
        .encode_image(&thumbnail)
        .ok()?;
    (bytes.len() <= 48 * 1024).then(|| {
        format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_reads_metadata_without_a_runtime_or_transfer_service() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plan.pdf");
        std::fs::write(&path, b"sample").unwrap();
        let result = inspect_paths(vec![path.to_string_lossy().into_owned()]).unwrap();
        assert_eq!(result[0].name, "plan.pdf");
        assert_eq!(result[0].size, Some(6));
        assert_eq!(result[0].kind, FileKind::Pdf);
        assert!(result[0].canonical_path.is_some());
        assert!(result[0].error.is_none());
    }

    #[test]
    fn invalid_entries_are_reported_individually_and_do_not_hide_valid_files() {
        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("notes.txt");
        std::fs::write(&valid, b"hello").unwrap();
        let results = inspect_paths(vec![
            directory.path().to_string_lossy().into_owned(),
            directory
                .path()
                .join("missing")
                .to_string_lossy()
                .into_owned(),
            valid.to_string_lossy().into_owned(),
        ])
        .unwrap();
        assert!(results[0].error.is_some());
        assert!(results[1].error.is_some());
        assert!(results[2].error.is_none());
        assert!(inspect_paths(vec![String::new(); 257]).is_err());
    }

    #[test]
    fn malformed_images_remain_sendable_without_a_preview() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        std::fs::write(&path, b"not an image").unwrap();
        let result = inspect_file(path.to_string_lossy().into_owned(), true);
        assert!(result.error.is_none());
        assert!(result.thumbnail_data_url.is_none());
    }

    #[test]
    fn supported_images_get_a_bounded_optional_preview() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png");
        image::RgbImage::new(400, 300).save(&path).unwrap();
        let result = inspect_file(path.to_string_lossy().into_owned(), true);
        let data = result.thumbnail_data_url.unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert!(decoded.width() <= 192 && decoded.height() <= 144);
        assert!(bytes.len() <= 48 * 1024);
        assert!(result.error.is_none());
        assert!(inspect_file(path.to_string_lossy().into_owned(), false)
            .thumbnail_data_url
            .is_none());
    }

    #[cfg(unix)]
    #[test]
    fn leaf_symlinks_are_not_silently_resolved_into_sendable_files() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("original");
        let link = directory.path().join("link");
        std::fs::write(&original, b"private").unwrap();
        std::os::unix::fs::symlink(original, &link).unwrap();
        assert!(inspect_file(link.to_string_lossy().into_owned(), false)
            .error
            .is_some());
    }
}
