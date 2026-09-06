use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use arcrelay_web_gateway::WebGatewaySettings;
use serde::{Deserialize, Serialize};

const DEFAULT_CLIPBOARD_SHORTCUT: &str = "CmdOrCtrl+Shift+V";
const DEFAULT_SCREENSHOT_SHORTCUT: &str = "CmdOrCtrl+Shift+A";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub revision: u64,
    pub device_name: String,
    pub theme: ThemePreference,
    pub language: LanguagePreference,
    pub launch_at_startup: bool,
    pub launch_silently: bool,
    pub auto_update_enabled: bool,
    pub notifications: NotificationPreferences,
    pub sounds: crate::sound::SoundPreferences,
    pub clipboard_enabled: bool,
    pub clipboard_shortcut: String,
    pub clipboard_auto_focus_search: bool,
    pub clipboard_sort_by: ClipboardSortPreference,
    pub clipboard_sync_enabled: bool,
    pub clipboard_sync_update_system_clipboard: bool,
    pub clipboard_sync_edits_and_deletes: bool,
    pub clipboard_sync_favorites: bool,
    pub nearby_discoverable: bool,
    pub enhanced_screenshot_enabled: bool,
    pub screenshot_shortcut: String,
    pub screenshot_include_cursor: bool,
    pub screenshot_format: ScreenshotFormat,
    pub screenshot_file_name_template: String,
    pub web_files: WebGatewaySettings,
}

/// Only supplied fields are changed; the host owns the current settings snapshot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
#[ts(optional_fields)]
pub struct AppSettingsPatch {
    pub device_name: Option<String>,
    pub theme: Option<ThemePreference>,
    pub language: Option<LanguagePreference>,
    pub launch_at_startup: Option<bool>,
    pub launch_silently: Option<bool>,
    pub auto_update_enabled: Option<bool>,
    pub notifications: Option<NotificationPreferencesPatch>,
    pub sounds: Option<crate::sound::SoundPreferencesPatch>,
    pub clipboard_enabled: Option<bool>,
    pub clipboard_shortcut: Option<String>,
    pub clipboard_auto_focus_search: Option<bool>,
    pub clipboard_sort_by: Option<ClipboardSortPreference>,
    pub clipboard_sync_enabled: Option<bool>,
    pub clipboard_sync_update_system_clipboard: Option<bool>,
    pub clipboard_sync_edits_and_deletes: Option<bool>,
    pub clipboard_sync_favorites: Option<bool>,
    pub nearby_discoverable: Option<bool>,
    pub enhanced_screenshot_enabled: Option<bool>,
    pub screenshot_shortcut: Option<String>,
    pub screenshot_include_cursor: Option<bool>,
    pub screenshot_format: Option<ScreenshotFormat>,
    pub screenshot_file_name_template: Option<String>,
    pub web_files: Option<WebGatewaySettingsPatch>,
}

