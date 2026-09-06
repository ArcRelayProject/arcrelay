<script lang="ts">
  import AppSelect from "../../components/AppSelect.svelte";
  import type { RuntimeSnapshot, WorkspaceConfiguration } from '../../types';
  import type { ConsumerKey, ConsumerShortcut } from '../../ipc/generated';
  import { translate as t, language } from '../../i18n';
  export let snapshot: RuntimeSnapshot;
  export let configuration: WorkspaceConfiguration;
  export let onChange: () => void;
  const actions: { value: ConsumerKey; label: string }[] = [
    { value: 'BrightnessDown', label: '降低亮度' }, { value: 'BrightnessUp', label: '提高亮度' },
    { value: 'PreviousTrack', label: '上一首' }, { value: 'PlayPause', label: '播放 / 暂停' },
    { value: 'NextTrack', label: '下一首' }, { value: 'VolumeDown', label: '降低音量' },
    { value: 'VolumeUp', label: '提高音量' }, { value: 'Mute', label: '静音' },
  ];
  const keys = [{ value: 0x4b, label: 'PageUp' }, { value: 0x4e, label: 'PageDown' },
    ...Array.from({ length: 12 }, (_, i) => ({ value: 0x3a + i, label: `F${i + 1}` })),
    ...Array.from({ length: 26 }, (_, i) => ({ value: 4 + i, label: String.fromCharCode(65 + i) }))];
  function change() { configuration = { ...configuration, consumerShortcuts: [...configuration.consumerShortcuts] }; onChange(); }
  function add() {
    const availableKey = keys.find(key => !configuration.consumerShortcuts.some(binding => binding.key === key.value && binding.modifiers === 3));
    if (!availableKey || configuration.consumerShortcuts.length >= 8) return;
    configuration.consumerShortcuts = [...configuration.consumerShortcuts,
      { key: availableKey.value, modifiers: 3, action: configuration.consumerShortcuts.length ? 'BrightnessDown' : 'BrightnessUp' } satisfies ConsumerShortcut];
    change();
  }
  function remove(index: number) { configuration.consumerShortcuts = configuration.consumerShortcuts.filter((_, i) => i !== index); change(); }
  $: displays = Object.values(configuration.layout?.displays ?? {});
</script>

<section class="consumer-section">
  <h3>{t('系统功能键', $language)}</h3>
  <p>{t('媒体、音量和亮度跟随当前跨屏目标。目标不支持时保留本机行为；普通 F1–F12 不变。', $language)}</p>
  <div class="capabilities">
    {#each displays as display (display.displayId)}
      {@const caps = display.deviceId === snapshot.serviceInstanceId ? snapshot.capabilities : snapshot.nearbyPeers.find(peer => peer.serviceInstanceId === display.deviceId)?.capabilities}
      {@const ready = display.deviceId === snapshot.serviceInstanceId || snapshot.displayAvailability[display.displayId] === 'Ready'}
      <div><strong>{display.name}</strong><span>{t(ready && caps && (caps.consumerInjectMask & 63) === 63 ? '媒体 / 音量可用' : '媒体 / 音量未就绪', $language)}</span><span>{t(ready && caps?.brightnessDisplayIds.includes(display.displayId) ? '亮度可调' : '亮度未就绪或屏幕不支持', $language)}</span></div>
    {/each}
  </div>
  <h4>{t('功能键替代快捷键', $language)}</h4>
  <p>{t('用于无法截获亮度键的键盘。仅在控制支持该动作的远端时生效；修改后点击保存更改。', $language)}</p>
  {#each configuration.consumerShortcuts as binding, index}
    <div class="binding">
      <label>{t('动作', $language)}<AppSelect bind:value={binding.action} onValueChange={change} aria-label={(t('动作', $language))}
        options={[
          ...actions.map((action) => ({ value: action.value, label: (t(action.label, $language)) })),
        ]}
      /></label>
      <label>{t('修饰键', $language)}<AppSelect bind:value={binding.modifiers} onValueChange={change} aria-label={(t('修饰键', $language))}
        options={[
          { value: 3, label: "Ctrl + Alt/Option" },
          { value: 7, label: "Ctrl + Alt/Option + Shift" },
          { value: 5, label: "Ctrl + Shift" },
          { value: 10, label: "Command/Win + Alt/Option" },
          { value: 12, label: "Command/Win + Shift" },
        ]}
      /></label>
      <label>{t('按键', $language)}<AppSelect bind:value={binding.key} onValueChange={change} aria-label={(t('按键', $language))}
        options={[
          ...keys.map((key) => ({ value: key.value, label: key.label })),
        ]}
      /></label>
      <button type="button" on:click={() => remove(index)} aria-label={t('移除快捷键', $language)}>{t('移除', $language)}</button>
    </div>
  {/each}
  <button type="button" class="add" disabled={configuration.consumerShortcuts.length >= 8} on:click={add}>{t('添加功能键快捷键', $language)}</button>
</section>

<style>
  .consumer-section { display: grid; gap: 10px; margin-top: 20px; }
  h3, h4, p { margin: 0; } h3 { font-size: 14px; } h4 { font-size: 12px; margin-top: 8px; }
  p { color: var(--text-secondary); font-size: 11px; line-height: 1.6; }
  .capabilities { border: 1px solid var(--border); border-radius: 9px; overflow: hidden; }
  .capabilities > div { display: grid; grid-template-columns: 1fr 1fr 1.3fr; gap: 8px; padding: 10px; font-size: 11px; }
  .capabilities > div + div { border-top: 1px solid var(--border); } .capabilities span { color: var(--text-secondary); }
  .binding { display: grid; grid-template-columns: 1fr 1.5fr .8fr auto; align-items: end; gap: 8px; }
  label { display: grid; gap: 5px; color: var(--text-secondary); font-size: 11px; }
  :global(.app-select-trigger), button { min-width: 0; min-height: 34px; border: 1px solid var(--border); border-radius: 7px; background: var(--surface); color: var(--text); padding: 5px 8px; }
  .add { justify-self: start; color: var(--accent); } button:disabled { opacity: .5; }
  @media (max-width: 700px) { .binding { grid-template-columns: 1fr 1fr; } .capabilities > div { grid-template-columns: 1fr; } }
</style>
