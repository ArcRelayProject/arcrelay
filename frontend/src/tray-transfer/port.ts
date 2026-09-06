import type { AppSettings, TransferDraftFile, TransferSnapshot } from '../types';

export interface TrayTransferPort {
  preview: boolean;
  initialTransferId?: string;
  takeDrop(): Promise<{ revision: number; hovering: boolean; paths: string[] }>;
  onDrop(listener: () => void): Promise<() => void>;
  updatePanel(height: number, hasContent: boolean): Promise<void>;
  hide(): Promise<void>;
  history(): Promise<void>;
  inspect(paths: string[]): Promise<TransferDraftFile[]>;
  pick(): Promise<string[]>;
  send(peerId: string, paths: string[]): Promise<string>;
  cancel(id: string): Promise<void>;
  resume(id: string): Promise<void>;
  refresh(): Promise<TransferSnapshot>;
  onTransfer(listener: (snapshot: TransferSnapshot) => void): Promise<() => void>;
  getSettings(): Promise<AppSettings>;
  onSettings(listener: (settings: AppSettings) => void): Promise<() => void>;
}
