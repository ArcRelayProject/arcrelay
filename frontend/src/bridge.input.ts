import { invoke, listen } from './ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { InputMetrics, InputRuntimeSnapshot, NearbyInputPeer, WorkspaceConfiguration } from "./types";
import { observeFrontendOperation } from "./logging";
export const inputBridge = {
    async getInputRuntimeSnapshot(): Promise<InputRuntimeSnapshot> {
        return invoke("get_input_runtime_snapshot");
    },
    async previewWorkspace(configuration: WorkspaceConfiguration): Promise<WorkspaceConfiguration> {
        return invoke("preview_input_workspace", { configuration });
    },
    async arrangeWorkspace(configuration: WorkspaceConfiguration): Promise<WorkspaceConfiguration> {
        return invoke("arrange_input_workspace", { configuration });
    },
    async saveWorkspace(configuration: WorkspaceConfiguration): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.workspace.save", () => invoke("save_input_workspace", { configuration }));
    },
    async setInputSharingEnabled(enabled: boolean): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.sharing.state", () => invoke("set_input_sharing_enabled", { enabled }));
    },
    async takeInputControl(): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.control.take", () => invoke("take_input_control"));
    },
    async releaseInputControl(): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.control.release", () => invoke("release_input_control"));
    },
    async connectInputPeer(peer: Pick<NearbyInputPeer, "serviceInstanceId" | "addresses" | "port">): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.peer.connect", () => invoke("connect_input_peer", {
            serviceInstanceId: peer.serviceInstanceId,
            addresses: peer.addresses,
            port: peer.port,
        }));
    },
    async forgetInputPeer(serviceInstanceId: string): Promise<InputRuntimeSnapshot> {
        return observeFrontendOperation("input.peer.forget", () => invoke("forget_input_peer", { serviceInstanceId }));
    },
    async onInputMetrics(listener: (metrics: InputMetrics | null) => void): Promise<UnlistenFn> {
        return listen("input-metrics", (event) => listener(event.payload));
    },
    async openInputPermissionSettings(): Promise<void> {
        await invoke("open_input_permission_settings");
    }
};
