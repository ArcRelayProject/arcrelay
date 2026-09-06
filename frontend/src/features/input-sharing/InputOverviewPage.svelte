<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import {
    ArrowCounterClockwise,
    ArrowRight,
    ArrowsLeftRight,
    Check,
    CheckCircle,
    DesktopTower,
    Gauge,
    Gear,
    Keyboard,
    Laptop,
    Monitor,
    PlugsConnected,
    Pulse,
    ShieldCheck,
    WarningCircle,
    WifiHigh,
    WifiSlash,
  } from "phosphor-svelte";

  import type { RuntimeSnapshot } from "../../types";
  import { displayStatusLabel } from "./workspaceMembership";
  import {
    activePortalCount,
    canTakeInputControl,
    connectedRemotePeer,
    friendlyPeerName,
    formatInputRate,
    inputEventRate,
    inputLatencyValues,
    percentile,
    recoverableOfflinePeer,
    workspaceIsReady,
  } from "./inputSharingUi";

  export let snapshot: RuntimeSnapshot;
  export let onNavigate: (section: "workspace" | "keyboard" | "diagnostics") => void;
  export let onSetup: (step?: 1 | 2 | 3 | 4) => void;
  export let onToggleInputSharing: () => void;
  export let onRetry: () => Promise<void> = async () => {};
  export let onForget: () => Promise<void> = async () => {};

  $: layout = snapshot.configuration.layout;
  $: displays = Object.values(layout?.displays ?? {});
  $: localDisplays = displays.filter((display) => display.deviceId === snapshot.serviceInstanceId);
  $: remoteDisplays = displays.filter((display) => display.deviceId !== snapshot.serviceInstanceId);
  $: readyRemoteCount = new Set(remoteDisplays.filter((display) => snapshot.displayAvailability[display.displayId] === "Ready").map((display) => display.deviceId)).size;
  $: connectedPeer = connectedRemotePeer(snapshot);
  $: offlinePeer = recoverableOfflinePeer(snapshot);
  $: portals = layout?.portals ?? [];
  $: ready = workspaceIsReady(snapshot);
  $: enabled = snapshot.configuration.inputSharingEnabled;
  $: activeReady = ready && enabled;
  $: latencyValues = inputLatencyValues(snapshot.diagnostics);
  $: p95 = percentile(latencyValues, .95);
  $: eventRate = inputEventRate(snapshot.diagnostics);
  $: recentEvents = snapshot.diagnostics.slice(-4).reverse();
  $: lastConnection = recentEvents[0]?.timestampMs ?? Date.now() - 2 * 60 * 1000;
</script>

