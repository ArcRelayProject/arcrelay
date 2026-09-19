//! Export files have a different lifetime from disposable UI thumbnails.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;

pub(super) struct ExportFile {
    pub extension: &'static str,
    pub bytes: Vec<u8>,
}

pub(super) struct ExportLease {
    directory: PathBuf,
    offered: Arc<AtomicBool>,
}

impl ExportLease {
    /// Set by the native adapter immediately before it offers data to the OS.
    pub fn handoff_flag(&self) -> Arc<AtomicBool> {
        self.offered.clone()
    }

    /// Run under the cache gate on a blocking worker before releasing the
    /// active lease. Receivers get a full retention window after completion.
    pub fn retain_after_handoff(&self) -> Result<(), String> {
        if !self.offered.load(Ordering::Acquire) {
            return Ok(());
        }
        let temporary = self
            .directory
            .join(format!(".expires-{}", uuid::Uuid::new_v4()));
        write_private(
            &temporary,
            epoch_seconds(SystemTime::now() + RETENTION)
                .to_string()
                .as_bytes(),
        )?;
        fs::rename(&temporary, self.directory.join(".expires"))
            .map_err(|_| "could not update clipboard export retention".to_string())
    }
}

impl Drop for ExportLease {
    fn drop(&mut self) {
        if !self.offered.load(Ordering::Acquire) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

fn epoch_seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn private_directory(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("clipboard export directory cannot be a symbolic link".into());
    }
    fs::create_dir_all(path).map_err(|_| "could not create clipboard export directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| "could not protect clipboard export directory")?;
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "could not create clipboard export file")?;
    file.write_all(bytes)
        .map_err(|_| "could not write clipboard export file".into())
}

/// Called only on a blocking worker. Cleanup never evicts a recently offered
/// file to make room: another process may still own the received pathname.
pub(super) fn collect(root: &Path, now: SystemTime, protected: &[PathBuf]) -> Result<u64, String> {
    private_directory(root)?;
    let mut bytes = 0u64;
    for entry in fs::read_dir(root).map_err(|_| "could not read clipboard export cache")? {
        let entry = entry.map_err(|_| "could not read clipboard export cache")?;
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("could not inspect clipboard export cache".into()),
        };
        if !kind.is_dir() || uuid::Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
            continue;
        }
        let directory = entry.path();
        let expiry = fs::read_to_string(directory.join(".expires"))
            .ok()
            .and_then(|text| text.parse::<u64>().ok());
        // Crash before the marker was written: age the directory itself.
        let expired = expiry.map_or_else(
            || {
                entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .ok()
                    .and_then(|time| now.duration_since(time).ok())
                    .is_some_and(|age| age > RETENTION)
            },
            |expires| epoch_seconds(now) >= expires,
        );
        if expired && !protected.contains(&directory) {
            match fs::remove_dir_all(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("could not clean expired clipboard exports".into()),
            }
            continue;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err("could not inspect clipboard export files".into()),
        };
        for file in entries {
            let file = match file {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("could not inspect clipboard export files".into()),
            };
            // Do not follow symlinks inside a cache directory.
            let metadata = match fs::symlink_metadata(file.path()) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err("could not inspect clipboard export file".into()),
            };
            if metadata.is_file() {
                bytes = bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(bytes)
}

pub(super) fn materialize(
    root: &Path,
    files: Vec<ExportFile>,
    protected: &[PathBuf],
) -> Result<(Vec<PathBuf>, Option<ExportLease>), String> {
    if files.is_empty() {
        return Ok((vec![], None));
    }
    let now = SystemTime::now();
    let used = collect(root, now, protected)?;
    let incoming = files
        .iter()
        .map(|file| file.bytes.len() as u64)
        .sum::<u64>();
    if used.saturating_add(incoming) > MAX_CACHE_BYTES {
        return Err("clipboard export cache is full; try again after older exports expire".into());
    }
    let directory = root.join(uuid::Uuid::new_v4().to_string());
    private_directory(&directory)?;
    let lease = ExportLease {
        directory: directory.clone(),
        offered: Arc::new(AtomicBool::new(false)),
    };
    write_private(
        &directory.join(".expires"),
        epoch_seconds(now + RETENTION).to_string().as_bytes(),
    )?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut paths = Vec::with_capacity(files.len());
    for (index, file) in files.into_iter().enumerate() {
        let name = format!("Clipboard-{stamp}-{}.{}", index + 1, file.extension);
        let temporary = directory.join(format!("{}.part", index + 1));
        write_private(&temporary, &file.bytes)?;
        let path = directory.join(name);
        fs::rename(temporary, &path).map_err(|_| "could not finalize clipboard export file")?;
        paths.push(path);
    }
    Ok((paths, Some(lease)))
}

impl ExportLease {
    pub fn directory(&self) -> PathBuf {
        self.directory.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_bytes_are_preserved_and_unoffered_files_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let bytes = vec![0, 255, 42, 13, 10];
        let (paths, lease) = materialize(
            root.path(),
            vec![ExportFile {
                extension: "png",
                bytes: bytes.clone(),
            }],
            &[],
        )
        .unwrap();
        assert_eq!(fs::read(&paths[0]).unwrap(), bytes);
        assert_eq!(paths[0].extension().unwrap(), "png");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&paths[0]).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(lease);
        assert!(!paths[0].exists());
    }