impl AppSettingsPatch {
    pub fn apply(self, mut current: AppSettings) -> AppSettings {
        if let Some(value) = self.device_name {
            current.device_name = value;
        }
        if let Some(value) = self.theme {
            current.theme = value;
        }
        if let Some(value) = self.language {
            current.language = value;
        }
        if let Some(value) = self.launch_at_startup {
            current.launch_at_startup = value;
        }
        if let Some(value) = self.launch_silently {
            current.launch_silently = value;
        }
        if let Some(value) = self.auto_update_enabled {
            current.auto_update_enabled = value;
        }
        if let Some(value) = self.sounds {
            current.sounds = value.apply(current.sounds);
        }
        if let Some(value) = self.notifications {
            current.notifications = value.apply(current.notifications);
        }
        if let Some(value) = self.clipboard_enabled {
            current.clipboard_enabled = value;
        }
        if let Some(value) = self.clipboard_shortcut {
            current.clipboard_shortcut = value;
        }
        if let Some(value) = self.clipboard_auto_focus_search {
            current.clipboard_auto_focus_search = value;
        }
        if let Some(value) = self.clipboard_sort_by {
            current.clipboard_sort_by = value;
        }
        if let Some(value) = self.clipboard_sync_enabled {
            current.clipboard_sync_enabled = value;
        }
        if let Some(value) = self.clipboard_sync_update_system_clipboard {
            current.clipboard_sync_update_system_clipboard = value;
        }
        if let Some(value) = self.clipboard_sync_edits_and_deletes {
            current.clipboard_sync_edits_and_deletes = value;
        }
        if let Some(value) = self.clipboard_sync_favorites {
            current.clipboard_sync_favorites = value;
        }
        if let Some(value) = self.nearby_discoverable {
            current.nearby_discoverable = value;
        }
        if let Some(value) = self.enhanced_screenshot_enabled {
            current.enhanced_screenshot_enabled = value;
        }
        if let Some(value) = self.screenshot_shortcut {
            current.screenshot_shortcut = value;
        }
        if let Some(value) = self.screenshot_include_cursor {
            current.screenshot_include_cursor = value;
        }
        if let Some(value) = self.screenshot_format {
            current.screenshot_format = value;
        }
        if let Some(value) = self.screenshot_file_name_template {
            current.screenshot_file_name_template = value;
        }
        if let Some(value) = self.web_files {
            current.web_files = value.apply(current.web_files);
        }
        current
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            revision: 0,
            device_name: String::new(),
            theme: ThemePreference::System,
            language: LanguagePreference::System,
            launch_at_startup: false,
            launch_silently: false,
            auto_update_enabled: true,
            notifications: NotificationPreferences::default(),
            sounds: crate::sound::SoundPreferences::default(),
            clipboard_enabled: true,
            clipboard_shortcut: DEFAULT_CLIPBOARD_SHORTCUT.into(),
            clipboard_auto_focus_search: true,
            clipboard_sort_by: ClipboardSortPreference::UpdatedAt,
            clipboard_sync_enabled: true,
            clipboard_sync_update_system_clipboard: true,
            clipboard_sync_edits_and_deletes: true,
            clipboard_sync_favorites: true,
            nearby_discoverable: true,
            enhanced_screenshot_enabled: false,
            screenshot_shortcut: DEFAULT_SCREENSHOT_SHORTCUT.into(),
            screenshot_include_cursor: false,
            screenshot_format: ScreenshotFormat::Png,
            screenshot_file_name_template: "Screenshot_{date}_{time}".into(),
            web_files: WebGatewaySettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPreferences {
    pub enabled: bool,
    pub only_when_inactive: bool,
    pub show_previews: bool,
    pub pairing_requests: bool,
    pub transfer_requests: bool,
    pub transfer_completed: bool,
    pub transfer_failed: bool,
    pub remote_file_completed: bool,
    pub remote_file_failed: bool,
    pub print_completed: bool,
    pub print_failed: bool,
    pub device_connections: bool,
    pub workflow_action_required: bool,
    pub workflow_completed: bool,
    pub workflow_failed: bool,
    pub input_permission_required: bool,
    pub agent_notifications: bool,
    pub update_available: bool,
}

impl Default for NotificationPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            only_when_inactive: true,
            show_previews: false,
            pairing_requests: true,
            transfer_requests: true,
            transfer_completed: true,
            transfer_failed: true,
            remote_file_completed: true,
            remote_file_failed: true,
            print_completed: false,
            print_failed: true,
            device_connections: false,
            workflow_action_required: true,
            workflow_completed: false,
            workflow_failed: true,
            input_permission_required: true,
            agent_notifications: true,
            update_available: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum LanguagePreference {
    System,
    ZhCn,
    EnUs,
    JaJp,
    KoKr,
    DeDe,
    FrFr,
    EsEs,
    PtBr,
}

impl LanguagePreference {
    pub fn resolve(self) -> Self {
        if self != Self::System {
            return self;
        }
        Self::from_locales(sys_locale::get_locales())
    }

    fn from_locales(locales: impl IntoIterator<Item = String>) -> Self {
        for locale in locales {
            let code = locale
                .split(['-', '_'])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            match code.as_str() {
                "zh" => return Self::ZhCn,
                "en" => return Self::EnUs,
                "ja" => return Self::JaJp,
                "ko" => return Self::KoKr,
                "de" => return Self::DeDe,
                "fr" => return Self::FrFr,
                "es" => return Self::EsEs,
                "pt" => return Self::PtBr,
                _ => {}
            }
        }
        Self::EnUs
    }

    pub fn uses_english_fallback(self) -> bool {
        self.resolve() != Self::ZhCn
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardSortPreference {
    CreatedAt,
    UpdatedAt,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
    Webp,
}

#[derive(Clone)]
pub struct SettingsManager {
    path: Arc<PathBuf>,
    settings: Arc<RwLock<AppSettings>>,
}

impl SettingsManager {
    pub fn new(config_directory: &Path) -> Result<Self, String> {
        let path = config_directory.join("settings.json");
        let settings = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(settings) => settings,
                Err(error) => {
                    let recovered = std::fs::read(path.with_extension("json.bak"))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
                    let Some(settings) = recovered else {
                        return Err(format!(
                            "settings file is corrupted; original retained at {}: {error}",
                            path.display()
                        ));
                    };
                    crate::infrastructure::durable_file::replace(
                        &path.with_extension(format!("json.corrupt.{}", uuid::Uuid::new_v4())),
                        &bytes,
                    )
                    .map_err(|error| error.to_string())?;
                    settings
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
            Err(error) => return Err(format!("failed to read settings: {error}")),
        };
        Ok(Self {
            path: Arc::new(path),
            settings: Arc::new(RwLock::new(settings)),
        })
    }

    pub fn snapshot(&self) -> AppSettings {
        self.settings
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn save(&self, mut settings: AppSettings) -> Result<AppSettings, String> {
        validate(&settings)?;
        let mut current = self
            .settings
            .write()
            .unwrap_or_else(|error| error.into_inner());
        settings.revision = current.revision.saturating_add(1);
        if let Ok(previous) = std::fs::read(self.path.as_ref()) {
            if serde_json::from_slice::<AppSettings>(&previous).is_ok() {
                crate::infrastructure::durable_file::replace(
                    &self.path.with_extension("json.bak"),
                    &previous,
                )
                .map_err(|error| error.to_string())?;
            }
        }
        let bytes = serde_json::to_vec_pretty(&settings)
            .map_err(|error| format!("failed to save settings: {error}"))?;
        crate::infrastructure::durable_file::replace(self.path.as_ref(), &bytes)
            .map_err(|error| format!("failed to save settings: {error}"))?;
        *current = settings.clone();
        Ok(settings)
    }
}

pub(crate) fn validate(settings: &AppSettings) -> Result<(), String> {
    validate_device_name(&settings.device_name)?;
    settings.sounds.validate()?;
    settings
        .web_files
        .validate()
        .map_err(|error| error.to_string())?;
    let shortcut = settings.clipboard_shortcut.trim();
    if shortcut.is_empty() {
        return Err("clipboard shortcut cannot be empty".into());
    }
    let clipboard_key = shortcut
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .map_err(|_| "invalid shortcut; use a format such as CmdOrCtrl+Shift+V".to_string())?;
    let screenshot_shortcut = settings.screenshot_shortcut.trim();
    if screenshot_shortcut.is_empty() {
        return Err("screenshot shortcut cannot be empty".into());
    }
    let screenshot_key = screenshot_shortcut
        .parse::<tauri_plugin_global_shortcut::Shortcut>()
        .map_err(|_| {
            "invalid screenshot shortcut; use a format such as CmdOrCtrl+Shift+A".to_string()
        })?;
    if settings.clipboard_enabled
        && settings.enhanced_screenshot_enabled
        && clipboard_key == screenshot_key
    {
        return Err("screenshot and clipboard shortcuts must differ".into());
    }
    let template = settings.screenshot_file_name_template.trim();
    if template.is_empty()
        || template
            .chars()
            .any(|character| ['/', '\\', '\0'].contains(&character))
    {
        return Err(
            "screenshot filename template cannot be empty or contain path separators".into(),
        );
    }
    Ok(())
}

pub fn validate_device_name(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.chars().any(char::is_control) {
        return Err("device name cannot contain control characters".into());
    }
    if value.len() > 128 {
        return Err("device name cannot exceed 128 bytes".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_preserve_unrelated_window_changes() {
        let initial = AppSettings::default();
        let first: AppSettingsPatch = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        let second: AppSettingsPatch =
            serde_json::from_str(r#"{"clipboardEnabled":false,"launchSilently":true}"#).unwrap();
        let current = second.apply(first.apply(initial));
        assert_eq!(current.theme, ThemePreference::Dark);
        assert!(!current.clipboard_enabled);
        assert!(current.launch_silently);
        assert!(!current.launch_at_startup);
        let third: AppSettingsPatch = serde_json::from_str(r#"{"launchSilently":false}"#).unwrap();
        let current = third.apply(current);
        assert!(!current.launch_silently);
        assert_eq!(current.theme, ThemePreference::Dark);
        assert!(serde_json::from_str::<AppSettingsPatch>(r#"{"typo":true}"#).is_err());
    }

    #[test]
    fn corrupt_settings_are_preserved_and_failed_saves_do_not_change_memory() {
        let directory = tempfile::tempdir().unwrap();
        let manager = SettingsManager::new(directory.path()).unwrap();
        let before = manager.snapshot();
        std::fs::create_dir(directory.path().join("settings.json")).unwrap();
        assert!(manager
            .save(AppSettings {
                theme: ThemePreference::Dark,
                ..before.clone()
            })
            .is_err());
        assert_eq!(manager.snapshot(), before);
        std::fs::remove_dir(directory.path().join("settings.json")).unwrap();
        std::fs::write(directory.path().join("settings.json"), b"broken").unwrap();
        assert!(SettingsManager::new(directory.path()).is_err());
        assert_eq!(
            std::fs::read(directory.path().join("settings.json")).unwrap(),
            b"broken"
        );
    }

    #[test]
    fn saves_and_loads_settings() {
        let directory =
            std::env::temp_dir().join(format!("arcrelay-settings-test-{}", uuid::Uuid::new_v4()));
        let manager = SettingsManager::new(&directory).unwrap();
        let mut settings = manager.snapshot();
        settings.clipboard_enabled = false;
        settings.device_name = "书房电脑".into();
        settings.theme = ThemePreference::Dark;
        settings.launch_silently = true;
        manager.save(settings.clone()).unwrap();
        settings.revision = 1;
        assert_eq!(
            SettingsManager::new(&directory).unwrap().snapshot(),
            settings
        );
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn missing_auto_focus_setting_uses_default() {
        let settings: AppSettings = serde_json::from_str("{}").unwrap();
        assert!(!settings.launch_silently);
        assert!(settings.clipboard_auto_focus_search);
        assert!(settings.auto_update_enabled);
        assert!(settings.notifications.enabled);
        assert!(settings.notifications.only_when_inactive);
        assert!(!settings.notifications.show_previews);
        assert!(settings.notifications.workflow_failed);
        assert!(!settings.notifications.workflow_completed);
        assert!(!settings.notifications.device_connections);
    }

    #[test]
    fn existing_settings_keep_enhanced_screenshot_opted_out() {
        let settings: AppSettings = serde_json::from_str(r#"{"deviceName":"旧设备"}"#).unwrap();
        assert!(!settings.enhanced_screenshot_enabled);
        assert_eq!(settings.screenshot_shortcut, DEFAULT_SCREENSHOT_SHORTCUT);
    }

    #[test]
    fn partial_notification_preferences_keep_safe_defaults() {
        let settings: AppSettings =
            serde_json::from_str(r#"{"notifications":{"enabled":false}}"#).unwrap();
        assert!(!settings.notifications.enabled);
        assert!(settings.notifications.only_when_inactive);
        assert!(!settings.notifications.show_previews);
        assert!(settings.notifications.transfer_failed);
        assert!(settings.notifications.print_failed);
        assert!(!settings.notifications.device_connections);
    }

    #[test]
    fn enabled_global_shortcuts_must_not_conflict() {
        let defaults = AppSettings::default();
        let settings = AppSettings {
            enhanced_screenshot_enabled: true,
            screenshot_shortcut: defaults.clipboard_shortcut.clone(),
            ..defaults
        };
        assert_eq!(
            validate(&settings).unwrap_err(),
            "screenshot and clipboard shortcuts must differ"
        );
    }

    #[test]
    fn global_shortcut_aliases_are_the_same_binding() {
        let settings = AppSettings {
            enhanced_screenshot_enabled: true,
            screenshot_shortcut: "Shift+CmdOrCtrl+KeyV".into(),
            ..AppSettings::default()
        };
        assert_eq!(
            validate(&settings).unwrap_err(),
            "screenshot and clipboard shortcuts must differ"
        );
    }

    #[test]
    fn accepts_control_and_command_as_distinct_modifiers() {
        let settings = AppSettings {
            enhanced_screenshot_enabled: true,
            screenshot_shortcut: "Ctrl+Cmd+A".into(),
            ..AppSettings::default()
        };
        validate(&settings).unwrap();
        let shortcut = settings
            .screenshot_shortcut
            .parse::<tauri_plugin_global_shortcut::Shortcut>()
            .unwrap();
        assert!(shortcut
            .mods
            .contains(tauri_plugin_global_shortcut::Modifiers::CONTROL));
        assert!(shortcut
            .mods
            .contains(tauri_plugin_global_shortcut::Modifiers::SUPER));
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
#[ts(optional_fields)]
pub struct NotificationPreferencesPatch {
    pub enabled: Option<bool>,
    pub only_when_inactive: Option<bool>,
    pub show_previews: Option<bool>,
    pub pairing_requests: Option<bool>,
    pub transfer_requests: Option<bool>,
    pub transfer_completed: Option<bool>,
    pub transfer_failed: Option<bool>,
    pub remote_file_completed: Option<bool>,
    pub remote_file_failed: Option<bool>,
    pub print_completed: Option<bool>,
    pub print_failed: Option<bool>,
    pub device_connections: Option<bool>,
    pub workflow_action_required: Option<bool>,
    pub workflow_completed: Option<bool>,
    pub workflow_failed: Option<bool>,
    pub input_permission_required: Option<bool>,
    pub agent_notifications: Option<bool>,
    pub update_available: Option<bool>,
}
impl NotificationPreferencesPatch {
    fn apply(self, mut current: NotificationPreferences) -> NotificationPreferences {
        if let Some(value) = self.enabled {
            current.enabled = value;
        }
        if let Some(value) = self.only_when_inactive {
            current.only_when_inactive = value;
        }
        if let Some(value) = self.show_previews {
            current.show_previews = value;
        }
        if let Some(value) = self.pairing_requests {
            current.pairing_requests = value;
        }
        if let Some(value) = self.transfer_requests {
            current.transfer_requests = value;
        }
        if let Some(value) = self.transfer_completed {
            current.transfer_completed = value;
        }
        if let Some(value) = self.transfer_failed {
            current.transfer_failed = value;
        }
        if let Some(value) = self.remote_file_completed {
            current.remote_file_completed = value;
        }
        if let Some(value) = self.remote_file_failed {
            current.remote_file_failed = value;
        }
        if let Some(value) = self.print_completed {
            current.print_completed = value;
        }
        if let Some(value) = self.print_failed {
            current.print_failed = value;
        }
        if let Some(value) = self.device_connections {
            current.device_connections = value;
        }
        if let Some(value) = self.workflow_action_required {
            current.workflow_action_required = value;
        }
        if let Some(value) = self.workflow_completed {
            current.workflow_completed = value;
        }
        if let Some(value) = self.workflow_failed {
            current.workflow_failed = value;
        }
        if let Some(value) = self.input_permission_required {
            current.input_permission_required = value;
        }
        if let Some(value) = self.agent_notifications {
            current.agent_notifications = value;
        }
        if let Some(value) = self.update_available {
            current.update_available = value;
        }
        current
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
#[ts(optional_fields)]
pub struct WebGatewaySettingsPatch {
    pub enabled: Option<bool>,
    pub port: Option<u16>,
    pub bind_mode: Option<arcrelay_web_gateway::WebGatewayBindMode>,
    pub site_name: Option<String>,
    pub session_idle_minutes: Option<u32>,
    pub allow_vpn_private: Option<bool>,
}
impl WebGatewaySettingsPatch {
    fn apply(self, mut current: WebGatewaySettings) -> WebGatewaySettings {
        if let Some(value) = self.enabled {
            current.enabled = value;
        }
        if let Some(value) = self.port {
            current.port = value;
        }
        if let Some(value) = self.bind_mode {
            current.bind_mode = value;
        }
        if let Some(value) = self.site_name {
            current.site_name = value;
        }
        if let Some(value) = self.session_idle_minutes {
            current.session_idle_minutes = value;
        }
        if let Some(value) = self.allow_vpn_private {
            current.allow_vpn_private = value;
        }
        current
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn language_preferences_work_without_async_runtime() {
        assert_eq!(AppSettings::default().language, LanguagePreference::System);
        for (tag, expected) in [
            ("de-AT", LanguagePreference::DeDe),
            ("pt_PT", LanguagePreference::PtBr),
            ("zh-Hans-CN", LanguagePreference::ZhCn),
            ("ko-KR", LanguagePreference::KoKr),
        ] {
            assert_eq!(LanguagePreference::from_locales([tag.to_owned()]), expected);
        }
        assert_eq!(
            LanguagePreference::from_locales(["xx".into(), "fr-CA".into()]),
            LanguagePreference::FrFr
        );
        assert_eq!(
            LanguagePreference::from_locales(["xx".into()]),
            LanguagePreference::EnUs
        );
        assert_ne!(
            LanguagePreference::System.resolve(),
            LanguagePreference::System
        );
        assert_eq!(
            serde_json::from_str::<LanguagePreference>("\"zhCn\"").unwrap(),
            LanguagePreference::ZhCn
        );
        assert_eq!(
            serde_json::from_str::<LanguagePreference>("\"enUs\"").unwrap(),
            LanguagePreference::EnUs
        );
    }
}
