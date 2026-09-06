import { derived, writable } from "svelte/store";
import type { LanguagePreference } from "./types";
import { localeFor, resolveLanguage } from "./localization";
export * from "./localization";

let preference: LanguagePreference = "system";
export const language = writable(resolveLanguage(preference));
export const locale = derived(language, ($language) => localeFor($language));

export function setLanguage(next: LanguagePreference) {
  preference = next;
  const resolved = resolveLanguage(next);
  language.set(resolved);
  if (typeof document !== "undefined") document.documentElement.lang = localeFor(resolved);
}

if (typeof window !== "undefined") {
  const onLanguageChange = () => { if (preference === "system") setLanguage(preference); };
  window.addEventListener("languagechange", onLanguageChange);
  import.meta.hot?.dispose(() => window.removeEventListener("languagechange", onLanguageChange));
}
