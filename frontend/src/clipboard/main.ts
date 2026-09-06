import { mount } from "svelte";

import ClipboardApp from "./ClipboardApp.svelte";
import "./clipboard.css";

mount(ClipboardApp, {
  target: document.getElementById("clipboard-root")!,
});
