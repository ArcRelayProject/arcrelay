import { mount } from "svelte";

import ClipboardApp from "./ClipboardApp.svelte";
import "./clipboard.css";

// A browser tab has no native window bounds. Match the desktop panel instead
// of stretching a compact clipboard utility across the entire viewport.
if (!("__TAURI_INTERNALS__" in window)) {
  document.body.classList.add("clipboard-browser-preview");
}

mount(ClipboardApp, {
  target: document.getElementById("clipboard-root")!,
});
