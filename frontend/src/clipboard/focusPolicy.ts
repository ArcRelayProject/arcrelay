export function isWindowsClipboard(platform: string, native: boolean) {
  return native && /Win/i.test(platform);
}

/** Existing saved preferences must not reactivate the Windows chooser. */
export function autoFocusSearch(windowsClipboard: boolean, preference: boolean | undefined) {
  return !windowsClipboard && (preference ?? true);
}

export function editableElement(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof Element)) return null;
  const element = target.closest<HTMLElement>('input:not([type="checkbox"]):not([type="radio"]):not([type="button"]):not([type="submit"]):not([type="hidden"]), textarea, select, [contenteditable="true"]');
  if (!element || element.matches(':disabled, [readonly]')) return null;
  return element;
}

interface FocusPort {
  editing(editing: boolean): Promise<unknown>;
  activate(): Promise<unknown>;
  visible(): boolean;
  error(reason: unknown): void;
}

/** Serialize focus transitions. Never restore focus during an input-to-input
 * handoff, and never allow a late focus request to resurrect a hidden surface. */
export function installEditableFocus(port: FocusPort) {
  let desired = false;
  let chain = Promise.resolve(true);
  let disposed = false;
  let timer: number | undefined;
  const editing = (next: boolean) => {
    desired = next;
    chain = chain.then(async () => {
      if (disposed || (next && (!desired || !port.visible()))) return false;
      await port.editing(next);
      return true;
    }).catch(reason => { desired = false; port.error(reason); return false; });
    return chain;
  };
  const clear = () => window.clearTimeout(timer);
  const pointer = (event: PointerEvent) => {
    if (!port.visible()) return;
    clear();
    const input = editableElement(event.target);
    if (input) {
      void editing(true).then(success => {
        if (success && !disposed && desired && port.visible() && document.contains(input)) input.focus();
      });
    } else {
      if (editableElement(document.activeElement)) (document.activeElement as HTMLElement).blur();
      void editing(false).then(() => port.visible() && port.activate()).catch(port.error);
    }
  };
  const focus = (event: FocusEvent) => {
    if (editableElement(event.target) && port.visible()) { clear(); void editing(true); }
  };
  const blur = () => {
    clear();
    timer = window.setTimeout(() => {
      if (!editableElement(document.activeElement)) void editing(false);
    }, 80);
  };
  document.addEventListener('pointerdown', pointer, true);
  document.addEventListener('focusin', focus, true);
  document.addEventListener('focusout', blur, true);
  return {
    editing,
    pause() { clear(); desired = false; editableElement(document.activeElement)?.blur(); },
    destroy() {
      disposed = true;
      clear();
      document.removeEventListener('pointerdown', pointer, true);
      document.removeEventListener('focusin', focus, true);
      document.removeEventListener('focusout', blur, true);
    },
  };
}