<section class="overview-page">
  {#if offlinePeer}
    <div class="recovery-card">
      <div class="recovery-copy">
        <div class="recovery-title"><span><WarningCircle size={27} weight="fill" /></span><div><h2>{friendlyPeerName(offlinePeer, snapshot)} {uiTranslate("已离线", $uiLanguage)}</h2><p>{uiTranslate("相关跨屏边缘已暂停，不会误将指针发送到其他设备。", $uiLanguage)}</p></div></div>
        <div class="reconnect-state"><ArrowCounterClockwise size={21} /><div><strong>{uiTranslate("可以重新连接", $uiLanguage)}</strong><small>{uiTranslate("布局和键盘设置已保留", $uiLanguage)}</small></div></div>
        <div class="recovery-actions"><button class="primary" on:click={onRetry}><ArrowCounterClockwise size={17} />{uiTranslate("立即重试", $uiLanguage)}</button><button class="secondary" on:click={() => onNavigate("diagnostics")}><WifiSlash size={17} />{uiTranslate("检查网络", $uiLanguage)}</button><button class="text-danger" on:click={onForget}>{uiTranslate("移除设备", $uiLanguage)}</button></div>
      </div>
      <div class="recovery-map">
        <article><span class="online-dot"></span><Monitor size={62} weight="duotone" /><strong>{uiTranslate("这台 Mac", $uiLanguage)}</strong></article>
        <div class="paused-link"><ArrowsLeftRight size={25} /><span>{uiTranslate("已暂停", $uiLanguage)}</span></div>
        <article class="offline"><span></span><DesktopTower size={62} weight="duotone" /><strong>{friendlyPeerName(offlinePeer, snapshot)}</strong><small>{uiTranslate("离线", $uiLanguage)}</small></article>
      </div>
    </div>

    <div class="recovery-grid">
      <div class="card affected-card"><h3>{uiTranslate("受影响的项目", $uiLanguage)}</h3>{#each portals as portal}<div class="affected-row"><Monitor size={18} /><span>{uiTranslate(layout?.displays[portal.sourceDisplay]?.name ?? "屏幕", $uiLanguage)} → {uiTranslate(layout?.displays[portal.targetDisplay]?.name ?? "屏幕", $uiLanguage)}</span><em>{uiTranslate("已暂停", $uiLanguage)}</em></div>{/each}<p>{uiTranslate("设备恢复后自动启用。", $uiLanguage)}</p></div>
      <div class="card kept-card"><h3>{uiTranslate("保留的配置", $uiLanguage)}</h3><span><CheckCircle size={18} weight="fill" />{uiTranslate("屏幕位置", $uiLanguage)}</span><span><CheckCircle size={18} weight="fill" />{uiTranslate("键盘行为", $uiLanguage)}</span><span><CheckCircle size={18} weight="fill" />{uiTranslate("边缘设置", $uiLanguage)}</span><p>{uiTranslate("无需重新配置", $uiLanguage)}</p></div>
      <div class="card reconcile-card"><h3>{uiTranslate("如果设备发生变化", $uiLanguage)}</h3><p>{uiTranslate("更换显示器或系统重装后，可能需要重新匹配屏幕身份。", $uiLanguage)}</p><button class="secondary" on:click={() => onNavigate("workspace")}>{uiTranslate("重新匹配屏幕", $uiLanguage)}</button></div>
    </div>
    <div class="safe-release"><span><ShieldCheck size={24} weight="fill" /></span><div><strong>{uiTranslate("输入已安全释放", $uiLanguage)}</strong><p>{uiTranslate("所有按键和鼠标按钮均已松开，本机控制已恢复。", $uiLanguage)}</p></div><CheckCircle size={24} weight="fill" /></div>
  {:else}
    <div class:ready={activeReady} class="readiness-card card">
      <div class="readiness-heading">
        <span class="readiness-icon">{#if activeReady}<Check size={30} weight="bold" />{:else}<WarningCircle size={30} weight="fill" />{/if}</span>
        <div><h2>{uiTranslate(activeReady ? "自动互通已就绪" : ready ? "工作区已就绪，等待启用" : "完成设置后即可互通", $uiLanguage)}</h2><p>{displays.length} {uiTranslate("块屏幕 ·", $uiLanguage)} {activePortalCount(snapshot)} {uiTranslate("条跨屏边缘 ·", $uiLanguage)} {readyRemoteCount} {uiTranslate("台远程设备在线", $uiLanguage)}</p></div>
      </div>

      <div class="workspace-summary" aria-label={uiTranslate("跨屏工作区概览", $uiLanguage)}>
        <div class="device-cluster">
          <header>{uiTranslate("这台 Mac", $uiLanguage)} <span class="online-dot"></span></header>
          <div class="screen-icons">
            {#each localDisplays as display, index}
              <article><svelte:component this={index === 1 ? Laptop : Monitor} size={62} weight="duotone" /><strong>{display.name}</strong><small>{uiTranslate(index === 0 ? "来源" : "本机", $uiLanguage)}</small></article>
            {/each}
            {#if localDisplays.length === 0}<article><Monitor size={62} weight="duotone" /><strong>{uiTranslate("本机显示器", $uiLanguage)}</strong></article>{/if}
          </div>
        </div>
        <div class:active={activePortalCount(snapshot) > 0} class="portal-summary"><ArrowsLeftRight size={32} weight="bold" /><strong>{activePortalCount(snapshot)} {uiTranslate("个边段", $uiLanguage)}</strong><small>{uiTranslate(activePortalCount(snapshot) ? "可穿越" : "屏幕尚未相邻", $uiLanguage)}</small></div>
        <div class="device-cluster remote">
          <header>{uiTranslate(connectedPeer ? friendlyPeerName(connectedPeer, snapshot) : "目标设备", $uiLanguage)} <span class:offline={!connectedPeer} class="online-dot"></span></header>
          <div class="screen-icons">
            {#each remoteDisplays as display}<article><Monitor size={62} weight="duotone" /><strong>{display.name}</strong><small>{uiTranslate(displayStatusLabel(snapshot.displayAvailability[display.displayId]), $uiLanguage)}</small></article>{/each}
            {#if remoteDisplays.length === 0}<article class="placeholder"><DesktopTower size={62} weight="duotone" /><strong>{uiTranslate("添加一台电脑", $uiLanguage)}</strong></article>{/if}
          </div>
        </div>
      </div>

      <div class="readiness-footer">
        <div class="checks">
          <span class:ok={snapshot.connectedPeers.length > 0}><CheckCircle size={18} weight="fill" />{uiTranslate("设备", $uiLanguage)}{uiTranslate(snapshot.connectedPeers.length ? "在线" : "未连接", $uiLanguage)}</span>
          <span class:ok={activePortalCount(snapshot) > 0}><CheckCircle size={18} weight="fill" />{uiTranslate("自动通道", $uiLanguage)}{uiTranslate(activePortalCount(snapshot) ? "可用" : "待排列", $uiLanguage)}</span>
          <span class:ok={canTakeInputControl(snapshot)}><CheckCircle size={18} weight="fill" />{uiTranslate("辅助功能", $uiLanguage)}{uiTranslate(canTakeInputControl(snapshot) ? "已开启" : "未开启", $uiLanguage)}</span>
          <span class:ok={enabled}><CheckCircle size={18} weight="fill" />{uiTranslate("跨屏输入", $uiLanguage)}{uiTranslate(enabled ? "持续待命" : "已关闭", $uiLanguage)}</span>
        </div>
        <div class="readiness-actions">
          {#if !snapshot.connectedPeers.length}<button class="primary" on:click={() => onSetup(2)}><PlugsConnected size={17} />{uiTranslate("添加设备", $uiLanguage)}</button>
          {:else if !activePortalCount(snapshot)}<button class="primary" on:click={() => onNavigate("workspace")}><ArrowsLeftRight size={17} />{uiTranslate("排列屏幕位置", $uiLanguage)}</button>
          {:else}<button class:controlling={enabled} class="primary" on:click={onToggleInputSharing}><PlugsConnected size={17} />{uiTranslate(enabled ? "关闭跨屏输入" : "启用自动互通", $uiLanguage)}</button>{/if}
          <button class="secondary" on:click={() => onNavigate("workspace")}>{uiTranslate("测试边缘", $uiLanguage)}</button>
        </div>
      </div>
    </div>

    {#if snapshot.controller}
      <div class="session-metrics">
        <article><Pulse size={24} /><span><small>{uiTranslate("输入", $uiLanguage)}</small><strong>{formatInputRate(eventRate)}</strong></span></article>
        <article><Gauge size={24} /><span><small>{uiTranslate("延迟", $uiLanguage)}</small><strong>{uiTranslate(p95 == null ? "等待样本" : `${p95.toFixed(1)} ms`, $uiLanguage)}</strong></span></article>
        <article><WifiHigh size={24} /><span><small>{uiTranslate("丢包", $uiLanguage)}</small><strong>{uiTranslate("未采样", $uiLanguage)}</strong></span></article>
        <div><ShieldCheck size={20} /><span>{uiTranslate("随时按", $uiLanguage)} <kbd>⌘⌥⇧ Esc</kbd> {uiTranslate("释放所有按键并返回本机", $uiLanguage)}</span></div>
      </div>
    {/if}

    <div class="overview-grid">
      <div class="card devices-card">
        <div class="card-title"><h3>{uiTranslate("设备", $uiLanguage)}</h3><button on:click={() => onSetup(2)}>{uiTranslate("管理设备", $uiLanguage)}<ArrowRight size={15} /></button></div>
        <div class="device-row"><span class="device-row-icon"><Monitor size={20} /></span><span><strong>{uiTranslate("这台 Mac", $uiLanguage)}</strong><small>{uiTranslate("可作为输入源和目标 ·", $uiLanguage)} {localDisplays.length || 1} {uiTranslate("块屏幕", $uiLanguage)}</small></span><em><i></i>{uiTranslate("在线", $uiLanguage)}</em></div>
        {#each snapshot.nearbyPeers as peer}
          <div class="device-row"><span class="device-row-icon"><DesktopTower size={20} /></span><span><strong>{friendlyPeerName(peer, snapshot)}</strong><small>{uiTranslate(peer.connected ? p95 == null ? "加密连接 · 等待延迟样本" : `加密连接 · ${p95.toFixed(0)} ms` : peer.paired ? "已配对 · 等待连接" : "尚未配对", $uiLanguage)}</small></span><em class:offline={!peer.connected}><i></i>{uiTranslate(peer.connected ? "在线" : "离线", $uiLanguage)}</em></div>
        {/each}
      </div>

      <div class="card activity-card">
        <div class="card-title"><h3>{uiTranslate("最近活动", $uiLanguage)}</h3><button on:click={() => onNavigate("diagnostics")}>{uiTranslate("查看诊断", $uiLanguage)}<ArrowRight size={15} /></button></div>
        {#if recentEvents.length === 0}
          <div class="activity-row"><i class="accent"></i><time>{new Date(lastConnection).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time><span>{uiTranslate("工作区已加载", $uiLanguage)}</span></div>
          <div class="activity-row"><i></i><time>{uiTranslate("刚刚", $uiLanguage)}</time><span>{uiTranslate("等待新的跨屏活动", $uiLanguage)}</span></div>
        {:else}
          {#each recentEvents as event, index}<div class="activity-row"><i class:accent={index === 0}></i><time>{new Date(event.timestampMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time><span>{event.message}</span></div>{/each}
        {/if}
      </div>

      <div class="card quick-card">
        <div class="card-title"><h3>{uiTranslate("快速操作", $uiLanguage)}</h3></div>
        <button on:click={() => onNavigate("workspace")}><ArrowsLeftRight size={20} /><span><strong>{uiTranslate("布局与通道", $uiLanguage)}</strong><small>{uiTranslate("按真实位置排列，自动生成穿越边段", $uiLanguage)}</small></span><ArrowRight size={17} /></button>
        <button on:click={() => onNavigate("keyboard")}><Keyboard size={20} /><span><strong>{uiTranslate("键盘行为", $uiLanguage)}</strong><small>{uiTranslate("适配不同系统的快捷键", $uiLanguage)}</small></span><ArrowRight size={17} /></button>
        <button on:click={() => onNavigate("diagnostics")}><Gear size={20} /><span><strong>{uiTranslate("连接诊断", $uiLanguage)}</strong><small>{uiTranslate("查看延迟、路径与事件", $uiLanguage)}</small></span><ArrowRight size={17} /></button>
      </div>
    </div>

    <div class="edge-hint"><PlugsConnected size={19} /><span>{uiTranslate(activePortalCount(snapshot) ? "从高亮边段即可按对应位置进入另一台电脑。" : "先在“布局与通道”中将两块屏幕按真实位置贴近。", $uiLanguage)}</span><button on:click={() => onNavigate("workspace")}>{uiTranslate("查看布局", $uiLanguage)}</button></div>
  {/if}
</section>

<style>
  .overview-page { display: flex; min-height: 0; flex-direction: column; gap: 16px; padding-bottom: 28px; }
  .card { border: 1px solid var(--border); border-radius: 15px; background: var(--surface-raised); box-shadow: var(--shadow-card); }
  h2, h3, p { margin-top: 0; }.readiness-card { overflow: hidden; }.readiness-heading { display: flex; align-items: center; gap: 15px; padding: 24px 28px 8px; }.readiness-icon { display: grid; width: 52px; height: 52px; place-items: center; flex: none; border-radius: 50%; color: var(--warning); background: var(--warning-soft); }.readiness-card.ready .readiness-icon { color: #fff; background: var(--success); }.readiness-heading h2 { margin-bottom: 5px; font-size: 22px; }.readiness-heading p { margin-bottom: 0; color: var(--text-secondary); font-size: 13px; }
  .workspace-summary { display: grid; grid-template-columns: minmax(260px,1fr) 110px minmax(220px,.75fr); align-items: center; gap: 18px; padding: 20px 28px 24px; }.device-cluster { padding: 16px; border: 1px solid var(--border); border-radius: 13px; background: var(--surface-soft); }.device-cluster header { display: flex; align-items: center; gap: 8px; margin-bottom: 13px; color: var(--text); font-size: 13px; font-weight: 700; }.online-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--success); box-shadow: 0 0 0 3px color-mix(in srgb, var(--success) 15%, transparent); }.online-dot.offline { background: var(--text-muted); box-shadow: none; }.screen-icons { display: flex; justify-content: center; gap: 18px; }.screen-icons article { display: grid; min-width: 92px; justify-items: center; gap: 4px; color: var(--accent); text-align: center; }.screen-icons strong { max-width: 145px; color: var(--text); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }.screen-icons small { color: var(--text-muted); font-size: 10px; }.screen-icons article.placeholder { color: var(--text-muted); }.portal-summary { display: grid; justify-items: center; gap: 4px; color: var(--text-muted); text-align: center; }.portal-summary.active { color: var(--accent); }.portal-summary strong { color: var(--text); font-size: 12px; }.portal-summary small { font-size: 10px; }
  .readiness-footer { display: flex; align-items: center; justify-content: space-between; gap: 18px; padding: 14px 22px; border-top: 1px solid var(--border); }.checks, .readiness-actions, .recovery-actions { display: flex; align-items: center; flex-wrap: wrap; gap: 12px; }.checks span { display: flex; align-items: center; gap: 6px; color: var(--text-muted); font-size: 11px; }.checks span.ok { color: var(--success); }.primary, .secondary, .text-danger { display: inline-flex; min-height: 38px; align-items: center; justify-content: center; gap: 7px; border-radius: 9px; padding: 0 14px; font-size: 12px; font-weight: 650; }.primary { border: 1px solid var(--accent); color: #fff; background: var(--accent); }.primary.controlling { border-color: var(--danger); background: var(--danger); }.secondary { border: 1px solid var(--border-strong); color: var(--text); background: var(--surface); }.text-danger { border: 0; color: var(--danger); background: transparent; }
  .session-metrics { display: grid; grid-template-columns: repeat(3,140px) minmax(260px,1fr); align-items: center; gap: 12px; padding: 14px 18px; border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border)); border-radius: 13px; background: var(--accent-soft); }.session-metrics article { display: flex; align-items: center; gap: 10px; color: var(--accent); }.session-metrics article span { display: grid; }.session-metrics small { color: var(--text-muted); }.session-metrics strong { color: var(--success); font-size: 17px; }.session-metrics > div { display: flex; align-items: center; justify-content: flex-end; gap: 8px; color: var(--text-secondary); font-size: 11px; }.session-metrics kbd { padding: 4px 7px; border: 1px solid color-mix(in srgb, var(--accent) 28%, var(--border)); border-radius: 6px; color: var(--accent-strong); background: var(--surface); font-family: inherit; font-weight: 700; }
  .overview-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; }.overview-grid > .card { padding: 20px; }.quick-card { grid-column: 1 / -1; }.card-title { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }.card-title h3 { margin: 0; font-size: 16px; }.card-title button { display: inline-flex; align-items: center; gap: 4px; border: 0; color: var(--accent-strong); background: transparent; font-size: 11px; }.device-row { display: grid; grid-template-columns: 38px minmax(0,1fr) auto; align-items: center; gap: 10px; min-height: 58px; border-top: 1px solid var(--border); }.device-row-icon { display: grid; width: 34px; height: 34px; place-items: center; border-radius: 9px; color: var(--accent); background: var(--accent-soft); }.device-row strong, .device-row small { display: block; }.device-row small { margin-top: 3px; color: var(--text-muted); font-size: 10px; }.device-row em { display: flex; align-items: center; gap: 6px; color: var(--success); font-size: 11px; font-style: normal; }.device-row em i { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }.device-row em.offline { color: var(--text-muted); }.activity-row { display: grid; grid-template-columns: 8px 70px minmax(0,1fr); align-items: center; gap: 9px; min-height: 46px; border-top: 1px solid var(--border); font-size: 11px; }.activity-row > i { width: 7px; height: 7px; border-radius: 50%; background: var(--text-muted); }.activity-row > i.accent { background: var(--accent); }.activity-row time { color: var(--text-muted); }.quick-card { display: grid; grid-template-columns: repeat(3, 1fr); column-gap: 10px; }.quick-card .card-title { grid-column: 1 / -1; }.quick-card > button { display: grid; grid-template-columns: 38px minmax(0,1fr) 18px; align-items: center; gap: 10px; min-height: 66px; padding: 10px 12px; border: 1px solid var(--border); border-radius: 11px; color: var(--accent); background: var(--surface); text-align: left; }.quick-card > button:hover { border-color: color-mix(in srgb,var(--accent) 36%,var(--border)); }.quick-card > button span { display: grid; gap: 3px; }.quick-card > button strong { color: var(--text); font-size: 12px; }.quick-card > button small { color: var(--text-muted); font-size: 10px; }.edge-hint { display: flex; min-height: 50px; align-items: center; gap: 9px; padding: 0 14px; border: 1px solid color-mix(in srgb,var(--accent) 24%,var(--border)); border-radius: 12px; color: var(--accent-strong); background: color-mix(in srgb,var(--accent) 5%,var(--surface)); font-size: 12px; }.edge-hint button { margin-left: auto; border: 0; color: var(--accent-strong); background: transparent; font-weight: 650; }
  .recovery-card { display: grid; grid-template-columns: minmax(380px,1fr) minmax(330px,.8fr); gap: 24px; padding: 28px; border: 1px solid color-mix(in srgb,var(--warning) 45%,var(--border)); border-radius: 16px; background: color-mix(in srgb,var(--warning-soft) 62%,var(--surface)); }.recovery-copy { display: grid; align-content: center; gap: 22px; }.recovery-title { display: flex; align-items: flex-start; gap: 14px; }.recovery-title > span { display: grid; width: 52px; height: 52px; place-items: center; flex: none; border-radius: 50%; color: var(--warning); background: var(--warning-soft); }.recovery-title h2 { margin: 3px 0 5px; font-size: 22px; }.recovery-title p, .reconnect-state small { margin: 0; color: var(--text-secondary); font-size: 12px; line-height: 1.55; }.reconnect-state { display: flex; align-items: center; gap: 11px; padding-top: 16px; border-top: 1px solid color-mix(in srgb,var(--warning) 20%,var(--border)); color: var(--accent); }.reconnect-state div { display: grid; gap: 3px; }.recovery-map { display: grid; grid-template-columns: 1fr 90px 1fr; align-items: center; gap: 8px; padding: 20px; border: 1px solid var(--border); border-radius: 14px; background: var(--surface); }.recovery-map article { display: grid; position: relative; justify-items: center; gap: 5px; color: var(--accent); text-align: center; }.recovery-map article strong { color: var(--text); font-size: 11px; }.recovery-map article small { color: var(--text-muted); font-size: 10px; }.recovery-map article.offline { opacity: .45; }.recovery-map article > span { position: absolute; top: 0; right: 10px; }.paused-link { display: grid; justify-items: center; gap: 5px; color: var(--warning); }.paused-link span { padding: 3px 7px; border-radius: 6px; background: var(--warning-soft); font-size: 10px; }.recovery-grid { display: grid; grid-template-columns: 1.4fr .7fr .9fr; gap: 16px; }.recovery-grid > .card { padding: 20px; }.recovery-grid h3 { margin-bottom: 13px; font-size: 15px; }.affected-row { display: grid; grid-template-columns: 22px minmax(0,1fr) auto; align-items: center; gap: 7px; min-height: 45px; border-top: 1px solid var(--border); font-size: 11px; }.affected-row em { color: var(--warning); font-style: normal; }.affected-card p, .kept-card p, .reconcile-card p { margin: 11px 0 0; color: var(--text-muted); font-size: 11px; line-height: 1.55; }.kept-card { display: grid; align-content: start; gap: 9px; }.kept-card span { display: flex; align-items: center; gap: 7px; color: var(--success); font-size: 12px; }.reconcile-card .secondary { margin-top: 18px; }.safe-release { display: grid; grid-template-columns: 46px minmax(0,1fr) 28px; align-items: center; gap: 12px; padding: 16px 20px; border: 1px solid color-mix(in srgb,var(--success) 35%,var(--border)); border-radius: 13px; color: var(--success); background: var(--success-soft); }.safe-release > span { display: grid; width: 42px; height: 42px; place-items: center; border-radius: 50%; color: #fff; background: var(--success); }.safe-release p { margin: 4px 0 0; color: var(--text-secondary); font-size: 11px; }
  @media (max-width: 1000px) { .workspace-summary { grid-template-columns: 1fr 72px 1fr; padding-inline: 18px; }.overview-grid, .recovery-card, .recovery-grid { grid-template-columns: 1fr; }.session-metrics { grid-template-columns: repeat(3,1fr); }.session-metrics > div { grid-column: 1/-1; justify-content: flex-start; }.quick-card { grid-template-columns: 1fr; }.quick-card .card-title { grid-column: 1; } }
</style>
