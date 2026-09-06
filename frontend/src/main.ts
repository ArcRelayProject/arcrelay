import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { installFrontendLogging } from "./logging";

installFrontendLogging();

mount(App, {
  target: document.getElementById("app")!,
});
