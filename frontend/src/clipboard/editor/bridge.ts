import { invoke } from "../../ipc/client";
import type {
  ClipboardEditorDraft,
  ClipboardEditorSnapshot,
  ClipboardTextFormat,
} from "../../ipc/generated";
export const native = () => "__TAURI_INTERNALS__" in window;
export const editorBridge = {
  snapshot: async (): Promise<ClipboardEditorSnapshot> => {
    if (native()) return invoke("clipboard_editor_snapshot");
    return (await import("./mock")).snapshot();
  },
  preview: async (source: string, format: ClipboardTextFormat) => {
    if (native()) return invoke("clipboard_editor_preview", { source, format });
    return (await import("./mock")).preview(source, format);
  },
  save: async (draft: ClipboardEditorDraft) =>
    native() ? invoke("clipboard_editor_save", { draft }) : (await import("./mock")).save(draft),
  copy: async () => (native() ? invoke("clipboard_editor_copy") : (await import("./mock")).copy()),
  close: async () => {
    if (native()) await invoke("clipboard_editor_close");
    else window.close();
  },
  access: async () => {
    if (native()) await invoke("clipboard_editor_access");
  },
};
