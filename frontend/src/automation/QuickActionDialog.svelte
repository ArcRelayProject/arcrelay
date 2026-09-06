<script lang="ts">
  import AppSelect from "../components/AppSelect.svelte";
  import { translate as uiTranslate, language as uiLanguage } from "../i18n";
  import { FolderOpen, Info } from "phosphor-svelte";
  import Modal from "./Modal.svelte";
  import { bridge } from "../bridge";
  import { errorMessage } from "../app_helpers";
  import type {
    ActionType,
    MediaOperation,
    SystemOperation,
  } from "../ipc/generated";
  import type { InstalledApp } from "../types";
  export let apps: InstalledApp[];
  export let close: () => void;
  export let save: (name: string, action: ActionType) => Promise<void>;
  let name = "";
  let type = "SetSystemVolume";
  let path = "";
  let url = "";
  let appPath = "";
  let volume = 50;
  let active = false;
  let muted = true;
  let media: MediaOperation = "pause";
  let system: SystemOperation = "lock_screen";
  let busy = false;
  let error = "";
  const choices = [
    { value: "LaunchApp", name: "启动应用" },
    { value: "OpenPath", name: "打开文件或文件夹" },
    { value: "OpenUrl", name: "打开网址" },
    { value: "SetSystemVolume", name: "设置系统音量" },
    { value: "SetSystemMuted", name: "系统静音" },
    { value: "SetMicrophone", name: "麦克风" },
    { value: "Media", name: "媒体控制" },
    { value: "System", name: "系统操作" },
  ];
  async function pick(directory: boolean) {
    try {
      const value = await bridge.pickAutomationPath(directory);
      if (value) path = value;
    } catch (e) {
      error = errorMessage(e);
    }
  }
  async function submit() {
    if (busy) return;
    busy = true;
    error = "";
    try {
      let action: ActionType;
      switch (type) {
        case "LaunchApp": {
          const app = apps.find((a) => a.path === appPath);
          if (!app) throw Error("select an installed application");
          action = {
            type: "LaunchApp",
            app_name: app.name,
            app_path: app.path,
          };
          break;
        }
        case "OpenPath":
          if (!path.trim()) throw Error("select a file or folder");
          action = { type: "OpenPath", path: path.trim() };
          break;
        case "OpenUrl": {
          let parsed: URL;
          try {
            parsed = new URL(url);
          } catch {
            throw Error("enter a complete URL, such as https://example.com");
          }
          if (!["https:", "http:"].includes(parsed.protocol))
            throw Error("only HTTP and HTTPS URLs are supported");
          action = { type: "OpenUrl", url: parsed.href };
          break;
        }
        case "SetSystemVolume":
          if (!Number.isInteger(volume) || volume < 0 || volume > 100)
            throw Error("volume must be an integer from 0 to 100");
          action = { type: "SetSystemVolume", volume };
          break;
        case "SetMicrophone":
          action = { type: "SetMicrophone", active };
          break;
        case "SetSystemMuted":
          action = { type: "SetSystemMuted", muted };
          break;
        case "Media":
          action = { type: "Media", operation: media };
          break;
        default:
          action = { type: "System", operation: system };
      }
      await save(
        name.trim() ||
          (type === "SetSystemVolume"
            ? `音量 ${volume}%`
            : choices.find((c) => c.value === type)!.name),
        action,
      );
      close();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<Modal title={uiTranslate("新建快捷动作", $uiLanguage)} {close} {busy}>
  <form on:submit|preventDefault={submit}>
    <p class="au-muted">{uiTranslate("创建后加入当前步骤，也会保存在“快捷动作”中。", $uiLanguage)}</p>
    <label class="au-field"
      >{uiTranslate("动作名称", $uiLanguage)}<input
        bind:value={name}
        placeholder={uiTranslate("例如：音量调到 50%", $uiLanguage)}
        maxlength={80}
      /></label
    >
    <label class="au-field"
      >{uiTranslate("操作类型", $uiLanguage)}<AppSelect bind:value={type} aria-label={uiTranslate("操作类型", $uiLanguage)}
        options={[
          ...choices.map((c) => ({ value: c.value, label: c.name })),
        ]}
      /></label
    >
    {#if type === "SetSystemVolume"}<div class="au-volume">
        <label class="au-field"
          >{uiTranslate("系统音量", $uiLanguage)}<input
            type="range"
            min="0"
            max="100"
            step="1"
            bind:value={volume}
          /></label
        ><label class="au-field"
          ><span class="au-sr-only">{uiTranslate("音量百分比", $uiLanguage)}</span><input
            type="number"
            min="0"
            max="100"
            step="1"
            bind:value={volume}
          /></label
        ><span>%</span>
      </div>
    {:else if type === "LaunchApp"}<label class="au-field"
        >{uiTranslate("应用", $uiLanguage)}<AppSelect bind:value={appPath} aria-label={uiTranslate("应用", $uiLanguage)}
          options={[
            { value: "", label: uiTranslate("选择已安装应用", $uiLanguage) },
            ...apps.map((app) => ({ value: app.path, label: app.name })),
          ]}
        /></label
      >
    {:else if type === "OpenPath"}<label class="au-field"
        >{uiTranslate("路径", $uiLanguage)}<input bind:value={path} placeholder={uiTranslate("选择文件或文件夹", $uiLanguage)} /></label
      >
      <div class="au-inline">
        <button type="button" class="au-button" on:click={() => pick(false)}
          >{uiTranslate("选择文件", $uiLanguage)}</button
        ><button type="button" class="au-button" on:click={() => pick(true)}
          ><FolderOpen size={18} />{uiTranslate("选择文件夹", $uiLanguage)}</button
        >
      </div>
    {:else if type === "OpenUrl"}<label class="au-field"
        >{uiTranslate("网址", $uiLanguage)}<input
          type="url"
          bind:value={url}
          placeholder="https://example.com"
        /></label
      >
    {:else if type === "SetMicrophone"}<label class="au-field"
        >{uiTranslate("麦克风", $uiLanguage)}<AppSelect bind:value={active} aria-label={uiTranslate("麦克风", $uiLanguage)}
          options={[
            { value: false, label: uiTranslate("静音", $uiLanguage) },
            { value: true, label: uiTranslate("开启", $uiLanguage) },
          ]}
        /></label
      >
    {:else if type === "SetSystemMuted"}<label class="au-field"
        >{uiTranslate("系统声音", $uiLanguage)}<AppSelect bind:value={muted} aria-label={uiTranslate("系统声音", $uiLanguage)}
          options={[
            { value: true, label: uiTranslate("静音", $uiLanguage) },
            { value: false, label: uiTranslate("取消静音", $uiLanguage) },
          ]}
        /></label
      >
    {:else if type === "Media"}<label class="au-field"
        >{uiTranslate("媒体操作", $uiLanguage)}<AppSelect bind:value={media} aria-label={uiTranslate("媒体操作", $uiLanguage)}
          options={[
            { value: "pause", label: uiTranslate("暂停", $uiLanguage) },
            { value: "play", label: uiTranslate("播放", $uiLanguage) },
            { value: "next", label: uiTranslate("下一首", $uiLanguage) },
            { value: "previous", label: uiTranslate("上一首", $uiLanguage) },
            { value: "toggle_play_pause", label: uiTranslate("播放 / 暂停切换", $uiLanguage) },
          ]}
        /></label
      >
    {:else}<label class="au-field"
        >{uiTranslate("系统操作", $uiLanguage)}<AppSelect bind:value={system} aria-label={uiTranslate("系统操作", $uiLanguage)}
          options={[
            { value: "lock_screen", label: uiTranslate("锁屏", $uiLanguage) },
            { value: "sleep", label: uiTranslate("睡眠", $uiLanguage) },
            { value: "display_sleep", label: uiTranslate("关闭显示器", $uiLanguage) },
            { value: "screenshot_full", label: uiTranslate("全屏截图", $uiLanguage) },
            { value: "screenshot_region", label: uiTranslate("区域截图", $uiLanguage) },
            { value: "shutdown", label: uiTranslate("关机（需确认）", $uiLanguage) },
            { value: "restart", label: uiTranslate("重启（需确认）", $uiLanguage) },
          ]}
        /></label
      >{/if}
    <p class="au-note">
      <Info size={17} />{uiTranslate("创建不会执行。系统动作在运行时按权限与安全规则检查。", $uiLanguage)}
    </p>
    {#if error}<p class="au-error" role="alert">{error}</p>{/if}
    <footer>
      <button type="button" class="au-button" disabled={busy} on:click={close}
        >{uiTranslate("取消", $uiLanguage)}</button
      ><button type="submit" class="au-button primary" disabled={busy}
        >{uiTranslate(busy ? "正在创建…" : "创建并加入步骤", $uiLanguage)}</button
      >
    </footer>
  </form>
</Modal>
