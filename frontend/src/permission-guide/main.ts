import { mount } from "svelte";
import PermissionGuide from "./PermissionGuide.svelte";
import "./permission-guide.css";
import { installVisualPreviewReadiness } from "../visualPreview";

mount(PermissionGuide, {
  target: document.getElementById("permission-guide")!,
});

installVisualPreviewReadiness();
