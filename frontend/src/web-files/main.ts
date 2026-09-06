import { mount } from "svelte";
import WebFilesApp from "./WebFilesApp.svelte";
import "./web-files.css";

mount(WebFilesApp, { target: document.getElementById("web-files-app")! });
