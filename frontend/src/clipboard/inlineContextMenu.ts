import { get, writable } from 'svelte/store';

export type ContextMenuEntry = { item: 'Separator' } | {
  id?: string;
  text: string;
  enabled?: boolean;
  action?: () => void;
  items?: ContextMenuEntry[];
};
interface MenuLevel { items: ContextMenuEntry[]; title: string; active: number }
interface InlineMenu { x: number; y: number; levels: MenuLevel[] }
export const inlineContextMenu = writable<InlineMenu | null>(null);
let resolveClose: (() => void) | undefined;
const firstEnabled = (items: ContextMenuEntry[]) => items.findIndex(item => !('item' in item) && item.enabled !== false);

export function closeInlineContextMenu() {
  inlineContextMenu.set(null);
  const resolve = resolveClose;
  resolveClose = undefined;
  resolve?.();
}

export function showInlineContextMenu(items: ContextMenuEntry[], x: number, y: number) {
  closeInlineContextMenu();
  inlineContextMenu.set({ x, y, levels: [{ items, title: '', active: firstEnabled(items) }] });
  return new Promise<void>(resolve => { resolveClose = resolve; });
}

export function selectInlineMenu(index: number) {
  const menu = get(inlineContextMenu);
  if (!menu) return;
  const entry = menu.levels.at(-1)?.items[index];
  if (!entry || 'item' in entry || entry.enabled === false) return;
  if (entry.items) {
    inlineContextMenu.set({ ...menu, levels: [...menu.levels, {
      items: entry.items, title: entry.text, active: firstEnabled(entry.items),
    }] });
  } else {
    closeInlineContextMenu();
    entry.action?.();
  }
}

export function backInlineMenu() {
  inlineContextMenu.update(menu => menu && menu.levels.length > 1
    ? { ...menu, levels: menu.levels.slice(0, -1) } : menu);
}

export function hoverInlineMenu(index: number) {
  inlineContextMenu.update(menu => {
    if (!menu) return menu;
    const levels = [...menu.levels];
    levels[levels.length - 1] = { ...levels[levels.length - 1], active: index };
    return { ...menu, levels };
  });
}

/** Handles native hook events as well as DOM events, without focusing a window. */
export function handleInlineMenuKey(event: Pick<KeyboardEvent, 'key' | 'shiftKey' | 'preventDefault' | 'stopPropagation'>) {
  const menu = get(inlineContextMenu);
  if (!menu) return false;
  event.preventDefault();
  event.stopPropagation();
  const level = menu.levels[menu.levels.length - 1];
  if (event.key === 'Escape') closeInlineContextMenu();
  else if (event.key === 'ArrowLeft') backInlineMenu();
  else if (event.key === 'Enter' || event.key === 'ArrowRight' || event.key === ' ') selectInlineMenu(level.active);
  else if (event.key === 'ArrowUp' || event.key === 'ArrowDown' || event.key === 'Tab') {
    const choices = level.items.map((item, index) => !('item' in item) && item.enabled !== false ? index : -1).filter(index => index >= 0);
    if (choices.length) {
      const current = choices.indexOf(level.active);
      const delta = event.key === 'ArrowUp' || (event.key === 'Tab' && event.shiftKey) ? -1 : 1;
      hoverInlineMenu(choices[(current + delta + choices.length) % choices.length]);
    }
  }
  return true;
}
