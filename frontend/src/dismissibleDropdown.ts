export type DropdownOptions = {
  open: boolean;
  close: () => void;
};

/** For inline popups only. Portalled menus should use Bits UI's dismissal layer. */
export function dismissibleDropdown(node: HTMLElement, options: DropdownOptions) {
  const document = node.ownerDocument;
  let current = options;
  let listening = false;
  const listenerOptions = { capture: true };

  function setListening(next: boolean) {
    if (listening === next) return;
    listening = next;
    if (next) {
      document.addEventListener("pointerdown", outside, listenerOptions);
      document.addEventListener("focusin", outside, listenerOptions);
      document.addEventListener("keydown", escape, listenerOptions);
    } else {
      document.removeEventListener("pointerdown", outside, listenerOptions);
      document.removeEventListener("focusin", outside, listenerOptions);
      document.removeEventListener("keydown", escape, listenerOptions);
    }
  }

  function close() {
    const callback = current.close;
    current = { ...current, open: false };
    setListening(false);
    callback();
  }

  function outside(event: Event) {
    // Capture phase also catches controls that stop pointer events bubbling.
    if (current.open && !event.composedPath().includes(node)) close();
  }

  function escape(event: KeyboardEvent) {
    if (!current.open || event.key !== "Escape" || event.isComposing) return;
    event.preventDefault();
    event.stopPropagation();
    close();
    node.querySelector<HTMLElement>("button[aria-expanded], summary")?.focus();
  }

  setListening(current.open);
  return {
    update(options: DropdownOptions) {
      current = options;
      setListening(current.open);
    },
    destroy() { setListening(false); },
  };
}

/** Native details used as a floating action menu, not as an inline disclosure. */
export function dismissibleDetailsDropdown(node: HTMLDetailsElement) {
  const options = () => ({ open: node.open, close: () => { node.open = false; } });
  const dismissal = dismissibleDropdown(node, options());
  const toggle = () => dismissal.update(options());
  const select = (event: MouseEvent) => {
    if (event.target instanceof Element && event.target.closest("button:not(:disabled), a[href]")) {
      node.open = false;
      toggle();
    }
  };
  node.addEventListener("toggle", toggle);
  node.addEventListener("click", select);
  return {
    destroy() {
      dismissal.destroy();
      node.removeEventListener("toggle", toggle);
      node.removeEventListener("click", select);
    },
  };
}
