import { invoke, listen } from './ipc/client';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { PrinterSharingSnapshot, RemoteFileEntry, RemoteFileDirectoryPage, RemoteFileDragPreparation, RemoteFileOpenResult, RemoteFileShare, RemoteFileState, RemoteFileTransferSession, LocalSharedDirectory, WebAccessMode, WebGatewayStatus } from "./types";
import { type RemoteFileSortDirection, type RemoteFileSortKey } from "./remoteFileSort";
export const filesBridge = {
    listSystemFolders: () => invoke("list_system_folders"),
    addSystemFolder: (peerId: string, shareId: string) => invoke("add_system_folder", { peerId, shareId }),
    openSystemFolder: (id: string) => invoke("open_system_folder", { id }),
    removeSystemFolder: (id: string) => invoke("remove_system_folder", { id }),
    openSystemFolderRecovery: () => invoke("open_system_folder_recovery"),
    async stopRemoteEdit(peerId: string, shareId: string, relativePath: string): Promise<void> {
        await invoke("stop_remote_edit", { peerId, shareId, relativePath });
    },
    async installRemotePrinter(sourceDeviceId: string, shareId: string): Promise<PrinterSharingSnapshot> {
        return invoke("install_remote_printer", { sourceDeviceId, shareId });
    },
    async removeRemotePrinter(bindingId: string): Promise<PrinterSharingSnapshot> {
        return invoke("remove_remote_printer", { bindingId });
    },
    async getRemoteFileState(): Promise<RemoteFileState> {
        return invoke("get_remote_file_state");
    },
    async listRemoteFileShares(peerId: string): Promise<RemoteFileShare[]> {
        return invoke("list_remote_file_shares", { peerId });
    },
    async listRemoteDirectory(peerId: string, shareId: string, relativePath: string, cursor: string | null = null, search = "", sortKey: RemoteFileSortKey = "modified", sortDirection: RemoteFileSortDirection = "descending"): Promise<RemoteFileDirectoryPage> {
        return invoke("list_remote_directory", {
            peerId,
            shareId,
            relativePath,
            cursor,
            search: search.trim() || null,
            sortKey,
            sortDirection,
        });
    },
    async createRemoteDirectory(peerId: string, shareId: string, relativePath: string, name: string): Promise<RemoteFileEntry> {
        return invoke("create_remote_directory", { peerId, shareId, relativePath, name });
    },
    async renameRemoteEntry(peerId: string, shareId: string, relativePath: string, newName: string): Promise<RemoteFileEntry> {
        return invoke("rename_remote_entry", { peerId, shareId, relativePath, newName });
    },
    async deleteRemoteEntries(peerId: string, shareId: string, paths: string[]): Promise<Array<{
        path: string;
        error: string | null;
    }>> {
        return invoke("delete_remote_entries", { peerId, shareId, paths });
    },
    async deleteRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<void> {
        await invoke("delete_remote_entry", { peerId, shareId, relativePath });
    },
    async pickRemoteUpload(peerId: string, shareId: string, relativePath: string, folder: boolean): Promise<number> {
        return invoke("pick_remote_upload", { peerId, shareId, relativePath, folder });
    },
    async uploadRemotePaths(peerId: string, shareId: string, relativePath: string, paths: string[]): Promise<number> {
        return invoke("upload_remote_paths", { peerId, shareId, relativePath, paths });
    },
    async startRemoteUpload(peerId: string, shareId: string, relativePath: string, folder: boolean): Promise<RemoteFileTransferSession | null> {
        return invoke("start_remote_upload", { peerId, shareId, relativePath, folder });
    },
    async startRemoteUploadPaths(peerId: string, shareId: string, relativePath: string, paths: string[]): Promise<RemoteFileTransferSession> {
        return invoke("start_remote_upload_paths", { peerId, shareId, relativePath, paths });
    },
    async downloadRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<string | null> {
        return invoke("download_remote_entry", { peerId, shareId, relativePath });
    },
    async downloadRemoteEntries(peerId: string, shareId: string, relativePaths: string[]): Promise<string[] | null> {
        return invoke("download_remote_entries", { peerId, shareId, relativePaths });
    },
    async startRemoteDownloadEntries(peerId: string, shareId: string, relativePaths: string[]): Promise<RemoteFileTransferSession[] | null> {
        return invoke("start_remote_download_entries", { peerId, shareId, relativePaths });
    },
    async listRemoteFileTransfers(): Promise<RemoteFileTransferSession[]> {
        return invoke("list_remote_file_transfers");
    },
    async getRemoteFileThumbnail(peerId: string, shareId: string, relativePath: string, modifiedAtMs: number): Promise<string | null> {
        return invoke("get_remote_file_thumbnail", { peerId, shareId, relativePath, modifiedAtMs });
    },
    async openRemoteEntry(peerId: string, shareId: string, relativePath: string): Promise<RemoteFileOpenResult> {
        return invoke("open_remote_entry", { peerId, shareId, relativePath });
    },
    async prepareRemoteDrag(peerId: string, shareId: string, relativePath: string): Promise<RemoteFileDragPreparation> {
        return invoke("prepare_remote_drag", { peerId, shareId, relativePath });
    },
    async startRemoteFilePromiseDrag(peerId: string, shareId: string, relativePath: string, kind: RemoteFileEntry["kind"], size: number): Promise<void> {
        return invoke("start_remote_file_promise_drag", { peerId, shareId, relativePath, kind, size });
    },
    async addRemoteFileShare(): Promise<LocalSharedDirectory | null> {
        return invoke("add_remote_file_share");
    },
    async openFullDiskAccessSettings(): Promise<void> {
        await invoke("open_full_disk_access_settings");
    },
    async removeRemoteFileShare(shareId: string): Promise<LocalSharedDirectory[]> {
        return invoke("remove_remote_file_share", { shareId });
    },
    async setRemoteFileShareWritable(shareId: string, writable: boolean): Promise<LocalSharedDirectory> {
        return invoke("set_remote_file_share_writable", { shareId, writable });
    },
    async setRemoteFileShareWebPolicy(shareId: string, policy: {
        mode: WebAccessMode;
        listed: boolean;
        allowPreview: boolean;
        allowDownload: boolean;
    }, newPassword?: string): Promise<LocalSharedDirectory> {
        return invoke("set_remote_file_share_web_policy", {
            shareId,
            policy,
            newPassword: newPassword || null,
        });
    },
    async getWebGatewayStatus(): Promise<WebGatewayStatus> {
        return invoke("get_web_gateway_status");
    },
    async onWebGatewayStatus(listener: (status: WebGatewayStatus) => void): Promise<UnlistenFn> {
        return listen("web-gateway-status", (event) => listener(event.payload));
    },
    async openWebGatewayUrl(url: string): Promise<void> {
        await invoke("open_web_gateway_url", { url });
    }
};
