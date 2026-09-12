import { mount } from "svelte";
import PrivacyOverlay from "./PrivacyOverlay.svelte";
import "./privacy-overlay.css";
import { setLanguage } from "./i18n";
import { installVisualPreviewReadiness, visualPreviewLanguage } from "./visualPreview";

const style = new URLSearchParams(window.location.search).get("style");
document.documentElement.dataset.maskStyle = style === "solid" ? "solid" : "frosted";
setLanguage(visualPreviewLanguage() ?? "zhCn");

mount(PrivacyOverlay, { target: document.getElementById("privacy-overlay")! });
installVisualPreviewReadiness();
