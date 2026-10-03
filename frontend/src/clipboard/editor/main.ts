import { mount } from "svelte";
import EditorApp from "./EditorApp.svelte";
import "../../styles/base.css";
import "./editor.css";
mount(EditorApp, { target: document.getElementById("editor-root")! });
