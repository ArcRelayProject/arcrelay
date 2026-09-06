#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Durably replaces `path` with `bytes` without exposing a partially-written
/// destination. The temporary file always lives beside the destination so the
/// final replace stays on the same filesystem.
pub fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace_inner(path, bytes, false)
}

/// Owner-only files, including temporary files created during replacement.
pub fn replace_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    replace_inner(path, bytes, true)
}

fn replace_inner(path: &Path, bytes: &[u8], private: bool) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;

    let temporary = temporary_path(path);
    let result = (|| {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        let mut output = options.open(&temporary)?;
        output.write_all(bytes)?;
        output.sync_all()?;
        drop(output);

        replace_path(&temporary, path)?;
        sync_parent(parent)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("arcrelay-data");
    path.with_file_name(format!(".{file_name}.{}.tmp", uuid::Uuid::new_v4()))
}

#[cfg(target_os = "windows")]
fn replace_path(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    let temporary = wide(temporary);
    let destination = wide(destination);
    unsafe {
        MoveFileExW(
            PCWSTR(temporary.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(std::io::Error::other)
}

#[cfg(not(target_os = "windows"))]
fn replace_path(temporary: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(temporary, destination)
}

#[cfg(unix)]
fn sync_parent(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent(_parent: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeatedly_replaces_existing_file() {
        let directory = std::env::temp_dir().join(format!(
            "arcrelay-durable-file-test-{}",
            uuid::Uuid::new_v4()
        ));
        let path = directory.join("settings.json");

        replace(&path, b"first").unwrap();
        replace(&path, b"second").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        let _ = std::fs::remove_dir_all(directory);
    }
}
