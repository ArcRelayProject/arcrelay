use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use toml::Value;

use crate::settings::{AppSettings, ScreenshotFormat};

const INTEGRATION_PROTOCOL_VERSION: u32 = 1;
const EXECUTABLE_OVERRIDE_ENV: &str = "ARCRELAY_SNIPTRA_PATH";
const PROFILE_DIRECTORY_ENV: &str = "SNIPTRA_PROFILE_DIR";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartOutcome {
    Started(u32),
    AlreadyRunning,
}

#[derive(Debug, Deserialize)]
struct IntegrationInfo {
    application: String,
    version: String,
    protocol_version: u32,
    one_shot_command: String,
    profile_directory_environment: String,
}

#[derive(Debug)]
pub struct ScreenshotService {
    enabled: AtomicBool,
    running: AtomicBool,
    profile_directory: PathBuf,
}

impl ScreenshotService {
    pub fn new(config_directory: &Path, enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            running: AtomicBool::new(false),
            profile_directory: config_directory.join("sniptra"),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn sync_preferences(&self, settings: &AppSettings) -> Result<(), String> {
        let path = self.profile_directory.join("config/settings.toml");
        let source = std::fs::read_to_string(&path).unwrap_or_else(|_| default_profile());
        let mut profile = source
            .parse::<Value>()
            .map_err(|error| format!("failed to read screenshot settings: {error}"))?;
        set_value(
            &mut profile,
            "capture",
            "include_cursor",
            settings.screenshot_include_cursor.into(),
        )?;
        set_value(
            &mut profile,
            "capture",
            "remember_last_region",
            false.into(),
        )?;
        let format = match settings.screenshot_format {
            ScreenshotFormat::Png => "png",
            ScreenshotFormat::Jpeg => "jpeg",
            ScreenshotFormat::Webp => "web_p",
        };
        set_value(
            &mut profile,
            "output",
            "default_action",
            "copy_to_clipboard".into(),
        )?;
        set_value(&mut profile, "output", "format", format.into())?;
        set_value(
            &mut profile,
            "output",
            "file_name_template",
            settings.screenshot_file_name_template.trim().into(),
        )?;
        let source = toml::to_string_pretty(&profile)
            .map_err(|error| format!("failed to save screenshot settings: {error}"))?;
        crate::infrastructure::durable_file::replace(&path, source.as_bytes())
            .map_err(|error| format!("failed to save screenshot settings: {error}"))
    }

    pub fn ensure_available(&self) -> Result<String, String> {
        let executable = resolve_executable();
        if !executable.is_file() {
            return Err(format!(
                "Sniptra integration component not found: {}",
                executable.display()
            ));
        }
        let mut command = Command::new(executable);
        command.arg("integration-info");
        let output =
            arcrelay_core::infrastructure::bounded_command::output(command, Duration::from_secs(5))
                .map_err(|error| {
                    format!("failed to inspect Sniptra integration component: {error}")
                })?;
        if !output.status.success() {
            return Err(format!(
                "Sniptra integration check failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let info: IntegrationInfo = serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("Sniptra returned invalid integration metadata: {error}"))?;
        if info.application != "sniptra"
            || info.protocol_version != INTEGRATION_PROTOCOL_VERSION
            || info.one_shot_command != "one-shot"
            || info.profile_directory_environment != PROFILE_DIRECTORY_ENV
        {
            return Err(format!(
                "incompatible Sniptra integration protocol (version {INTEGRATION_PROTOCOL_VERSION} required)"
            ));
        }
        Ok(info.version)
    }

    pub fn capture_region(self: &Arc<Self>) -> Result<StartOutcome, String> {
        self.spawn_interactive(region_arguments())
    }

    pub fn open_settings(self: &Arc<Self>) -> Result<StartOutcome, String> {
        self.spawn_interactive(settings_arguments())
    }

    pub fn capture_fullscreen(self: &Arc<Self>, path: &Path) -> Result<(), String> {
        let _guard = self.acquire()?;
        let status = self
            .command(fullscreen_arguments(path))
            .status()
            .map_err(|error| format!("failed to start Sniptra: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Sniptra full-screen capture failed: {status}"))
        }
    }

    fn spawn_interactive(
        self: &Arc<Self>,
        arguments: Vec<OsString>,
    ) -> Result<StartOutcome, String> {
        let guard = match self.acquire() {
            Ok(guard) => guard,
            Err(error) if error == "SNIPTRA_ALREADY_RUNNING" => {
                return Ok(StartOutcome::AlreadyRunning);
            }
            Err(error) => return Err(error),
        };
        let mut child = self
            .command(arguments)
            .spawn()
            .map_err(|error| format!("failed to start Sniptra: {error}"))?;
        let pid = child.id();
        std::thread::spawn(move || {
            match child.wait() {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(?status, "Sniptra process exited unsuccessfully"),
                Err(error) => tracing::warn!(%error, "Failed to wait for Sniptra process"),
            }
            drop(guard);
        });
        Ok(StartOutcome::Started(pid))
    }

    fn acquire(self: &Arc<Self>) -> Result<RunningGuard, String> {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "SNIPTRA_ALREADY_RUNNING".to_string())?;
        Ok(RunningGuard {
            service: Arc::clone(self),
        })
    }

    fn command(&self, arguments: Vec<OsString>) -> Command {
        // Resolve on every launch. ArcRelay commonly stays alive in the tray
        // while an installer upgrades the application and adds or replaces
        // sidecars beside the executable. Caching a missing development
        // fallback at startup makes the newly installed sidecar unreachable
        // until the resident process is restarted.
        let mut command = Command::new(resolve_executable());
        command
            .args(arguments)
            .env(PROFILE_DIRECTORY_ENV, &self.profile_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The bundled executable may use the console subsystem even
            // though it presents its own native UI.
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command
    }
}

fn set_value(profile: &mut Value, section: &str, key: &str, value: Value) -> Result<(), String> {
    let table = profile
        .as_table_mut()
        .ok_or_else(|| "invalid screenshot settings root".to_string())?;
    let section = table
        .entry(section)
        .or_insert_with(|| Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| "invalid screenshot settings group".to_string())?;
    section.insert(key.to_string(), value);
    Ok(())
}

const DEFAULT_PROFILE: &str = r#"schema_version = 3

[appearance]
language = "simplified_chinese"
theme = "dark"

[hotkeys]
area_capture = "Cmd+Shift+A"
copy_image = "Cmd+C"
copy_ocr = "Cmd+Shift+C"
copy_color = "Alt+C"
save_file = "Cmd+S"
pin_to_desktop = "Cmd+P"
close_window = "Cmd+W"

[capture]
include_cursor = false
remember_last_region = false

[output]
default_action = "copy_to_clipboard"
format = "png"
quick_save_directory = ""
file_name_template = "Screenshot_{date}_{time}"

[editor]
default_color = { r = 255, g = 59, b = 48, a = 255 }
default_line_width = 4.0
default_font_size = 24.0

[privacy]
persistent_history = false
private_mode = false

[pinboard]
default_opacity = 1.0
"#;

fn default_profile() -> String {
    #[cfg(target_os = "macos")]
    return DEFAULT_PROFILE.to_string();
    #[cfg(not(target_os = "macos"))]
    return DEFAULT_PROFILE.replace("Cmd+", "Ctrl+");
}

#[derive(Debug)]
struct RunningGuard {
    service: Arc<ScreenshotService>,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.service.running.store(false, Ordering::SeqCst);
    }
}

fn region_arguments() -> Vec<OsString> {
    ["one-shot", "capture", "--mode", "area", "--copy"]
        .into_iter()
        .map(OsString::from)
        .collect()
}

fn settings_arguments() -> Vec<OsString> {
    ["one-shot", "settings", "edit"]
        .into_iter()
        .map(OsString::from)
        .collect()
}

fn fullscreen_arguments(path: &Path) -> Vec<OsString> {
    let mut arguments = [
        "one-shot",
        "capture",
        "--mode",
        "desktop",
        "--no-copy",
        "--save",
    ]
    .into_iter()
    .map(OsString::from)
    .collect::<Vec<_>>();
    arguments.push(path.as_os_str().to_owned());
    arguments.push(OsString::from("--no-edit"));
    arguments
}

fn resolve_executable() -> PathBuf {
    let executable_name = if cfg!(windows) {
        "sniptra.exe"
    } else {
        "sniptra"
    };
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            let bundled = directory.join(executable_name);
            if bundled.is_file() {
                return bundled;
            }
        }
    }
    if let Some(override_path) = std::env::var_os(EXECUTABLE_OVERRIDE_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    {
        return override_path;
    }
    PathBuf::from(executable_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_capture_uses_the_public_one_shot_contract() {
        assert_eq!(
            region_arguments(),
            ["one-shot", "capture", "--mode", "area", "--copy"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn fullscreen_capture_preserves_noninteractive_save_semantics() {
        let path = Path::new("/tmp/ArcRelay Screenshot.png");
        assert_eq!(
            fullscreen_arguments(path),
            [
                OsString::from("one-shot"),
                OsString::from("capture"),
                OsString::from("--mode"),
                OsString::from("desktop"),
                OsString::from("--no-copy"),
                OsString::from("--save"),
                path.as_os_str().to_owned(),
                OsString::from("--no-edit"),
            ]
        );
    }

    #[test]
    fn integration_info_rejects_protocol_drift() {
        let value = br#"{
            "application":"sniptra",
            "version":"0.1.0",
            "protocol_version":2,
            "one_shot_command":"one-shot",
            "profile_directory_environment":"SNIPTRA_PROFILE_DIR"
        }"#;
        let info: IntegrationInfo = serde_json::from_slice(value).unwrap();
        assert_ne!(info.protocol_version, INTEGRATION_PROTOCOL_VERSION);
    }

    #[test]
    fn integrated_preferences_are_written_to_the_sniptra_profile() {
        let directory = tempfile::tempdir().unwrap();
        let service = ScreenshotService::new(directory.path(), true);
        let settings = AppSettings {
            screenshot_include_cursor: true,
            screenshot_format: ScreenshotFormat::Webp,
            screenshot_file_name_template: "ArcRelay_{date}".into(),
            ..AppSettings::default()
        };

        service.sync_preferences(&settings).unwrap();

        let profile =
            std::fs::read_to_string(directory.path().join("sniptra/config/settings.toml"))
                .unwrap()
                .parse::<Value>()
                .unwrap();
        assert_eq!(profile["capture"]["include_cursor"].as_bool(), Some(true));
        assert_eq!(
            profile["capture"]["remember_last_region"].as_bool(),
            Some(false)
        );
        assert_eq!(
            profile["output"]["default_action"].as_str(),
            Some("copy_to_clipboard")
        );
        assert_eq!(profile["output"]["format"].as_str(), Some("web_p"));
        assert_eq!(
            profile["output"]["file_name_template"].as_str(),
            Some("ArcRelay_{date}")
        );
    }
}
