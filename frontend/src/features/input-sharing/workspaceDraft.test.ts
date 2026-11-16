import test from "node:test";
import assert from "node:assert/strict";
import type { WorkspaceConfiguration } from "../../types.ts";
import { applyKeyboardDraft, applyLayoutDraft } from "./workspaceDraft.ts";

const configuration = (changes: Partial<WorkspaceConfiguration>) =>
  ({
    inputSharingEnabled: true,
    layout: null,
    rememberedDisplays: {},
    excludedDisplays: [],
    keyboardProfiles: [],
    consumerShortcuts: [],
    rightOptionRawMode: true,
    ...changes,
  }) as WorkspaceConfiguration;

test("saving a layout draft preserves a newer disabled switch and keyboard settings", () => {
  const latest = configuration({
    inputSharingEnabled: false,
    activeKeyboardProfile: "Terminal",
    horizontalScrollBehavior: "NavigateHistory",
  });
  const draft = configuration({
    inputSharingEnabled: true,
    activeKeyboardProfile: "Productivity",
    excludedDisplays: ["removed-display"],
  });
  const saved = applyLayoutDraft(latest, draft);
  assert.equal(saved.inputSharingEnabled, false);
  assert.equal(saved.activeKeyboardProfile, "Terminal");
  assert.equal(saved.horizontalScrollBehavior, "NavigateHistory");
  assert.deepEqual(saved.excludedDisplays, ["removed-display"]);
});

test("saving keyboard settings preserves newly received topology and device membership", () => {
  const latest = configuration({
    inputSharingEnabled: false,
    layout: { workspaceId: "new-layout", revision: 9, displays: {}, portals: [] },
    excludedDisplays: ["offline-display"],
  });
  const draft = configuration({ activeKeyboardProfile: "Terminal", rightOptionRawMode: false });
  const saved = applyKeyboardDraft(latest, draft);
  assert.equal(saved.layout, latest.layout);
  assert.equal(saved.inputSharingEnabled, false);
  assert.deepEqual(saved.excludedDisplays, latest.excludedDisplays);
  assert.equal(saved.activeKeyboardProfile, "Terminal");
  assert.equal(saved.rightOptionRawMode, false);
});
