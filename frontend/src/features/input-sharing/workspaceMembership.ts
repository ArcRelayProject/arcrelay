import type { RuntimeSnapshot, WorkspaceConfiguration } from "../../types.ts";

export function removeWorkspaceDisplay(configuration: WorkspaceConfiguration, id: string): WorkspaceConfiguration {
  const next = structuredClone(configuration);
  const display = next.layout?.displays[id];
  if (!display || !next.layout) return next;
  next.rememberedDisplays[id] = display;
  next.excludedDisplays = [...new Set([...next.excludedDisplays, id])];
  delete next.layout.displays[id];
  next.layout.portals = next.layout.portals.filter((portal) => portal.sourceDisplay !== id && portal.targetDisplay !== id);
  return next;
}

export function addWorkspaceDisplay(configuration: WorkspaceConfiguration, id: string): WorkspaceConfiguration {
  const next = structuredClone(configuration);
  const display = next.rememberedDisplays[id];
  if (!next.layout || !display || next.layout.displays[id]) return next;
  next.layout.displays[id] = structuredClone(display);
  next.excludedDisplays = next.excludedDisplays.filter((value) => value !== id);
  return next;
}

export function displayStatusLabel(state: RuntimeSnapshot["displayAvailability"][string] | undefined): string {
  switch (state) {
    case "Ready": return "在线";
    case "SharingDisabled": return "跨屏输入已关闭";
    case "PermissionRequired": return "输入权限不可用";
    case "DisplayDisconnected": return "屏幕未连接";
    case "Connecting": return "正在准备输入";
    default: return "离线";
  }
}
