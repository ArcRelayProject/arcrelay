use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum AppUpdateSource {
    Github,
    // Keep the cross-platform IPC contract stable; only Windows constructs this.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    MicrosoftStore,
}

impl AppUpdateSource {
    pub fn current() -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        {
            // Store builds remain Store-only even if launched without package identity.
            if option_env!("ARCRELAY_WINDOWS_MSIX_PACKAGE") == Some("1") {
                return Ok(Self::MicrosoftStore);
            }
            windows_source()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Ok(Self::Github)
        }
    }

    pub fn require_github(self) -> Result<(), String> {
        match self {
            Self::Github => Ok(()),
            Self::MicrosoftStore => {
                Err("this installation must be updated through Microsoft Store".to_string())
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_source() -> Result<AppUpdateSource, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackagePath;

    let mut length = 0;
    // A size query distinguishes an unpackaged process without allocating resources.
    let status = unsafe { GetCurrentPackagePath(&mut length, None) };
    if status == APPMODEL_ERROR_NO_PACKAGE {
        return Ok(AppUpdateSource::Github);
    }
    if status != ERROR_INSUFFICIENT_BUFFER || length == 0 {
        return Err(format!("failed to query package path: {}", status.0));
    }
    let mut buffer = vec![0u16; length as usize];
    let status = unsafe { GetCurrentPackagePath(&mut length, Some(PWSTR(buffer.as_mut_ptr()))) };
    if status != ERROR_SUCCESS {
        return Err(format!("failed to read package path: {}", status.0));
    }
    let end = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    let root = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buffer[..end]));
    let executable = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|error| format!("failed to resolve executable path: {error}"))?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("failed to resolve package path: {error}"))?;
    Ok(source_for_paths(&executable, &root))
}

#[cfg(any(target_os = "windows", test))]
fn source_for_paths(
    executable: &std::path::Path,
    package_root: &std::path::Path,
) -> AppUpdateSource {
    // Standalone installers register a sparse Share Target package whose payload
    // lives elsewhere. Package identity alone must not disable their updater.
    if executable.starts_with(package_root) {
        AppUpdateSource::MicrosoftStore
    } else {
        AppUpdateSource::Github
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn store_payload_cannot_use_the_standalone_updater() {
        let source = source_for_paths(
            Path::new("/WindowsApps/ArcRelay/ArcRelay.exe"),
            Path::new("/WindowsApps/ArcRelay"),
        );
        assert_eq!(source, AppUpdateSource::MicrosoftStore);
        assert!(source.require_github().is_err());
    }

    #[test]
    fn sparse_share_target_keeps_the_standalone_updater() {
        for executable in [
            "/Users/test/AppData/Local/ArcRelay/ArcRelay.exe",
            "/WindowsApps/ArcRelay.SystemShare-other/ArcRelay.exe",
        ] {
            let source = source_for_paths(
                Path::new(executable),
                Path::new("/WindowsApps/ArcRelay.SystemShare"),
            );
            assert_eq!(source, AppUpdateSource::Github);
            assert!(source.require_github().is_ok());
        }
    }

    #[test]
    fn source_detection_needs_no_tokio_context() {
        let source = AppUpdateSource::current().unwrap();
        #[cfg(not(target_os = "windows"))]
        assert_eq!(source, AppUpdateSource::Github);
        #[cfg(target_os = "windows")]
        assert_eq!(
            source,
            if option_env!("ARCRELAY_WINDOWS_MSIX_PACKAGE") == Some("1") {
                AppUpdateSource::MicrosoftStore
            } else {
                AppUpdateSource::Github
            }
        );
    }
}