    #[test]
    fn drop_keeps_offered_files_for_delayed_read_and_cleanup_respects_active_lease() {
        let root = tempfile::tempdir().unwrap();
        let (paths, lease) = materialize(
            root.path(),
            vec![ExportFile {
                extension: "txt",
                bytes: "晚些读取".as_bytes().to_vec(),
            }],
            &[],
        )
        .unwrap();
        let lease = lease.unwrap();
        lease.handoff_flag().store(true, Ordering::Release);
        let protected = vec![lease.directory()];
        let future = SystemTime::now() + RETENTION + Duration::from_secs(10);
        collect(root.path(), future, &protected).unwrap();
        assert_eq!(fs::read_to_string(&paths[0]).unwrap(), "晚些读取");
        drop(lease);
        assert!(paths[0].exists());
        collect(root.path(), future, &[]).unwrap();
        assert!(!paths[0].exists());
    }

    #[test]
    fn native_handoff_extends_retention_from_completion() {
        let root = tempfile::tempdir().unwrap();
        let (paths, lease) = materialize(
            root.path(),
            vec![ExportFile {
                extension: "txt",
                bytes: b"delayed reader".to_vec(),
            }],
            &[],
        )
        .unwrap();
        let lease = lease.unwrap();
        let marker = lease.directory().join(".expires");
        fs::write(&marker, b"1").unwrap();
        // A cancelled start has never offered a path to another process.
        lease.retain_after_handoff().unwrap();
        assert_eq!(fs::read_to_string(&marker).unwrap(), "1");
        lease.handoff_flag().store(true, Ordering::Release);
        let completed = SystemTime::now();
        lease.retain_after_handoff().unwrap();
        drop(lease);
        collect(
            root.path(),
            completed + RETENTION - Duration::from_secs(2),
            &[],
        )
        .unwrap();
        assert_eq!(fs::read(&paths[0]).unwrap(), b"delayed reader");
        collect(
            root.path(),
            completed + RETENTION + Duration::from_secs(2),
            &[],
        )
        .unwrap();
        assert!(!paths[0].exists());
    }

    #[test]
    fn filenames_are_distinct_even_when_extensions_match() {
        let root = tempfile::tempdir().unwrap();
        let (paths, _lease) = materialize(
            root.path(),
            vec![
                ExportFile {
                    extension: "txt",
                    bytes: b"first".to_vec(),
                },
                ExportFile {
                    extension: "txt",
                    bytes: b"second".to_vec(),
                },
            ],
            &[],
        )
        .unwrap();
        assert_ne!(paths[0], paths[1]);
        assert_eq!(fs::read(&paths[1]).unwrap(), b"second");
    }
}
