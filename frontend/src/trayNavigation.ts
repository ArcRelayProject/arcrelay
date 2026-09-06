import type { SystemShareRequest } from './types';

export type TrayDestination = { page: 'settings' | 'devices' | 'transfers' } | { page: 'files'; peerId: string };

export function parseTrayDestination(value: string | null): TrayDestination | null {
  if (value === 'settings' || value === 'devices' || value === 'transfers') return { page: value };
  if (value?.startsWith('files:') && value.length > 6) return { page: 'files', peerId: value.slice(6) };
  return null;
}

// OS share targets may explicitly request an immediate send. Tray file pickers
// only stage files and suggest a recipient; they still need a click to send.
export function shouldAutoSendSystemShare(request: Pick<SystemShareRequest, 'source' | 'targetPeerId'>): boolean {
  return request.source !== 'tray' && Boolean(request.targetPeerId);
}
