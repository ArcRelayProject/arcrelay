use serde::Serialize;

use crate::domain::action_icon;
use crate::domain::quick_action::{ActionType, MediaOperation, QuickAction, SystemOperation};

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActionPresetView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon_id: String,
    pub icon_svg: String,
    pub color: String,
    pub group: String,
    pub default_enabled: bool,
    pub requires_confirmation: bool,
    pub installed: bool,
    pub action_type: ActionType,
}

#[derive(Clone)]
pub struct ActionPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub icon_id: &'static str,
    pub color: &'static str,
    pub group: &'static str,
    pub default_enabled: bool,
    pub requires_confirmation: bool,
    pub action_type: ActionType,
}

impl ActionPreset {
    pub fn build_action(&self, sort_order: u32) -> QuickAction {
        let mut action = QuickAction {
            id: uuid::Uuid::new_v4().to_string(),
            revision: 0,
            name: self.name.to_string(),
            icon_id: self.icon_id.to_string(),
            icon_svg: String::new(),
            color: self.color.to_string(),
            group: self.group.to_string(),
            sort_order,
            source_preset_id: Some(self.id.to_string()),
            confirm_before_run: self.requires_confirmation,
            global_shortcut: None,
            action_type: self.action_type.clone(),
        };
        action_icon::normalize_action_icon(&mut action)
            .expect("built-in preset icons must always normalize");
        action
    }

