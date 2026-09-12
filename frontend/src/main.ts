import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { installFrontendLogging } from "./logging";
import { installVisualPreviewReadiness } from "./visualPreview";

installFrontendLogging();

mount(App, {
  target: document.getElementById("app")!,
});

installVisualPreviewReadiness();
