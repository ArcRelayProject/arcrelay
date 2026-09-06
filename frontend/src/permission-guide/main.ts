import { mount } from "svelte";
import PermissionGuide from "./PermissionGuide.svelte";
import "./permission-guide.css";

mount(PermissionGuide, {
  target: document.getElementById("permission-guide")!,
});
