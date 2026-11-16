import type { WorkspaceConfiguration } from "../../types.ts";

export function applyLayoutDraft(
  latest: WorkspaceConfiguration,
  draft: WorkspaceConfiguration,
): WorkspaceConfiguration {
  return {
    ...latest,
    layout: draft.layout,
    rememberedDisplays: { ...latest.rememberedDisplays, ...draft.rememberedDisplays },
    excludedDisplays: draft.excludedDisplays,
  };
}

export function applyKeyboardDraft(
  latest: WorkspaceConfiguration,
  draft: WorkspaceConfiguration,
): WorkspaceConfiguration {
  return {
    ...latest,
    keyboardProfiles: draft.keyboardProfiles,
    activeKeyboardProfile: draft.activeKeyboardProfile,
    rightOptionRawMode: draft.rightOptionRawMode,
    horizontalScrollBehavior: draft.horizontalScrollBehavior,
    consumerShortcuts: draft.consumerShortcuts,
  };
}
