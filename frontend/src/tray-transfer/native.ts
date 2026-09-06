import { bridge } from '../bridge';
import { invoke, listen } from '../ipc/client';
import type { TrayTransferPort } from './port';

export const port: TrayTransferPort = {
  preview: false,
  takeDrop: () => invoke('take_tray_transfer_drop'),
  onDrop: listener => listen('tray-transfer-drop-changed', listener),
  updatePanel: (height, hasContent) => invoke('update_tray_transfer_panel', { height, hasContent }),
  hide: () => invoke('hide_tray_transfer_panel'),
  history: () => invoke('open_tray_transfer_history'),
  inspect: paths => bridge.inspectTransferFiles(paths),
  pick: () => bridge.pickTransferFiles(),
  send: (peerId, paths) => bridge.sendTransfer(peerId, paths),
  cancel: id => bridge.cancelTransfer(id),
  resume: id => bridge.resumeTransfer(id),
  refresh: () => bridge.refreshTransferDevices(),
  onTransfer: listener => bridge.onTransferState(listener),
  getSettings: () => bridge.getAppSettings(),
  onSettings: listener => bridge.onAppSettingsChanged(listener),
};
