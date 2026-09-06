import circleSvg from "../../assets/icons/circle.svg?raw";
import closeSvg from "../../assets/icons/x.svg?raw";
import crosshairSvg from "../../assets/icons/crosshair.svg?raw";
import editSvg from "../../assets/icons/edit.svg?raw";
import folderSvg from "../../assets/icons/folder.svg?raw";
import globeSvg from "../../assets/icons/globe.svg?raw";
import gripSvg from "../../assets/icons/grip.svg?raw";
import keyboardSvg from "../../assets/icons/keyboard.svg?raw";
import linkSvg from "../../assets/icons/link.svg?raw";
import masksSvg from "../../assets/icons/masks.svg?raw";
import playSvg from "../../assets/icons/play.svg?raw";
import radioSvg from "../../assets/icons/radio.svg?raw";
import scrollSvg from "../../assets/icons/scroll.svg?raw";
import settingsSvg from "../../assets/icons/settings.svg?raw";
import smartphoneSvg from "../../assets/icons/smartphone.svg?raw";
import terminalSvg from "../../assets/icons/terminal.svg?raw";
import toggleSvg from "../../assets/icons/toggle-left.svg?raw";
import zapSvg from "../../assets/icons/zap.svg?raw";

import type { ActionIconOption } from "./types";

export const actionIconCatalog: ActionIconOption[] = [
  { id: "zap", label: "闪电", svg: zapSvg },
  { id: "folder", label: "文件夹", svg: folderSvg },
  { id: "globe", label: "网页", svg: globeSvg },
  { id: "terminal", label: "终端", svg: terminalSvg },
  { id: "keyboard", label: "键盘", svg: keyboardSvg },
  { id: "scroll", label: "脚本", svg: scrollSvg },
  { id: "settings", label: "系统", svg: settingsSvg },
  { id: "play", label: "播放", svg: playSvg },
  { id: "radio", label: "音频", svg: radioSvg },
  { id: "link", label: "组合", svg: linkSvg },
  { id: "toggle", label: "开关", svg: toggleSvg },
  { id: "crosshair", label: "捕获", svg: crosshairSvg },
  { id: "smartphone", label: "移动设备", svg: smartphoneSvg },
  { id: "masks", label: "模式", svg: masksSvg },
  { id: "edit", label: "编辑", svg: editSvg },
  { id: "circle", label: "圆点", svg: circleSvg },
  { id: "grip", label: "控制", svg: gripSvg },
  { id: "close", label: "停止", svg: closeSvg },
];

export function iconSvgFor(iconId: string): string {
  return actionIconCatalog.find((icon) => icon.id === iconId)?.svg ?? zapSvg;
}

export function sanitizeImportedSvg(source: string): string {
  const trimmed = source.trim();
  if (new TextEncoder().encode(trimmed).length > 16 * 1024) {
    throw new Error("SVG icon cannot exceed 16 KiB");
  }
  if (!trimmed.startsWith("<svg") || trimmed.includes("<!DOCTYPE")) {
    throw new Error("select a standalone SVG file");
  }

  const document = new DOMParser().parseFromString(trimmed, "image/svg+xml");
  const parseError = document.querySelector("parsererror");
  const root = document.documentElement;
  if (parseError || root.localName !== "svg") {
    throw new Error("invalid SVG file");
  }

  const forbiddenElements = new Set([
    "script",
    "style",
    "link",
    "foreignobject",
    "iframe",
    "object",
    "embed",
    "image",
    "use",
    "audio",
    "video",
    "animate",
    "animatemotion",
    "animatetransform",
    "set",
  ]);
  for (const element of [root, ...root.querySelectorAll("*")]) {
    if (forbiddenElements.has(element.localName.toLowerCase())) {
      throw new Error(`SVG cannot contain a <${element.localName}> element`);
    }
    for (const attribute of [...element.attributes]) {
      const name = attribute.name.toLowerCase();
      const value = attribute.value.trim().toLowerCase();
      if (
        name.startsWith("on") ||
        name === "href" ||
        name === "src" ||
        name === "style" ||
        name === "xlink:href" ||
        value.includes("javascript:") ||
        value.includes("data:") ||
        value.includes("http://") ||
        value.includes("https://") ||
        value.includes("file:") ||
        (value.includes("url(") && !value.includes("url(#"))
      ) {
        throw new Error(`SVG contains an unsafe attribute: ${attribute.name}`);
      }
    }
  }
  return root.outerHTML;
}
