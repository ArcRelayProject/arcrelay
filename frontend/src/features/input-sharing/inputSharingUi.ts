import type {
  DiagnosticRecord,
  DisplaySurface,
  KeyboardProfile,
  NearbyInputPeer,
  Portal,
  RuntimeSnapshot,
} from "../../types";

export const DEFAULT_KEYBOARD_PROFILES: KeyboardProfile[] = [
  { name: "日常办公", kind: "Productivity", revision: 1, textStrategy: "UseTargetLayout", semanticOverrides: [] },
  { name: "终端", kind: "Terminal", revision: 1, textStrategy: "UseTargetLayout", semanticOverrides: [] },
  { name: "开发工具", kind: "Ide", revision: 1, textStrategy: "UseTargetLayout", semanticOverrides: [] },
  { name: "远程桌面", kind: "RemoteDesktop", revision: 1, textStrategy: "FollowSourceText", semanticOverrides: [] },
  { name: "游戏 · 原始按键", kind: "GameRaw", revision: 1, textStrategy: "FollowSourceText", semanticOverrides: [] },
  { name: "演示", kind: "Presentation", revision: 1, textStrategy: "UseTargetLayout", semanticOverrides: [] },
];

export function canTakeInputControl(snapshot: RuntimeSnapshot) {
  return snapshot.capabilities.canCapturePointer
    && snapshot.capabilities.canCaptureKeyboard
    && snapshot.capabilities.canSuppressLocalInput;
}

export function isLocalDisplay(display: DisplaySurface, snapshot: RuntimeSnapshot) {
  return display.deviceId === snapshot.serviceInstanceId;
}

export function friendlyDeviceName(serviceInstanceId: string, snapshot: RuntimeSnapshot) {
  if (serviceInstanceId === snapshot.serviceInstanceId) return "这台 Mac";
  const peer = snapshot.nearbyPeers.find((value) => value.serviceInstanceId === serviceInstanceId);
  if (peer?.displayName?.trim()) return peer.displayName.trim();
  const display = Object.values(snapshot.configuration.layout?.displays ?? {})
    .find((value) => value.deviceId === serviceInstanceId);
  if (display?.name && !/^Display\s*\d+$/i.test(display.name)) {
    return display.name.includes("Mac") ? display.name : `${display.name} 所在电脑`;
  }
  const shortId = serviceInstanceId.length > 18 ? `${serviceInstanceId.slice(0, 8)}…` : serviceInstanceId;
  return peer ? `附近设备 ${shortId}` : `远程设备 ${shortId}`;
}

export function friendlyPeerName(peer: NearbyInputPeer, snapshot: RuntimeSnapshot) {
  return friendlyDeviceName(peer.serviceInstanceId, snapshot);
}

export function primaryAddress(peer: NearbyInputPeer) {
  return peer.addresses.find((address) => /^\d+\.\d+\.\d+\.\d+$/.test(address))
    ?? peer.addresses.find((address) => !address.startsWith("fe80"))
    ?? peer.addresses[0]
    ?? "本地网络";
}

export function activePortalCount(snapshot: RuntimeSnapshot) {
  return snapshot.configuration.layout?.portals.filter((portal) => portal.status === "Active").length ?? 0;
}

export function remoteDisplayCount(snapshot: RuntimeSnapshot) {
  return Object.values(snapshot.configuration.layout?.displays ?? {})
    .filter((display) => !isLocalDisplay(display, snapshot)).length;
}

export function connectedRemotePeer(snapshot: RuntimeSnapshot) {
  return snapshot.nearbyPeers.find((peer) => snapshot.connectedPeers.includes(peer.serviceInstanceId)
    && Object.values(snapshot.configuration.layout?.displays ?? {}).some((display) =>
      display.deviceId === peer.serviceInstanceId && snapshot.displayAvailability[display.displayId] === "Ready"))
    ?? null;
}

