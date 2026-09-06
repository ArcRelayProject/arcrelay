import { invoke, listen } from './ipc/client';
import { type UnlistenFn } from "@tauri-apps/api/event";
import type { SystemShareRequest, TransferSnapshot } from "./types";
export const transfersBridge = {
    async getTransferState(): Promise<TransferSnapshot> {
        return invoke("get_transfer_state");
    },
    async onTransferState(listener: (state: TransferSnapshot) => void): Promise<UnlistenFn> {
        return (await import("./transferSubscription")).subscribeTransfer(listener);
    },
    async onOpenTransferRequest(listener: (transferId: string) => void): Promise<UnlistenFn> {
        return listen("open-transfer-request", ({ payload }) => listener(payload));
    },
    async takePendingTransferRequest(): Promise<string | null> {
        return invoke("take_pending_transfer_request");
    },
    async onOpenSystemShareRequest(listener: (request: SystemShareRequest) => void): Promise<UnlistenFn> {
        return listen("open-system-share-request", ({ payload }) => listener(payload));
    },
    async listSystemShareRequests(): Promise<SystemShareRequest[]> {
        return invoke("list_system_share_requests");
    },
    async submitSystemShareRequest(requestId: string, transferId: string, peerId: string): Promise<void> {
        await invoke("submit_system_share_request", { requestId, transferId, peerId });
    },
    async discardSystemShareRequest(requestId: string): Promise<void> {
        await invoke("discard_system_share_request", { requestId });
    },
    async refreshTransferDevices(): Promise<TransferSnapshot> {
        return invoke("refresh_transfer_devices");
    },
    async pickTransferFiles(): Promise<string[]> {
        return invoke("pick_transfer_files");
    },
    async inspectTransferFiles(paths: string[]): Promise<import('./ipc/generated').TransferDraftFile[]> {
        return invoke("inspect_transfer_files", { paths });
    },
    async sendTransfer(peerId: string, paths: string[], requestIds: string[] = []): Promise<string> {
        return requestIds.length
            ? invoke("send_system_share_transfer", { peerId, paths, requestIds })
            : invoke("send_transfer", { peerId, paths });
    },
    async respondTransfer(transferId: string, accepted: boolean, automaticReceive: boolean): Promise<void> {
        await invoke("respond_transfer", { transferId, accepted, automaticReceive });
    },
    async pauseTransfer(transferId: string): Promise<void> {
        await invoke("pause_transfer", { transferId });
    },
    async resumeTransfer(transferId: string): Promise<void> {
        await invoke("resume_transfer", { transferId });
    },
    async cancelTransfer(transferId: string): Promise<void> {
        await invoke("cancel_transfer", { transferId });
    },
    async chooseTransferReceiveDirectory(): Promise<TransferSnapshot> {
        return invoke("choose_transfer_receive_directory");
    },
    async openTransferReceiveDirectory(): Promise<void> {
        await invoke("open_transfer_receive_directory");
    },
    async setTransferReceivePolicy(peerId: string, automatic: boolean): Promise<TransferSnapshot> {
        return invoke("set_transfer_receive_policy", { peerId, automatic });
    }
};
