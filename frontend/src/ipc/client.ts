import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen, type EventCallback } from '@tauri-apps/api/event';
import type { CommandMap, EventMap, IpcError } from './generated';

export class CommandError extends Error {
  override toString() { return this.message; }
  readonly code: IpcError['code'];
  readonly retryable: boolean;
  readonly recoveryAction: IpcError['recoveryAction'];
  readonly errorId?: string;
  constructor(error: IpcError) { super(error.message); this.name = 'CommandError'; this.code = error.code; this.retryable = error.retryable; this.recoveryAction = error.recoveryAction; this.errorId = error.errorId ?? undefined; }
}
export function isCommandError(error: unknown, code: IpcError['code']): error is CommandError {
  return error instanceof CommandError && error.code === code;
}
type Result<C extends keyof CommandMap> = CommandMap[C]['result'] extends null ? void : CommandMap[C]['result'];
export async function invoke<C extends keyof CommandMap>(command: C, ...args: keyof CommandMap[C]['args'] extends never ? [] : [CommandMap[C]['args']]): Promise<Result<C>> {
  try { return await tauriInvoke<Result<C>>(command, args[0]); }
  catch (error) {
    if (typeof error === 'object' && error !== null && 'code' in error && 'message' in error && 'retryable' in error) throw new CommandError(error as IpcError);
    // Transport/plugin failures may precede the Rust command adapter.
    throw new Error(typeof error === 'string' ? error : String(error));
  }
}
export function listen<E extends keyof EventMap>(event: E, handler: EventCallback<EventMap[E]>) { return tauriListen<EventMap[E]>(event, handler); }
