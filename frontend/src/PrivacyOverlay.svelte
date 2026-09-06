<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage, setLanguage } from "./i18n";
  import { SubscriptionScope, observeSnapshot } from "./subscriptions";
  import { onMount } from "svelte";
  import { invoke, listen } from "./ipc/client";
  import { EyeSlash } from "phosphor-svelte";
  import type { AppSettings, LanguagePreference } from "./types";

  const tr = (zh: string, en: string) => uiTranslate(zh, $uiLanguage);

  onMount(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const scope = new SubscriptionScope();
    void observeSnapshot<AppSettings>(scope,
      (accept) => listen("app-settings-changed", (event) => accept(event.payload)),
      () => invoke("get_app_settings"),
      (settings) => setLanguage(settings.language),
    ).catch(console.error);
    return scope.dispose;
  });
</script>

<main class="privacy-mask" aria-label={uiTranslate((tr("隐私内容已隐藏", "Private content is hidden")), $uiLanguage)}>
  <div class="privacy-mask-content">
    <EyeSlash size={42} weight="duotone" />
    <strong>{tr("隐私内容已隐藏", "Private content is hidden")}</strong>
    <span>{tr("ArcRelay 投屏隐私保护", "ArcRelay Screen Privacy")}</span>
  </div>
</main>
