use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::notification::HostNotification;
use crate::settings::{LanguagePreference, NotificationPreferences, SettingsManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNotificationCategory {
    PairingRequest,
    TransferRequest,
    TransferCompleted,
    TransferFailed,
    RemoteFileCompleted,
    RemoteFileFailed,
    PrintCompleted,
    PrintFailed,
    DeviceConnection,
    WorkflowActionRequired,
    WorkflowCompleted,
    WorkflowFailed,
    InputPermissionRequired,
    AgentNotification,
    UpdateAvailable,
}

impl DesktopNotificationCategory {
    pub fn enabled(self, settings: &NotificationPreferences) -> bool {
        settings.enabled
            && match self {
                Self::PairingRequest => settings.pairing_requests,
                Self::TransferRequest => settings.transfer_requests,
                Self::TransferCompleted => settings.transfer_completed,
                Self::TransferFailed => settings.transfer_failed,
                Self::RemoteFileCompleted => settings.remote_file_completed,
                Self::RemoteFileFailed => settings.remote_file_failed,
                Self::PrintCompleted => settings.print_completed,
                Self::PrintFailed => settings.print_failed,
                Self::DeviceConnection => settings.device_connections,
                Self::WorkflowActionRequired => settings.workflow_action_required,
                Self::WorkflowCompleted => settings.workflow_completed,
                Self::WorkflowFailed => settings.workflow_failed,
                Self::InputPermissionRequired => settings.input_permission_required,
                Self::AgentNotification => settings.agent_notifications,
                Self::UpdateAvailable => settings.update_available,
            }
    }

    fn private_title(self, language: LanguagePreference) -> String {
        let (zh_cn, en_us) = match self {
            Self::PairingRequest => ("新的配对请求", "New pairing request"),
            Self::TransferRequest => ("文件传输等待确认", "File transfer needs approval"),
            Self::TransferCompleted => ("文件传输已完成", "File transfer completed"),
            Self::TransferFailed => ("文件传输失败", "File transfer failed"),
            Self::RemoteFileCompleted => ("远程文件操作已完成", "Remote file operation completed"),
            Self::RemoteFileFailed => ("远程文件操作失败", "Remote file operation failed"),
            Self::PrintCompleted => ("打印任务已完成", "Print job completed"),
            Self::PrintFailed => ("打印任务需要处理", "Print job needs attention"),
            Self::DeviceConnection => ("设备连接状态已变化", "Device connection changed"),
            Self::WorkflowActionRequired => ("工作流等待确认", "Workflow needs confirmation"),
            Self::WorkflowCompleted => ("工作流已完成", "Workflow completed"),
            Self::WorkflowFailed => ("工作流失败", "Workflow failed"),
            Self::InputPermissionRequired => {
                ("远程输入需要系统权限", "Remote input needs permission")
            }
            Self::AgentNotification => ("新的 Agent 通知", "New Agent notification"),
            Self::UpdateAvailable => ("ArcRelay 可以更新", "ArcRelay update available"),
        };
        localized(language, zh_cn, en_us)
    }

    fn requires_foreground_feedback(self) -> bool {
        matches!(
            self,
            Self::PairingRequest
                | Self::TransferRequest
                | Self::TransferFailed
                | Self::RemoteFileFailed
                | Self::PrintFailed
                | Self::WorkflowActionRequired
                | Self::WorkflowFailed
                | Self::InputPermissionRequired
                | Self::AgentNotification
        )
    }

    fn is_error(self) -> bool {
        matches!(
            self,
            Self::TransferFailed
                | Self::RemoteFileFailed
                | Self::PrintFailed
                | Self::WorkflowFailed
                | Self::InputPermissionRequired
        )
    }
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InAppNotification {
    pub title: String,
    pub body: String,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopNotification {
    pub category: DesktopNotificationCategory,
    pub title: String,
    pub body: String,
    pub action: Option<DesktopNotificationAction>,
    pub error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopNotificationAction {
    OpenTransferRequest(String),
}

impl DesktopNotification {
    pub fn new(
        category: DesktopNotificationCategory,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            category,
            title: title.into(),
            body: body.into(),
            action: None,
            error: false,
        }
    }

    pub fn open_transfer_request(mut self, transfer_id: impl Into<String>) -> Self {
        self.action = Some(DesktopNotificationAction::OpenTransferRequest(
            transfer_id.into(),
        ));
        self
    }

    pub fn error(mut self) -> Self {
        self.error = true;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopNotificationDelivery {
    Shown,
    Disabled,
    MainWindowActive,
}

pub fn show(
    app: &AppHandle,
    settings: &SettingsManager,
    notification: DesktopNotification,
) -> Result<DesktopNotificationDelivery, String> {
    let snapshot = settings.snapshot();
    let preferences = &snapshot.notifications;
    if !notification.category.enabled(preferences) {
        return Ok(DesktopNotificationDelivery::Disabled);
    }
    if preferences.only_when_inactive && crate::windowing::main_window_active() {
        if notification.category.requires_foreground_feedback() {
            let window = app
                .get_webview_window("main")
                .ok_or_else(|| "main window is active but unavailable for feedback".to_string())?;
            window
                .emit(
                    "desktop-notification",
                    InAppNotification {
                        title: notification.title,
                        body: notification.body,
                        error: notification.error || notification.category.is_error(),
                    },
                )
                .map_err(|error| format!("failed to show in-app notification: {error}"))?;
        }
        return Ok(DesktopNotificationDelivery::MainWindowActive);
    }
    let (title, body) = if preferences.show_previews {
        (notification.title, notification.body)
    } else {
        (
            notification.category.private_title(snapshot.language),
            localized(
                snapshot.language,
                "打开 ArcRelay 查看详情。",
                "Open ArcRelay to view details.",
            ),
        )
    };
    if let Some(action) = notification.action {
        show_actionable(app, title, body, action, snapshot.language)?;
    } else {
        show_unfiltered(app, title, body)?;
    }
    Ok(DesktopNotificationDelivery::Shown)
}

/// Attention events deliver their visual feedback before playing audio.
/// Passive events preserve the existing audio-first behavior.
pub fn show_with_sound(
    app: &AppHandle,
    settings: &SettingsManager,
    notification: DesktopNotification,
    sound: crate::sound::SoundEvent,
    automation_id: Option<&str>,
) -> Result<DesktopNotificationDelivery, String> {
    let play = || {
        if let Some(id) = automation_id {
            crate::sound::play_automation(sound, id);
        } else {
            crate::sound::play(sound);
        }
    };
    if sound.requires_visible_feedback() {
        let delivery = show(app, settings, notification)?;
        if delivery != DesktopNotificationDelivery::Disabled {
            play();
        }
        Ok(delivery)
    } else {
        play();
        show(app, settings, notification)
    }
}

static PENDING_TRANSFER_REQUEST: Mutex<Option<String>> = Mutex::new(None);

pub fn take_pending_transfer_request() -> Option<String> {
    PENDING_TRANSFER_REQUEST.lock().ok()?.take()
}

// Both visual-notification paths use the same silent adapter. macOS leaves
// sound unset; notify-rust's Windows adapter uses Toast::sound(None). XDG needs
// an explicit suppress-sound hint. Audio belongs exclusively to SoundService.
fn silent_notification(
    app: &AppHandle,
    title: &str,
    body: &str,
) -> Result<notify_rust::Notification, String> {
    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(body);
    #[cfg(target_os = "windows")]
    {
        let executable =
            tauri::utils::platform::current_exe().map_err(|error| error.to_string())?;
        let directory = executable
            .parent()
            .ok_or_else(|| "failed to resolve the ArcRelay application directory".to_string())?
            .display()
            .to_string();
        let separator = std::path::MAIN_SEPARATOR;
        if !(directory.ends_with(&format!("{separator}target{separator}debug"))
            || directory.ends_with(&format!("{separator}target{separator}release")))
        {
            notification.app_id(&app.config().identifier);
        }
    }
    #[cfg(target_os = "macos")]
    {
        let identifier = if tauri::is_dev() {
            "com.apple.Terminal"
        } else {
            &app.config().identifier
        };
        let _ = notify_rust::set_application(identifier);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    notification.appname(&app.config().identifier);

    #[cfg(all(unix, not(target_os = "macos")))]
    notification.hint(notify_rust::Hint::SuppressSound(true));
    Ok(notification)
}

fn show_actionable(
    app: &AppHandle,
    title: String,
    body: String,
    action: DesktopNotificationAction,
    language: LanguagePreference,
) -> Result<(), String> {
    let mut notification = silent_notification(app, &title, &body)?;
    notification.action("open", &localized(language, "查看请求", "View request"));
    let handle = notification
        .show()
        .map_err(|error| format!("failed to send system notification: {error}"))?;
    let app = app.clone();
    std::thread::Builder::new()
        .name("arcrelay-notification-action".into())
        .spawn(move || {
            handle.wait_for_action(move |selected| {
                if selected == "__closed" {
                    return;
                }
                match action {
                    DesktopNotificationAction::OpenTransferRequest(transfer_id) => {
                        if let Ok(mut pending) = PENDING_TRANSFER_REQUEST.lock() {
                            *pending = Some(transfer_id.clone());
                        }
                        crate::windowing::request_main_window(app.clone());
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.emit("open-transfer-request", transfer_id);
                        }
                    }
                }
            });
        })
        .map_err(|error| format!("failed to listen for system notification actions: {error}"))?;
    Ok(())
}

pub fn show_test(app: &AppHandle, settings: &SettingsManager) -> Result<(), String> {
    let snapshot = settings.snapshot();
    if !snapshot.notifications.enabled {
        return Err("turn on desktop notifications first".to_string());
    }
    show_unfiltered(
        app,
        localized(
            snapshot.language,
            "ArcRelay 测试通知",
            "ArcRelay test notification",
        ),
        localized(
            snapshot.language,
            "通知机制工作正常，你可以在设置中调整接收范围。",
            "Notifications are working. You can choose which alerts to receive in Settings.",
        ),
    )
}

fn show_unfiltered(app: &AppHandle, title: String, body: String) -> Result<(), String> {
    app.notification()
        .request_permission()
        .map_err(|error| format!("failed to request notification permission: {error}"))?;
    silent_notification(app, &title, &body)?
        .show()
        .map(|_| ())
        .map_err(|error| format!("failed to send system notification: {error}"))
}

pub fn localized(language: LanguagePreference, zh_cn: &str, en_us: &str) -> String {
    crate::localization::translate(language, zh_cn, en_us)
}

pub struct HostNotificationTracker {
    known_ids: HashSet<String>,
}

impl HostNotificationTracker {
    pub fn from_notifications(notifications: &[HostNotification]) -> Self {
        Self {
            known_ids: notifications
                .iter()
                .map(|notification| notification.id.clone())
                .collect(),
        }
    }

    pub fn update(&mut self, notifications: &[HostNotification]) -> Vec<HostNotification> {
        let current_ids = notifications
            .iter()
            .map(|notification| notification.id.clone())
            .collect::<HashSet<_>>();
        let added = notifications
            .iter()
            .filter(|notification| {
                notification.read_at_ms.is_none() && !self.known_ids.contains(&notification.id)
            })
            .cloned()
            .collect();
        self.known_ids = current_ids;
        added
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notification::NotificationKind;

    fn host_notification(id: &str) -> HostNotification {
        HostNotification {
            id: id.into(),
            title: "title".into(),
            body: "body".into(),
            source: "test".into(),
            kind: NotificationKind::Info,
            reference: None,
            created_at_ms: 1,
            read_at_ms: None,
            read_by_device_id: None,
            read_by_device_name: None,
        }
    }

    #[test]
    fn category_defaults_prioritize_attention_and_failures() {
        let settings = NotificationPreferences::default();
        assert!(DesktopNotificationCategory::PairingRequest.enabled(&settings));
        assert!(DesktopNotificationCategory::WorkflowFailed.enabled(&settings));
        assert!(!DesktopNotificationCategory::WorkflowCompleted.enabled(&settings));
        assert!(!DesktopNotificationCategory::DeviceConnection.enabled(&settings));
    }

    #[test]
    fn foreground_fallback_covers_attention_categories() {
        use DesktopNotificationCategory as Category;
        for category in [
            Category::PairingRequest,
            Category::TransferRequest,
            Category::TransferFailed,
            Category::RemoteFileFailed,
            Category::PrintFailed,
            Category::WorkflowActionRequired,
            Category::WorkflowFailed,
            Category::InputPermissionRequired,
            Category::AgentNotification,
        ] {
            assert!(category.requires_foreground_feedback(), "{category:?}");
        }
        assert!(!Category::TransferCompleted.requires_foreground_feedback());
        assert!(!Category::DeviceConnection.requires_foreground_feedback());
    }

    #[test]
    fn host_tracker_only_returns_new_unread_notifications() {
        let first = host_notification("first");
        let mut tracker = HostNotificationTracker::from_notifications(std::slice::from_ref(&first));
        assert!(tracker.update(std::slice::from_ref(&first)).is_empty());

        let second = host_notification("second");
        assert_eq!(tracker.update(&[second.clone(), first]).len(), 1);
        assert!(tracker.update(std::slice::from_ref(&second)).is_empty());
    }
}
