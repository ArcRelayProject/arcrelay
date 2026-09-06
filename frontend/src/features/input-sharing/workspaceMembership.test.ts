import assert from "node:assert/strict";
import test from "node:test";
import type { DisplaySurface, WorkspaceConfiguration, RuntimeSnapshot } from "../../types.ts";
import { addWorkspaceDisplay, removeWorkspaceDisplay, displayStatusLabel } from "./workspaceMembership.ts";
import { workspaceIsReady, recoverableOfflinePeer } from "./inputSharingUi.ts";

function fixture(): WorkspaceConfiguration {
  const screen: DisplaySurface = {
    displayId: "screen", deviceId: "device", name: "Screen", fingerprint: "screen",
    pixelSize: { width: 1920, height: 1080 }, logicalBounds: { x: 0, y: 0, width: 1920, height: 1080 },
    scaleFactor: 1, physicalSizeUm: { width: 500000, height: 300000 }, rotation: "Degrees0",
    deskRectUm: { x: 700000, y: -100000, width: 500000, height: 300000 },
    geometryConfidence: "UserCalibrated", inventoryRevision: 1,
  };
  return {
    version: 2, inputSharingEnabled: true, topologyAuthor: "device", keyboardProfiles: [],
    rememberedDisplays: {}, excludedDisplays: [], activeKeyboardProfile: "Productivity",
    rightOptionRawMode: true, horizontalScrollBehavior: "NativeScroll", consumerShortcuts: [],
    layout: { workspaceId: "workspace", revision: 1, displays: { screen }, portals: [] },
  };
}

test("removing the last screen retains its geometry and a durable exclusion", () => {
  const original = fixture();
  const next = removeWorkspaceDisplay(original, "screen");
  assert.deepEqual(next.layout?.displays, {});
  assert.deepEqual(next.excludedDisplays, ["screen"]);
  assert.deepEqual(next.rememberedDisplays.screen, original.layout?.displays.screen);
  assert.equal(Object.keys(original.layout!.displays).length, 1);
  assert.deepEqual(removeWorkspaceDisplay(next, "screen"), next);
});

test("adding restores the saved position and clears the removal decision", () => {
  const original = fixture();
  const added = addWorkspaceDisplay(removeWorkspaceDisplay(original, "screen"), "screen");
  assert.deepEqual(added.layout, original.layout);
  assert.deepEqual(added.excludedDisplays, []);
  assert.deepEqual(addWorkspaceDisplay(added, "unknown"), added);
});

test("removal drops only edges touching the selected screen", () => {
  const configuration = fixture();
  const portal = {
    portalId: "edge", sourceDisplay: "screen", targetDisplay: "other",
    sourceEdge: "Right", targetEdge: "Left", sourceSegment: { startUm: 0, endUm: 1000 },
    targetSegment: { startUm: 0, endUm: 1000 }, direction: "Bidirectional",
    activationPolicy: "Immediate", allowWhileDragging: false, insetUm: 0, hysteresisUm: 0, status: "Active",
  } as const;
  configuration.layout!.portals = [portal, { ...portal, portalId: "unrelated", sourceDisplay: "a", targetDisplay: "b" }];
  assert.deepEqual(removeWorkspaceDisplay(configuration, "screen").layout?.portals.map((edge) => edge.portalId), ["unrelated"]);
});

test("disabled input, offline devices, and missing permissions have distinct labels", () => {
  assert.equal(displayStatusLabel("SharingDisabled"), "跨屏输入已关闭");
  assert.equal(displayStatusLabel("Offline"), "离线");
  assert.equal(displayStatusLabel("PermissionRequired"), "输入权限不可用");
  assert.equal(displayStatusLabel("Connecting"), "正在准备输入");
});

test("an unrelated offline screen does not turn a usable workspace into global recovery", () => {
  const configuration = fixture();
  const local = configuration.layout!.displays.screen;
  configuration.layout!.displays.remote = { ...local, displayId: "remote", deviceId: "remote-device" };
  configuration.layout!.displays.offline = { ...local, displayId: "offline", deviceId: "offline-device" };
  configuration.layout!.portals = [{
    portalId: "online-edge", sourceDisplay: "screen", targetDisplay: "remote",
    sourceEdge: "Right", targetEdge: "Left", sourceSegment: { startUm: 0, endUm: 1000 },
    targetSegment: { startUm: 0, endUm: 1000 }, direction: "Bidirectional", activationPolicy: "Immediate",
    allowWhileDragging: false, insetUm: 0, hysteresisUm: 0, status: "Active",
  }];
  const snapshot: RuntimeSnapshot = {
    revision: 1, productId: "arc.input", serviceInstanceId: "device", localOperatingSystem: "MacOs",
    remoteOperatingSystems: {}, configuration, discoveredPeers: [], connectedPeers: ["remote-device"],
    displayAvailability: { screen: "Ready", remote: "Ready", offline: "Offline" },
    controller: null, controlEpoch: null, captureActive: true, diagnostics: [],
    capabilities: { canCapturePointer: true, canCaptureKeyboard: true, canSuppressLocalInput: true } as RuntimeSnapshot["capabilities"],
    nearbyPeers: [{ serviceInstanceId: "offline-device", displayName: "Offline", addresses: [], port: 0,
      certificateSha256: "", capabilityDigest: "", paired: true, connected: false, capabilities: null }],
  };
  assert.equal(workspaceIsReady(snapshot), true);
  assert.equal(recoverableOfflinePeer(snapshot), null);
  snapshot.displayAvailability.remote = "SharingDisabled";
  assert.equal(workspaceIsReady(snapshot), false);
  // Retaining a connection to an excluded device is not input readiness.
  snapshot.configuration = removeWorkspaceDisplay(configuration, "remote");
  assert.equal(workspaceIsReady(snapshot), false);
});
