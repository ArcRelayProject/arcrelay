import { mount } from "svelte";
import WebFilesApp from "./WebFilesApp.svelte";
import "./web-files.css";
import { installVisualPreviewReadiness } from "../visualPreview";

mount(WebFilesApp, { target: document.getElementById("web-files-app")! });
installVisualPreviewReadiness();