    pub fn view(&self, installed: bool) -> ActionPresetView {
        let action = self.build_action(0);
        ActionPresetView {
            id: self.id.to_string(),
            name: self.name.to_string(),
            description: self.description.to_string(),
            icon_id: action.icon_id,
            icon_svg: action.icon_svg,
            color: self.color.to_string(),
            group: self.group.to_string(),
            default_enabled: self.default_enabled,
            requires_confirmation: self.requires_confirmation,
            installed,
            action_type: self.action_type.clone(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn preset(
    id: &'static str,
    name: &'static str,
    description: &'static str,
    icon_id: &'static str,
    color: &'static str,
    group: &'static str,
    default_enabled: bool,
    requires_confirmation: bool,
    action_type: ActionType,
) -> ActionPreset {
    ActionPreset {
        id,
        name,
        description,
        icon_id,
        color,
        group,
        default_enabled,
        requires_confirmation,
        action_type,
    }
}

pub fn catalog() -> Vec<ActionPreset> {
    let mut presets = vec![
        preset(
            "system.lock",
            "锁定电脑",
            "立即锁定当前用户会话。",
            "settings",
            "#2563EB",
            "系统",
            true,
            false,
            ActionType::System {
                operation: SystemOperation::LockScreen,
            },
        ),
        preset(
            "system.display_sleep",
            "关闭显示器",
            "让显示器立即进入休眠，电脑继续运行。",
            "settings",
            "#4F46E5",
            "系统",
            true,
            false,
            ActionType::System {
                operation: SystemOperation::DisplaySleep,
            },
        ),
        preset(
            "system.sleep",
            "电脑睡眠",
            "让电脑进入睡眠状态。",
            "settings",
            "#4338CA",
            "系统",
            false,
            true,
            ActionType::System {
                operation: SystemOperation::Sleep,
            },
        ),
        preset(
            "system.shutdown",
            "关闭电脑",
            "关闭当前电脑，执行前需要确认。",
            "settings",
            "#DC2626",
            "系统",
            false,
            true,
            ActionType::System {
                operation: SystemOperation::Shutdown,
            },
        ),
        preset(
            "system.restart",
            "重新启动",
            "重新启动当前电脑，执行前需要确认。",
            "toggle",
            "#EA580C",
            "系统",
            false,
            true,
            ActionType::System {
                operation: SystemOperation::Restart,
            },
        ),
        preset(
            "capture.full",
            "全屏截图",
            "截取整个屏幕并保存到系统图片目录。",
            "crosshair",
            "#0891B2",
            "截图",
            true,
            false,
            ActionType::System {
                operation: SystemOperation::ScreenshotFull,
            },
        ),
        preset(
            "capture.region",
            "区域截图",
            "打开当前选择的区域截图工具。",
            "crosshair",
            "#0E7490",
            "截图",
            true,
            false,
            ActionType::System {
                operation: SystemOperation::ScreenshotRegion,
            },
        ),
        preset(
            "media.toggle",
            "播放 / 暂停",
            "控制当前系统媒体会话。",
            "play",
            "#7C3AED",
            "媒体",
            true,
            false,
            ActionType::Media {
                operation: MediaOperation::TogglePlayPause,
            },
        ),
        preset(
            "media.previous",
            "上一曲",
            "切换到上一首媒体内容。",
            "play",
            "#8B5CF6",
            "媒体",
            true,
            false,
            ActionType::Media {
                operation: MediaOperation::Previous,
            },
        ),
        preset(
            "media.next",
            "下一曲",
            "切换到下一首媒体内容。",
            "play",
            "#8B5CF6",
            "媒体",
            true,
            false,
            ActionType::Media {
                operation: MediaOperation::Next,
            },
        ),
        preset(
            "volume.mute",
            "系统静音",
            "静音系统输出并保留当前音量值。",
            "radio",
            "#475569",
            "音频",
            true,
            false,
            ActionType::SetSystemMuted { muted: true },
        ),
        preset(
            "volume.unmute",
            "取消静音",
            "恢复系统声音。",
            "radio",
            "#16A34A",
            "音频",
            true,
            false,
            ActionType::SetSystemMuted { muted: false },
        ),
        preset(
            "volume.25",
            "音量 25%",
            "将系统音量设置为 25%。",
            "radio",
            "#0284C7",
            "音频",
            true,
            false,
            ActionType::SetSystemVolume { volume: 25 },
        ),
        preset(
            "volume.50",
            "音量 50%",
            "将系统音量设置为 50%。",
            "radio",
            "#0284C7",
            "音频",
            true,
            false,
            ActionType::SetSystemVolume { volume: 50 },
        ),
        preset(
            "volume.75",
            "音量 75%",
            "将系统音量设置为 75%。",
            "radio",
            "#0284C7",
            "音频",
            true,
            false,
            ActionType::SetSystemVolume { volume: 75 },
        ),
        preset(
            "microphone.mute",
            "麦克风静音",
            "关闭系统默认麦克风输入。",
            "radio",
            "#DC2626",
            "音频",
            true,
            false,
            ActionType::SetMicrophone { active: false },
        ),
        preset(
            "microphone.unmute",
            "开启麦克风",
            "恢复系统默认麦克风输入。",
            "radio",
            "#16A34A",
            "音频",
            true,
            false,
            ActionType::SetMicrophone { active: true },
        ),
        preset(
            "presentation.previous",
            "演示上一页",
            "向当前演示应用发送左方向键。",
            "play",
            "#D97706",
            "演示",
            true,
            false,
            ActionType::Hotkey {
                modifiers: vec![],
                key: "arrow_left".to_string(),
            },
        ),
        preset(
            "presentation.next",
            "演示下一页",
            "向当前演示应用发送右方向键。",
            "play",
            "#D97706",
            "演示",
            true,
            false,
            ActionType::Hotkey {
                modifiers: vec![],
                key: "arrow_right".to_string(),
            },
        ),
        preset(
            "presentation.exit",
            "退出演示",
            "向当前演示应用发送 Escape。",
            "close",
            "#B45309",
            "演示",
            true,
            false,
            ActionType::Hotkey {
                modifiers: vec![],
                key: "escape".to_string(),
            },
        ),
    ];

    let path_presets = [
        (
            "path.home",
            "打开主目录",
            "打开当前用户的主目录。",
            "folder",
            dirs::home_dir(),
        ),
        (
            "path.desktop",
            "打开桌面目录",
            "打开当前用户的桌面目录。",
            "folder",
            dirs::desktop_dir(),
        ),
        (
            "path.downloads",
            "打开下载目录",
            "打开当前用户的下载目录。",
            "folder",
            dirs::download_dir(),
        ),
        (
            "path.documents",
            "打开文稿目录",
            "打开当前用户的文稿目录。",
            "folder",
            dirs::document_dir(),
        ),
    ];
    for (id, name, description, icon_id, path) in path_presets {
        if let Some(path) = path {
            presets.push(preset(
                id,
                name,
                description,
                icon_id,
                "#2563EB",
                "文件",
                true,
                false,
                ActionType::OpenPath {
                    path: path.to_string_lossy().into_owned(),
                },
            ));
        }
    }

    presets
}

pub fn default_actions() -> Vec<QuickAction> {
    catalog()
        .into_iter()
        .filter(|preset| preset.default_enabled)
        .enumerate()
        .map(|(index, preset)| preset.build_action(index as u32))
        .collect()
}

pub fn find(id: &str) -> Option<ActionPreset> {
    catalog().into_iter().find(|preset| preset.id == id)
}
