import { bridge } from './bridge';
import { TransferDraft } from './transferDraft';

// Retained across page navigation, deliberately not persisted to localStorage.
export const transferDraft = new TransferDraft({
  inspect: paths => bridge.inspectTransferFiles(paths),
  send: (peerId, paths, requestIds) => bridge.sendTransfer(peerId, paths, requestIds),
});
