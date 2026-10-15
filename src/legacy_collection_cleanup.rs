//! One-time removal of application-owned, unreleased vault resources. No import.
use std::path::Path;

pub fn run() {
    let Some(root) = dirs::config_dir().map(|p| p.join("ArcRelay")) else {
        return;
    };
    if let Err(error) = clean(&root) {
        tracing::warn!(event = "clipboard.collections.legacy_cleanup_failed", kind = ?error.kind(), "old application-owned resources remain pending cleanup");
    }
}

fn clean(root: &Path) -> std::io::Result<()> {
    let marker = root.join("collections-v1-cleanup.done");
    if marker.exists() {
        return Ok(());
    }
    let device = root.join("login-device.json");
    if std::fs::symlink_metadata(&device).is_ok_and(|m| m.file_type().is_file()) {
        let id = std::fs::read_to_string(&device)?;
        if uuid::Uuid::parse_str(&id).is_ok() {
            #[cfg(target_os = "macos")]
            remove_keychain(&id)?;
            #[cfg(target_os = "windows")]
            remove(&root.join(format!("login-hello-{id}.key")))?;
        }
    }
    // These exact names belonged solely to the removed vault implementation.
    for name in ["logins.arclogin", "logins.lock", "login-device.json"] {
        remove(&root.join(name))?;
    }
    std::fs::create_dir_all(root)?;
    std::fs::write(marker, b"1")
}
fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
#[cfg(target_os = "macos")]
fn remove_keychain(id: &str) -> std::io::Result<()> {
    use security_framework::passwords::{self, PasswordOptions};
    if let Err(e) = passwords::delete_generic_password("cn.arcrelay.login.system-verification", id)
    {
        if e.code() != -25300 {
            return Err(std::io::Error::other("old key cleanup failed"));
        }
    }
    let mut options = PasswordOptions::new_generic_password("cn.arcrelay.login.user-presence", id);
    options.use_protected_keychain();
    options.set_access_synchronized(Some(false));
    if let Err(e) = passwords::delete_generic_password_options(options) {
        if !matches!(e.code(), -25300 | -34018) {
            return Err(std::io::Error::other("old protected key cleanup failed"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_is_runtime_free_idempotent_and_does_not_touch_exports() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "logins.arclogin",
            "logins.lock",
            "export.arclogin",
            "clipboard.sqlite",
        ] {
            std::fs::write(dir.path().join(name), b"test").unwrap();
        }
        clean(dir.path()).unwrap();
        clean(dir.path()).unwrap();
        assert!(!dir.path().join("logins.arclogin").exists());
        assert!(dir.path().join("export.arclogin").exists());
        assert!(dir.path().join("clipboard.sqlite").exists());
    }
}
