import type { LanguagePreference } from "./types";
import { languageOptions } from "./localization";
export const browserLanguageKey = "arcrelay.preview.language";
export function loadBrowserLanguage(): LanguagePreference {
  try {
    const saved = localStorage.getItem(browserLanguageKey);
    return saved === "system" || languageOptions.some(option => option.value === saved)
      ? saved as LanguagePreference : "system";
  } catch { return "system"; }
}
export function saveBrowserLanguage(value: LanguagePreference) {
  try { localStorage.setItem(browserLanguageKey, value); } catch { /* Private browsing may disable storage. */ }
}