export function recoverableOfflinePeer(snapshot: RuntimeSnapshot) {
  // An unrelated offline island must not replace the usable workspace with a
  // global failure/recovery screen.
  if (workspaceIsReady(snapshot)) return null;
  const remoteDeviceIds = new Set(
    Object.values(snapshot.configuration.layout?.displays ?? {})
      .filter((display) => !isLocalDisplay(display, snapshot))
      .map((display) => display.deviceId),
  );
  return snapshot.nearbyPeers.find((peer) =>
    peer.paired
      && !peer.connected
      && remoteDeviceIds.has(peer.serviceInstanceId)) ?? null;
}

export function inputLatencyValues(records: DiagnosticRecord[]) {
  const endToEnd = records.filter((record) => record.category === "input-e2e");
  const source = endToEnd.length ? endToEnd : records.filter((record) => record.category === "input-latency");
  return source.flatMap((record) => record.latencyMicros == null ? [] : [record.latencyMicros / 1000]);
}

export function inputEventRate(records: DiagnosticRecord[]) {
  const endToEnd = records.filter((record) => record.category === "input-e2e");
  const samples = (endToEnd.length ? endToEnd : records.filter((record) => record.category === "input-latency"))
    .slice(-256);
  if (samples.length < 2) return null;
  const elapsedSeconds = (samples[samples.length - 1].timestampMs - samples[0].timestampMs) / 1000;
  return elapsedSeconds > 0 ? (samples.length - 1) / elapsedSeconds : null;
}

export function formatInputRate(rate: number | null) {
  if (rate == null) return "等待样本";
  if (rate < 1) return "< 1 Hz";
  return `${rate.toFixed(0)} Hz`;
}

export function portalEdgeTestRequest(
  portal: Portal,
  displays: Record<string, DisplaySurface>,
  reverse = false,
) {
  const displayId = reverse ? portal.targetDisplay : portal.sourceDisplay;
  const edge = reverse ? portal.targetEdge : portal.sourceEdge;
  const segment = reverse ? portal.targetSegment : portal.sourceSegment;
  const display = displays[displayId];
  if (!display) return null;
  const segmentCenter = Math.round((segment.startUm + segment.endUm) / 2);
  const horizontal = edge === "Left" || edge === "Right";
  const direction = edge === "Left" || edge === "Top" ? -1 : 1;
  return {
    displayId,
    pointXUm: horizontal ? display.deskRectUm.x + Math.round(display.deskRectUm.width / 2) : display.deskRectUm.x + segmentCenter,
    pointYUm: horizontal ? display.deskRectUm.y + segmentCenter : display.deskRectUm.y + Math.round(display.deskRectUm.height / 2),
    deltaXUm: horizontal ? direction * display.deskRectUm.width : 0,
    deltaYUm: horizontal ? 0 : direction * display.deskRectUm.height,
  };
}

export function percentile(values: number[], ratio: number) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(Math.floor(sorted.length * ratio), sorted.length - 1)];
}

export function displayDiagonalInches(display: DisplaySurface) {
  return Math.hypot(display.physicalSizeUm.width, display.physicalSizeUm.height) / 25_400;
}

export function displayStatusLabel(display: DisplaySurface, snapshot: RuntimeSnapshot) {
  if (isLocalDisplay(display, snapshot)) return "本机";
  return snapshot.connectedPeers.includes(display.deviceId) ? "在线" : "离线";
}

export function workspaceIsReady(snapshot: RuntimeSnapshot) {
  const layout = snapshot.configuration.layout;
  return Boolean(layout && canTakeInputControl(snapshot) && layout.portals.some((portal) => {
    const source = layout.displays[portal.sourceDisplay];
    const target = layout.displays[portal.targetDisplay];
    return portal.status === "Active" && source && target
      && source.deviceId !== target.deviceId
      && (source.deviceId === snapshot.serviceInstanceId || target.deviceId === snapshot.serviceInstanceId)
      && snapshot.displayAvailability[source.displayId] === "Ready"
      && snapshot.displayAvailability[target.displayId] === "Ready";
  }));
}
