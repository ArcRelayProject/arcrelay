<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import { CheckCircle, Link } from "phosphor-svelte";
  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  export let snapshot: RuntimeSnapshot;
  export let onSnapshot: (snapshot: RuntimeSnapshot) => void;
  export let notify: (message: string, error?: boolean) => void;
  let busyPeer = "";

  const labels: Record<string, string> = {
    canCapturePointer: "捕获指针", canCaptureKeyboard: "捕获键盘", canSuppressLocalInput: "抑制本机输入",
    canPlaceInternalBarrier: "内部边界", canInjectAbsolutePointer: "绝对指针注入", canInjectKeyboard: "键盘注入", canInjectAppPointer: "应用内鼠标",
    canControlElevatedApps: "高权限应用", canPersistPermission: "权限持久化",
    canCaptureNativeQuartzEvents: "原生 Quartz 捕获", canInjectNativeQuartzEvents: "原生 Quartz 注入",
    canCapturePrecisionTouchpadEvents: "精确触控板捕获", canInjectPrecisionTouchpadEvents: "精确触控板注入",
    canCaptureSystemGestures: "Mac 系统手势捕获（实验）", canInjectSystemGestures: "Mac 系统手势注入（实验）",
  };

  async function pairDevice(serviceInstanceId: string) {
    busyPeer = serviceInstanceId;
    try {
      await bridge.connectDesktopDevice(serviceInstanceId);
      notify("已发起统一设备配对，请核对六位配对码");
    } catch (error) { notify(String(error), true); }
    finally { busyPeer = ""; }
  }

  async function reconnect(peer: RuntimeSnapshot["nearbyPeers"][number]) {
    busyPeer = peer.serviceInstanceId;
    try {
      onSnapshot(await bridge.connectInputPeer(peer));
      notify("正在重新连接已配对设备");
    } catch (error) { notify(String(error), true); }
    finally { busyPeer = ""; }
  }

</script>

