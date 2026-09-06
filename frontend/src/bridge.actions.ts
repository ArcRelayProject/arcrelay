import { invoke, listen } from './ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { ActionPreset, ActionOutputEvent, BootstrapState, ImportActionsResponse, InstalledApp, QuickAction } from "./types";
export const actionsBridge = {
    async getActionPresets(): Promise<ActionPreset[]> {
        return invoke("get_action_presets");
    },
    async installActionPreset(presetId: string): Promise<BootstrapState> {
        return invoke("install_action_preset", { presetId });
    },
    async listInstalledApps(refresh = false): Promise<InstalledApp[]> {
        return invoke("list_installed_apps", { refresh });
    },
    async getInstalledAppIcon(path: string): Promise<string | null> {
        return invoke("get_installed_app_icon", { path });
    },
    async exportActionsText(): Promise<string> {
        return invoke("export_actions_text");
    },
    async importActionsText(text: string): Promise<ImportActionsResponse> {
        return invoke("import_actions_text", { text });
    },
    async onActionOutput(listener: (event: ActionOutputEvent) => void): Promise<UnlistenFn> {
        return listen("action-output", (event) => listener(event.payload));
    },
    async validateActionShortcut(actionId: string, shortcut: string): Promise<void> {
        await invoke("validate_action_shortcut", { actionId, shortcut });
    },
    async saveAction(action: QuickAction): Promise<BootstrapState> {
        return invoke("save_action", { action });
    },
    async deleteAction(actionId: string): Promise<BootstrapState> {
        return invoke("delete_action", { actionId });
    },
    async executeAction(actionId: string): Promise<string> {
        return invoke("execute_action", { actionId });
    },
    async getActionOutput(actionId: string): Promise<ActionOutputEvent> {
        return invoke("get_action_output", { actionId });
    }
};
