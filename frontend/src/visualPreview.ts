import type { LanguagePreference, ThemePreference } from "./types";

const languages = new Set<LanguagePreference>([
  "zhCn", "enUs", "jaJp", "koKr", "deDe", "frFr", "esEs", "ptBr",
]);
const themes = new Set<ThemePreference>(["light", "dark"]);

function params() {
  return typeof window === "undefined" ? null : new URLSearchParams(window.location.search);
}

export function visualPreviewEnabled() {
  return params()?.get("visual") === "1";
}

export function visualPreviewLanguage(): LanguagePreference | null {
  const value = params()?.get("lang") as LanguagePreference | null;
  return value && languages.has(value) ? value : null;
}

export function visualPreviewTheme(): ThemePreference | null {
  const value = params()?.get("theme") as ThemePreference | null;
  return value && themes.has(value) ? value : null;
}

export function installVisualPreviewReadiness() {
  if (!visualPreviewEnabled()) return;
  const style = document.createElement("style");
  style.textContent = `
    *, *::before, *::after {
      animation-duration: 0s !important;
      animation-delay: 0s !important;
      caret-color: transparent !important;
      scroll-behavior: auto !important;
      transition-duration: 0s !important;
      transition-delay: 0s !important;
    }
  `;
  document.head.append(style);

  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  const finish = async () => {
    await document.fonts?.ready;
    await Promise.all(Array.from(document.images, (image) => image.complete
      ? image.decode?.().catch(() => undefined)
      : new Promise<void>((resolve) => {
          image.addEventListener("load", () => resolve(), { once: true });
          image.addEventListener("error", () => resolve(), { once: true });
        })));
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
    document.documentElement.dataset.visualReady = "true";
  };
  const schedule = () => {
    document.documentElement.dataset.visualReady = "false";
    if (settleTimer) clearTimeout(settleTimer);
    settleTimer = setTimeout(() => void finish(), 180);
  };
  const observer = new MutationObserver(schedule);
  observer.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
  schedule();
  setTimeout(() => {
    observer.disconnect();
    void finish();
  }, 4_000);
}