<section class="page">
  <header><h1>{uiTranslate("设置与能力", $uiLanguage)}</h1><p class="muted">{uiTranslate("跨屏输入复用 ArcRelay 根设备身份、统一配对、能力授权和 v1 实时会话。", $uiLanguage)}</p></header>
  <div class="settings-grid">
    <div class="card peers">
      <h3>{uiTranslate("附近设备", $uiLanguage)}</h3>
      <p class="muted">{uiTranslate("已配对设备直接复用统一授权；新设备只需完成一次配对。", $uiLanguage)}</p>
      {#if snapshot.nearbyPeers.length === 0}<div class="empty">{uiTranslate("尚未发现同一局域网中的设备。", $uiLanguage)}</div>{/if}
      {#each snapshot.nearbyPeers as peer}
        <div class="peer-row">
          <span class:online={peer.connected} class="dot"></span>
          <div><strong class="mono">{peer.serviceInstanceId}</strong><small>{peer.addresses.join(", ")}:{peer.port}</small>{#if peer.capabilities?.canInjectAppPointer}<small>{uiTranslate("仅应用内鼠标；键盘输入返回电脑", $uiLanguage)}</small>{:else if peer.capabilities && (!peer.capabilities.canInjectAbsolutePointer || !peer.capabilities.canInjectKeyboard)}<small class="warning">{uiTranslate("目标输入能力不足：", $uiLanguage)}{uiTranslate(peer.capabilities.limitation ?? "平台不支持完整注入", $uiLanguage)}</small>{/if}</div>
          {#if peer.connected}<span class="state"><CheckCircle size={16} />{uiTranslate("已连接", $uiLanguage)}</span>
          {:else if peer.paired}<button class="button secondary" disabled={busyPeer === peer.serviceInstanceId} on:click={() => reconnect(peer)}>{uiTranslate("重新连接", $uiLanguage)}</button>
          {:else}<button class="button" disabled={busyPeer === peer.serviceInstanceId} on:click={() => pairDevice(peer.serviceInstanceId)}><Link size={15} />{uiTranslate("统一配对", $uiLanguage)}</button>{/if}
        </div>
      {/each}
    </div>
    <div class="card identity"><h3>{uiTranslate("统一身份", $uiLanguage)}</h3><dl><dt>feature</dt><dd class="mono">{snapshot.productId}</dd><dt>device_id</dt><dd class="mono">{snapshot.serviceInstanceId}</dd><dt>{uiTranslate("控制 epoch", $uiLanguage)}</dt><dd>{uiTranslate(snapshot.controlEpoch ?? "未持有", $uiLanguage)}</dd></dl></div>
    <div class="card capabilities"><h3>{uiTranslate("平台能力", $uiLanguage)}</h3>{#each Object.entries(labels) as [key, label]}<div><span>{uiTranslate(label, $uiLanguage)}</span><strong class:yes={snapshot.capabilities[key as keyof typeof snapshot.capabilities] === true}>{uiTranslate(snapshot.capabilities[key as keyof typeof snapshot.capabilities] === true ? "支持" : "不可用", $uiLanguage)}</strong></div>{/each}{#if snapshot.capabilities.limitation}<p>{snapshot.capabilities.limitation}</p>{/if}</div>
    <div class="card safety"><h3>{uiTranslate("紧急恢复", $uiLanguage)}</h3><p class="muted">{uiTranslate("断线、睡眠、锁屏、权限丢失或退出都会触发 ReleaseAll。输入只允许控制器直达目标，不允许目标继续转发。", $uiLanguage)}</p><div class="shortcut">⌘⌥⇧ Esc <span>{uiTranslate("紧急释放（Windows/Linux 为 Ctrl+Alt+Shift+Esc）", $uiLanguage)}</span></div><div class="shortcut">⌘⌥⇧ M <span>{uiTranslate("释放控制并将光标移至当前本机屏幕中央（Windows/Linux 为 Ctrl+Alt+Shift+M）", $uiLanguage)}</span></div></div>
  </div>
</section>

<style>
  .page { height: 100%; overflow: auto; }
  h1 { margin: 0 0 6px; font-size: 25px; }
  header p { margin: 0 0 22px; font-size: 14px; }
  .settings-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }
  .settings-grid > div { padding: 22px; }
  .peers { grid-column: 1 / -1; }
  h3 { margin-top: 0; }
  .peer-row { display: grid; grid-template-columns: 10px 1fr auto; align-items: center; gap: 10px; padding: 12px 0; border-top: 1px solid #edf0f5; }
  .peer-row > div { display: grid; gap: 3px; min-width: 0; }
  .peer-row small { color: #7d8798; font-size: 11px; overflow-wrap: anywhere; }
  .peer-row small.warning { color: #b54708; }
  .peer-row button { display: inline-flex; align-items: center; gap: 6px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #c3cad5; }
  .dot.online { background: #22b66d; box-shadow: 0 0 0 4px #dcfae6; }
  .state { display: inline-flex; align-items: center; gap: 5px; color: #067647; font-size: 12px; font-weight: 700; }
  .empty { padding: 12px; border-radius: 9px; color: #667085; background: #f7f8fa; font-size: 12px; }
  dl { display: grid; grid-template-columns: 150px 1fr; gap: 12px; font-size: 13px; }
  dt { color: #7d8798; } dd { margin: 0; overflow-wrap: anywhere; }
  .capabilities > div { display: flex; justify-content: space-between; padding: 9px 0; border-top: 1px solid #edf0f5; font-size: 13px; }
  .capabilities strong { color: #9c2d2d; } .capabilities strong.yes { color: #067647; }
  .capabilities p { padding: 10px; border-radius: 9px; color: #8b4b14; background: #fff4e5; font-size: 12px; }
  .safety { grid-column: 1 / -1; }
  .shortcut { width: fit-content; margin-top: 18px; padding: 11px 14px; border-radius: 10px; color: #4338ca; background: #eeecff; font-weight: 800; }
  .shortcut span { margin-left: 12px; color: #6b7280; font-size: 12px; font-weight: 500; }
</style>
