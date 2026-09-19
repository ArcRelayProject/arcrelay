import { translate } from "../../localization.ts";
import type { LanguagePreference } from "../../types";

// Translate only unchanged built-in metadata; renamed actions remain user data.
const presetDefaults: Record<string, { name: string; group: string }> = {
  "system.lock": { name: "锁定电脑", group: "系统" },
  "system.display_sleep": { name: "关闭显示器", group: "系统" },
  "system.sleep": { name: "电脑睡眠", group: "系统" },
  "system.shutdown": { name: "关闭电脑", group: "系统" },
  "system.restart": { name: "重新启动", group: "系统" },
  "capture.full": { name: "全屏截图", group: "截图" },
  "capture.region": { name: "区域截图", group: "截图" },
  "media.toggle": { name: "播放 / 暂停", group: "媒体" },
  "media.previous": { name: "上一曲", group: "媒体" },
  "media.next": { name: "下一曲", group: "媒体" },
  "volume.mute": { name: "系统静音", group: "音频" },
  "volume.unmute": { name: "取消静音", group: "音频" },
  "volume.25": { name: "音量 25%", group: "音频" },
  "volume.50": { name: "音量 50%", group: "音频" },
  "volume.75": { name: "音量 75%", group: "音频" },
  "microphone.mute": { name: "麦克风静音", group: "音频" },
  "microphone.unmute": { name: "开启麦克风", group: "音频" },
  "presentation.previous": { name: "演示上一页", group: "演示" },
  "presentation.next": { name: "演示下一页", group: "演示" },
  "presentation.exit": { name: "退出演示", group: "演示" },
  "path.home": { name: "打开主目录", group: "文件" },
  "path.desktop": { name: "打开桌面目录", group: "文件" },
  "path.downloads": { name: "打开下载目录", group: "文件" },
  "path.documents": { name: "打开文稿目录", group: "文件" },
};

type ActionMetadata = { source_preset_id?: string | null; name: string; group: string };
export function actionDisplayName(action: ActionMetadata, language: LanguagePreference) {
  return presetDefaults[action.source_preset_id ?? ""]?.name === action.name
    ? translate(action.name, language)
    : action.name;
}
export function actionDisplayGroup(action: ActionMetadata, language: LanguagePreference) {
  return presetDefaults[action.source_preset_id ?? ""]?.group === action.group
    ? translate(action.group, language)
    : action.group;
}
