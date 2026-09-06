import assert from "node:assert/strict";
import test from "node:test";
import { defaultSoundPreferences } from "./soundDefaults.ts";
import type { AppSettings, NotificationPreferences } from "./ipc/generated.ts";
import {
  detectPermissionPreset,
  detectReminderPreset,
  notificationPresetPatches,
  permissionPresets,
} from "./features/settings/settingsModels.ts";

const notifications: NotificationPreferences = {
  enabled: true,
  onlyWhenInactive: true,
  showPreviews: false,
  ...notificationPresetPatches.balanced,
} as NotificationPreferences;

test("balanced reminder preset matches product defaults and custom changes are detected", () => {
  const settings = { sounds: defaultSoundPreferences(), notifications } as AppSettings;
  assert.equal(detectReminderPreset(settings), "balanced");
  settings.notifications.deviceConnections = true;
  assert.equal(detectReminderPreset(settings), "custom");
});

test("agent permission presets remain distinct and detectable", () => {
  assert.equal(detectPermissionPreset(permissionPresets.readOnly), "readOnly");
  assert.equal(detectPermissionPreset(permissionPresets.daily), "daily");
  assert.equal(detectPermissionPreset(permissionPresets.full), "full");
  assert.equal(detectPermissionPreset({ ...permissionPresets.daily, scripts: true }), "custom");
});
