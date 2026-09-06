import type { SoundPreferences } from "./ipc/generated";

export const defaultSoundPreferences = (): SoundPreferences => ({
  "enabled": true,
  "volume": 30,
  "muteDuringPrivacy": true,
  "mutedAutomationIds": [],
  "clipboardAdded": true,
  "clipboardReceived": false,
  "clipboardUsed": false,
  "transferRequest": true,
  "transferSent": true,
  "transferReceived": true,
  "transferFailed": true,
  "actionStarted": false,
  "actionSucceeded": true,
  "actionFailed": true,
  "automationStarted": false,
  "automationConfirmation": true,
  "automationSucceeded": false,
  "automationFailed": true,
  "automationInterrupted": true
});
