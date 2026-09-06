import { mount } from "svelte";
import PrivacyOverlay from "./PrivacyOverlay.svelte";
import "./privacy-overlay.css";

const style = new URLSearchParams(window.location.search).get("style");
document.documentElement.dataset.maskStyle = style === "solid" ? "solid" : "frosted";

mount(PrivacyOverlay, { target: document.getElementById("privacy-overlay")! });
