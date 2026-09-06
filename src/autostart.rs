use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as AutostartExt;

pub fn enable(app: &AppHandle) -> Result<(), String> {
    platform::enable(app)
}

pub fn disable(app: &AppHandle) -> Result<(), String> {
    platform::disable(app)
}

pub fn sync_on_startup(app: &AppHandle) -> Result<bool, String> {
    platform::sync_on_startup(app)
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;

    pub fn enable(app: &AppHandle) -> Result<(), String> {
        app.autolaunch().enable().map_err(|error| error.to_string())
    }

    pub fn disable(app: &AppHandle) -> Result<(), String> {
        app.autolaunch()
            .disable()
            .map_err(|error| error.to_string())
    }

    pub fn sync_on_startup(app: &AppHandle) -> Result<bool, String> {
        app.autolaunch()
            .is_enabled()
            .map_err(|error| error.to_string())
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::fs;
    use std::path::{Path, PathBuf};

    use objc2_foundation::{NSError, NSString, NSURL};
    use objc2_service_management::{
        kSMErrorAlreadyRegistered, kSMErrorJobNotFound, SMAppService, SMAppServiceStatus,
    };

    use super::*;

    const LEGACY_LAUNCH_AGENT_NAME: &str = "ArcRelay.plist";

    pub fn enable(app: &AppHandle) -> Result<(), String> {
        if supports_service_management() {
            register_main_app()
        } else {
            app.autolaunch().enable().map_err(|error| error.to_string())
        }?;
        remove_legacy_launch_agent()
    }

    pub fn disable(app: &AppHandle) -> Result<(), String> {
        let registration_result = if supports_service_management() {
            unregister_main_app()
        } else {
            app.autolaunch()
                .disable()
                .map_err(|error| error.to_string())
        };

        let cleanup_result = remove_legacy_launch_agent();
        registration_result.and(cleanup_result)
    }

    pub fn sync_on_startup(app: &AppHandle) -> Result<bool, String> {
        if legacy_launch_agent_is_enabled()? {
            enable(app)?;
        }
        is_enabled(app)
    }

    fn is_enabled(app: &AppHandle) -> Result<bool, String> {
        if supports_service_management() {
            Ok(main_app_status() == SMAppServiceStatus::Enabled)
        } else {
            app.autolaunch()
                .is_enabled()
                .map_err(|error| error.to_string())
        }
    }

    fn supports_service_management() -> bool {
        objc2::available!(macos = 13.0)
    }

    fn main_app_status() -> SMAppServiceStatus {
        // SAFETY: SMAppService is only reached after the macOS 13 availability check.
        unsafe { SMAppService::mainAppService().status() }
    }

    fn register_main_app() -> Result<(), String> {
        if main_app_status() == SMAppServiceStatus::Enabled {
            return Ok(());
        }

        // SAFETY: SMAppService is only reached after the macOS 13 availability check.
        let result = unsafe { SMAppService::mainAppService().registerAndReturnError() };
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.code() == kSMErrorAlreadyRegistered as isize => Ok(()),
            Err(error) => Err(service_error("register ArcRelay login item", &error)),
        }
    }

    fn unregister_main_app() -> Result<(), String> {
        if matches!(
            main_app_status(),
            SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound
        ) {
            return Ok(());
        }

        // SAFETY: SMAppService is only reached after the macOS 13 availability check.
        let result = unsafe { SMAppService::mainAppService().unregisterAndReturnError() };
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.code() == kSMErrorJobNotFound as isize => Ok(()),
            Err(error) => Err(service_error("remove ArcRelay login item", &error)),
        }
    }

    fn service_error(action: &str, error: &NSError) -> String {
        format!("failed to {action}: {}", error.localizedDescription())
    }

    fn legacy_launch_agent_path() -> Option<PathBuf> {
        dirs::home_dir().map(|home| {
            home.join("Library")
                .join("LaunchAgents")
                .join(LEGACY_LAUNCH_AGENT_NAME)
        })
    }

    fn legacy_launch_agent_is_enabled() -> Result<bool, String> {
        let Some(path) = legacy_launch_agent_path() else {
            return Ok(false);
        };
        if !is_arcrelay_legacy_launch_agent(&path)? {
            return Ok(false);
        }

        if !supports_service_management() {
            return Ok(true);
        }

        let path_string = path.to_string_lossy();
        let path = NSString::from_str(&path_string);
        let url = NSURL::fileURLWithPath(&path);
        // SAFETY: SMAppService is only reached after the macOS 13 availability check.
        let status = unsafe { SMAppService::statusForLegacyURL(&url) };
        Ok(status == SMAppServiceStatus::Enabled)
    }

    fn remove_legacy_launch_agent() -> Result<(), String> {
        let Some(path) = legacy_launch_agent_path() else {
            return Ok(());
        };
        if !is_arcrelay_legacy_launch_agent(&path)? {
            return Ok(());
        }
        fs::remove_file(&path)
            .map_err(|error| format!("failed to remove legacy ArcRelay LaunchAgent: {error}"))
    }

    fn is_arcrelay_legacy_launch_agent(path: &Path) -> Result<bool, String> {
        if !path.exists() {
            return Ok(false);
        }
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read legacy ArcRelay LaunchAgent: {error}"))?;
        Ok(contents.contains("<key>Label</key>")
            && contents.contains("<string>ArcRelay</string>")
            && contents.contains("arcrelay-desktop"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recognizes_the_legacy_arcrelay_launch_agent() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let path = directory.path().join(LEGACY_LAUNCH_AGENT_NAME);
            fs::write(
                &path,
                r#"<plist><dict><key>Label</key><string>ArcRelay</string><key>ProgramArguments</key><array><string>/Applications/ArcRelay.app/Contents/MacOS/arcrelay-desktop</string></array></dict></plist>"#,
            )
            .expect("write launch agent");

            assert!(is_arcrelay_legacy_launch_agent(&path).unwrap());
        }

        #[test]
        fn ignores_an_unrelated_launch_agent_with_the_same_filename() {
            let directory = tempfile::tempdir().expect("temporary directory");
            let path = directory.path().join(LEGACY_LAUNCH_AGENT_NAME);
            fs::write(
                &path,
                r#"<plist><dict><key>Label</key><string>OtherApp</string></dict></plist>"#,
            )
            .expect("write launch agent");

            assert!(!is_arcrelay_legacy_launch_agent(&path).unwrap());
        }
    }
}
