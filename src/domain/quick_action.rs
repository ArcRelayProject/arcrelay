use serde::{Deserialize, Serialize};

/// A user-defined quick action that can be triggered from the mobile app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
pub struct QuickAction {
    #[serde(default)]
    pub id: String,
    /// Optimistic version, assigned by the catalog. Old configurations start at zero.
    #[serde(default)]
    pub revision: u64,
    pub name: String,
    pub icon_id: String,
    /// Sanitized SVG markup rendered by both desktop and mobile clients.
    #[serde(default)]
    pub icon_svg: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub sort_order: u32,
    #[serde(default)]
    pub source_preset_id: Option<String>,
    #[serde(default)]
    pub confirm_before_run: bool,
    /// Local desktop trigger, independent from a Hotkey action's output keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_shortcut: Option<String>,
    pub action_type: ActionType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS, schemars::JsonSchema)]
#[serde(tag = "type")]
pub enum ActionType {
    LaunchApp {
        #[serde(default)]
        app_name: String,
        #[serde(default)]
        app_path: Option<String>,
    },
    OpenPath {
        path: String,
    },
    OpenUrl {
        url: String,
    },
    ShellCommand {
        command: String,
        working_dir: Option<String>,
    },
    Hotkey {
        modifiers: Vec<String>,
        key: String,
    },
    AppleScript {
        script: String,
    },
    System {
        operation: SystemOperation,
    },
    Media {
        operation: MediaOperation,
    },
    SetSystemVolume {
        volume: u8,
    },
    SetSystemMuted {
        muted: bool,
    },
    SetMicrophone {
        active: bool,
    },
    /// Toggle mode: first tap starts a long-running process, second tap stops it.
    ToggleShellCommand {
        start_command: String,
        /// If empty, the running process is killed (SIGTERM).
        stop_command: Option<String>,
        working_dir: Option<String>,
    },
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SystemOperation {
    LockScreen,
    Sleep,
    DisplaySleep,
    Shutdown,
    Restart,
    ScreenshotFull,
    ScreenshotRegion,
}

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MediaOperation {
    TogglePlayPause,
    Play,
    Pause,
    Next,
    Previous,
    SeekForward,
    SeekBackward,
}

impl QuickAction {
    #[cfg(test)]
    pub fn new(name: String, group: String, action_type: ActionType) -> Self {
        Self {
            id: uuid_v4(),
            revision: 0,
            name,
            icon_id: "zap".to_string(),
            icon_svg: crate::domain::action_icon::builtin_svg("zap")
                .unwrap_or_default()
                .to_string(),
            color: "#6366F1".to_string(),
            group,
            sort_order: 0,
            source_preset_id: None,
            confirm_before_run: false,
            global_shortcut: None,
            action_type,
        }
    }

    pub fn is_toggle(&self) -> bool {
        matches!(self.action_type, ActionType::ToggleShellCommand { .. })
    }

    pub fn action_type_label(&self) -> &'static str {
        match &self.action_type {
            ActionType::LaunchApp { .. } => "启动应用",
            ActionType::OpenPath { .. } => "打开路径",
            ActionType::OpenUrl { .. } => "打开 URL",
            ActionType::ShellCommand { .. } => "Shell 命令",
            ActionType::Hotkey { .. } => "快捷键",
            ActionType::AppleScript { .. } => "AppleScript",
            ActionType::System { operation } => match operation {
                SystemOperation::LockScreen => "锁定电脑",
                SystemOperation::Sleep => "电脑睡眠",
                SystemOperation::DisplaySleep => "关闭显示器",
                SystemOperation::Shutdown => "关机",
                SystemOperation::Restart => "重新启动",
                SystemOperation::ScreenshotFull => "全屏截图",
                SystemOperation::ScreenshotRegion => "区域截图",
            },
            ActionType::Media { .. } => "媒体控制",
            ActionType::SetSystemVolume { .. } => "设置音量",
            ActionType::SetSystemMuted { muted } => {
                if *muted {
                    "系统静音"
                } else {
                    "取消静音"
                }
            }
            ActionType::SetMicrophone { active } => {
                if *active {
                    "开启麦克风"
                } else {
                    "麦克风静音"
                }
            }
            ActionType::ToggleShellCommand { .. } => "开关命令",
        }
    }

    pub fn requires_confirmation(&self) -> bool {
        self.confirm_before_run
            || matches!(
                self.action_type,
                ActionType::System {
                    operation: SystemOperation::Shutdown | SystemOperation::Restart
                }
            )
    }
}

#[cfg(test)]
fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_action_has_valid_uuid() {
        let action = QuickAction::new(
            "test".into(),
            "group".into(),
            ActionType::OpenUrl {
                url: "https://example.com".into(),
            },
        );
        // UUID v4 format: 8-4-4-4-12 hex chars
        assert_eq!(action.id.len(), 36);
        assert_eq!(&action.id[14..15], "4"); // version nibble
    }

    #[test]
    fn unique_ids() {
        let a = QuickAction::new(
            "a".into(),
            "g".into(),
            ActionType::OpenUrl { url: "u".into() },
        );
        let b = QuickAction::new(
            "b".into(),
            "g".into(),
            ActionType::OpenUrl { url: "u".into() },
        );
        assert_ne!(a.id, b.id);
    }

    #[test]
    fn action_type_label() {
        let action = QuickAction::new(
            "test".into(),
            "g".into(),
            ActionType::ShellCommand {
                command: "ls".into(),
                working_dir: None,
            },
        );
        assert_eq!(action.action_type_label(), "Shell 命令");
    }
}
