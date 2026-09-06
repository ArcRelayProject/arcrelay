<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../../i18n";
  let last = "点击此处并按键";
  let active = false;

  function describe(event: KeyboardEvent) {
    event.preventDefault();
    const keys = [event.ctrlKey && "Ctrl", event.altKey && "Alt", event.shiftKey && "Shift", event.metaKey && "Command", event.key].filter(Boolean);
    last = keys.join(" + ");
  }
</script>

<div class:active class="tester" role="textbox" tabindex="0" on:focus={() => active = true} on:blur={() => active = false} on:keydown={describe}>
  <span>{uiTranslate("键盘测试器", $uiLanguage)}</span>
  <strong>{last}</strong>
  <small>{uiTranslate("这里只显示本地测试结果，不写入日志。", $uiLanguage)}</small>
</div>

<style>
  .tester { display: grid; place-items: center; gap: 8px; min-height: 145px; border: 2px dashed #ccd3e0; border-radius: 14px; outline: none; background: #fafbfc; }
  .tester.active { border-color: #635bdf; background: #f5f3ff; }
  span, small { color: #667085; font-size: 12px; }
  strong { font-size: 18px; }
  @media (prefers-color-scheme: dark) { .tester { background: #171e2a; border-color: #3a4558; } .tester.active { background: #211f38; } }
</style>
