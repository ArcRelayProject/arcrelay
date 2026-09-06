//! One-time, restartable retirement of ArcFlow data, without loading its runtime.
use std::{io, path::Path};

pub fn archive_legacy_database(directory: &Path) -> io::Result<()> {
    let archive = directory.join("retired/arcflow-v1");
    let names = [
        "workflows.sqlite3",
        "workflows.sqlite3-wal",
        "workflows.sqlite3-shm",
        "workflows.sqlite3-journal",
    ];
    let mut present = Vec::new();
    for name in names {
        let source = directory.join(name);
        match std::fs::symlink_metadata(&source) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                if std::fs::symlink_metadata(archive.join(name)).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "retired workflow archive already contains this file",
                    ));
                }
                present.push(name);
            }
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "retired workflow source is not a regular file",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if present.is_empty() && !archive.exists() {
        return Ok(());
    }
    for path in [directory.join("retired"), archive.clone()] {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "retired archive is not a regular directory",
                ))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    std::fs::create_dir_all(&archive)?;
    for name in present {
        std::fs::rename(directory.join(name), archive.join(name))?;
    }
    // A crash during renames is resumable: already moved files are absent from
    // the source on the next launch, and destinations are never overwritten.
    let marker = archive.join("retirement-v1.json");
    if let Ok(metadata) = std::fs::symlink_metadata(&marker) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "retirement receipt is not a regular file",
            ));
        }
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(marker)?;
    use std::io::Write;
    file.write_all(b"{\"version\":1,\"reason\":\"ArcFlow retired; current automations use a separate store\"}\n")?;
    file.sync_all()?;
    #[cfg(unix)]
    {
        std::fs::File::open(&archive)?.sync_all()?;
        std::fs::File::open(directory)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retirement_needs_no_runtime_and_preserves_current_data() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let directory =
            std::env::temp_dir().join(format!("arcrelay-retire-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        for name in [
            "workflows.sqlite3",
            "workflows.sqlite3-wal",
            "workflows.sqlite3-shm",
            "workflows.sqlite3-journal",
            "automations.sqlite3",
            "actions.json",
            "clipboard.sqlite3",
            "workflows-other.sqlite3",
        ] {
            std::fs::write(directory.join(name), name).unwrap();
        }
        archive_legacy_database(&directory).unwrap();
        archive_legacy_database(&directory).unwrap();
        assert!(!directory.join("workflows.sqlite3").exists());
        assert!(!directory.join("workflows.sqlite3-wal").exists());
        assert!(!directory.join("workflows.sqlite3-shm").exists());
        assert!(!directory.join("workflows.sqlite3-journal").exists());
        for name in [
            "automations.sqlite3",
            "actions.json",
            "clipboard.sqlite3",
            "workflows-other.sqlite3",
        ] {
            assert_eq!(std::fs::read_to_string(directory.join(name)).unwrap(), name);
            std::fs::remove_file(directory.join(name)).unwrap();
        }
        for name in [
            "workflows.sqlite3",
            "workflows.sqlite3-wal",
            "workflows.sqlite3-shm",
            "workflows.sqlite3-journal",
        ] {
            assert_eq!(
                std::fs::read_to_string(directory.join("retired/arcflow-v1").join(name)).unwrap(),
                name
            );
        }
        std::fs::remove_dir_all(directory.join("retired")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn retirement_resumes_after_the_database_was_already_moved() {
        let directory =
            std::env::temp_dir().join(format!("arcrelay-retire-{}", uuid::Uuid::new_v4()));
        let archive = directory.join("retired/arcflow-v1");
        std::fs::create_dir_all(&archive).unwrap();
        std::fs::write(archive.join("workflows.sqlite3"), "database").unwrap();
        std::fs::write(directory.join("workflows.sqlite3-wal"), "wal").unwrap();
        archive_legacy_database(&directory).unwrap();
        assert_eq!(
            std::fs::read_to_string(archive.join("workflows.sqlite3-wal")).unwrap(),
            "wal"
        );
        std::fs::remove_file(archive.join("retirement-v1.json")).unwrap();
        archive_legacy_database(&directory).unwrap();
        assert!(archive.join("retirement-v1.json").is_file());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn retirement_does_not_recursively_remove_unexpected_directories() {
        let directory =
            std::env::temp_dir().join(format!("arcrelay-retire-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(directory.join("workflows.sqlite3")).unwrap();
        assert!(archive_legacy_database(&directory).is_err());
        assert!(directory.join("workflows.sqlite3").is_dir());
        std::fs::remove_dir(directory.join("workflows.sqlite3")).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
