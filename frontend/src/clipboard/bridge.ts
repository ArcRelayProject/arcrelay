import type { clipboardBridge as tauriBridge } from './bridge.tauri';
// Load browser fixtures only in browser previews; production imports contain no mock logic.
const inTauri = () => '__TAURI_INTERNALS__' in window;
let adapter: Promise<typeof tauriBridge> | undefined;
const load = () => adapter ??= inTauri()
  ? import('./bridge.tauri').then(m => m.clipboardBridge)
  : import('./bridge.mock').then(m => m.clipboardBridge);
export const clipboardBridge: typeof tauriBridge = new Proxy({} as typeof tauriBridge, {
  get(_target, key: keyof typeof tauriBridge) {
    return (...args: unknown[]) => load().then(api => Reflect.apply(api[key], api, args));
  },
});
