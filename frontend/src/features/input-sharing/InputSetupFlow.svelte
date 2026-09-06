<script lang="ts">
  import { translate as uiTranslate, language as uiLanguage } from "../../i18n";
  import {
    ArrowLeft,
    ArrowRight,
    Check,
    CheckCircle,
    CircleNotch,
    DesktopTower,
    Globe,
    Laptop,
    Monitor as MonitorIcon,
    MouseSimple,
    ShieldCheck,
    X,
  } from "phosphor-svelte";

  import { bridge } from "../../bridge";
  import type { RuntimeSnapshot } from "../../types";
  import { canTakeInputControl, friendlyPeerName, primaryAddress } from "./inputSharingUi";

  export let snapshot: RuntimeSnapshot;
  export let onSnapshot: (snapshot: RuntimeSnapshot) => void;
  export let notify: (message: string, error?: boolean) => void;
  export let onClose: () => void;
  export let initialStep: 1 | 2 | 3 | 4 = 1;

  let step = initialStep;
  let selectedPeerId = snapshot.nearbyPeers.find((peer) => !peer.paired)?.serviceInstanceId
    ?? snapshot.nearbyPeers[0]?.serviceInstanceId
    ?? "";
  let busy = false;
  let discovering = false;

  $: selectedPeer = snapshot.nearbyPeers.find((peer) => peer.serviceInstanceId === selectedPeerId) ?? null;
  $: permissionReady = canTakeInputControl(snapshot);
  $: displays = Object.values(snapshot.configuration.layout?.displays ?? {});

  async function openPermissionSettings() {
    try {
      await bridge.openInputPermissionSettings();
      notify("已打开系统辅助功能设置");
    } catch (error) {
      notify(String(error), true);
    }
  }

  async function connectOrPair() {
    if (!selectedPeer) return;
    busy = true;
    try {
      if (selectedPeer.paired) {
        onSnapshot(await bridge.connectInputPeer(selectedPeer));
        notify("设备已重新连接");
      } else {
        await bridge.connectDesktopDevice(selectedPeer.serviceInstanceId);
        notify("已发起统一设备配对，请在设备设置中核对六位配对码");
      }
    } catch (error) {
      notify(String(error), true);
    } finally {
      busy = false;
    }
  }

  async function refreshNearby() {
    discovering = true;
    try {
      const next = await bridge.getInputRuntimeSnapshot();
      onSnapshot(next);
      snapshot = next;
      if (!next.nearbyPeers.some((peer) => peer.serviceInstanceId === selectedPeerId)) {
        selectedPeerId = next.nearbyPeers[0]?.serviceInstanceId ?? "";
      }
      notify("附近设备列表已刷新");
    } catch (error) {
      notify(String(error), true);
    } finally {
      discovering = false;
    }
  }

  function finish() {
    window.localStorage.setItem("arc-input-onboarding-complete", "1");
    onClose();
  }
</script>

<section class="setup-shell" aria-label={uiTranslate("跨屏输入设置向导", $uiLanguage)}>
  <header class="setup-header">
    <div>
      <h1>{uiTranslate(step === 1 ? "开启跨屏输入" : step === 2 ? "添加一台电脑" : step === 3 ? "排列屏幕" : "跨屏输入已就绪", $uiLanguage)}</h1>
      <p>{uiTranslate(step === 1 ? "在这台 Mac 和附近电脑之间共享鼠标与键盘" : step === 2 ? "选择同一局域网中的设备，并核对双方身份" : step === 3 ? "确认检测到的屏幕，随后按真实桌面位置排列" : "现在可以自然地在设备之间移动鼠标和键盘", $uiLanguage)}</p>
    </div>
    <button class="icon-button" aria-label={uiTranslate("关闭设置向导", $uiLanguage)} on:click={onClose}><X size={20} /></button>
  </header>

  <ol class="stepper" aria-label={uiTranslate("设置进度", $uiLanguage)}>
    {#each ["权限", "添加设备", "排列屏幕", "完成"] as label, index}
      <li class:active={step === index + 1} class:done={step > index + 1}>
        <span>{step > index + 1 ? "✓" : index + 1}</span>
        <strong>{uiTranslate(label, $uiLanguage)}</strong>
      </li>
    {/each}
  </ol>

  {#if step === 1}
    <div class="setup-card permission-layout">
      <div class="setup-visual" aria-hidden="true">
        <div class="device-pair"><Laptop size={82} weight="duotone" /><span></span><DesktopTower size={82} weight="duotone" /></div>
        <p>{uiTranslate("这台 Mac", $uiLanguage)} <small>{uiTranslate("可作为输入源", $uiLanguage)}</small></p>
      </div>
      <div class="setup-copy">
        <div class="section-title"><MouseSimple color="var(--accent)" size={24} weight="duotone" /><div><h2>{uiTranslate("允许控制鼠标和键盘", $uiLanguage)}</h2><p>{uiTranslate("ArcRelay 只转发输入事件，不记录按键内容、文字或剪贴板。", $uiLanguage)}</p></div></div>
        <div class="permission-list">
          <button on:click={openPermissionSettings}>
            <span class="row-icon"><MouseSimple size={21} /></span>
            <span><strong>{uiTranslate("辅助功能", $uiLanguage)}</strong><small>{uiTranslate("用于发送鼠标和键盘事件", $uiLanguage)}</small></span>
            <em class:ready={permissionReady}>{uiTranslate(permissionReady ? "已开启" : "需要开启", $uiLanguage)}</em>
            <ArrowRight size={17} />
          </button>
          <div class="permission-row">
            <span class="row-icon"><Globe size={21} /></span>
            <span><strong>{uiTranslate("本地网络", $uiLanguage)}</strong><small>{uiTranslate("用于发现和连接附近设备", $uiLanguage)}</small></span>
            <em class="ready">{uiTranslate("已允许", $uiLanguage)}</em>
            <Check size={17} />
          </div>
          <div class="permission-row">
            <span class="row-icon"><ShieldCheck size={21} /></span>
            <span><strong>{uiTranslate("屏幕录制", $uiLanguage)}</strong><small>{uiTranslate("跨屏输入不会读取屏幕内容", $uiLanguage)}</small></span>
            <em>{uiTranslate("不需要", $uiLanguage)}</em>
            <Check size={17} />
          </div>
        </div>
        <button class="primary wide" disabled={!permissionReady} on:click={() => step = 2}>{uiTranslate(permissionReady ? "继续添加设备" : "开启权限后继续", $uiLanguage)}<ArrowRight size={17} /></button>
        <button class="text-button" on:click={() => step = 2}>{uiTranslate("稍后设置权限", $uiLanguage)}</button>
      </div>
    </div>
    <div class="setup-note"><ShieldCheck color="var(--accent)" size={20} /><span>{uiTranslate("权限只用于跨屏输入，可随时在系统设置中关闭。", $uiLanguage)}</span><kbd>⌘⌥⇧ Esc</kbd><small>{uiTranslate("紧急释放", $uiLanguage)}</small></div>
  {:else if step === 2}
    <div class="pairing-layout">
      <div class="setup-card nearby-card">
        <div class="card-heading"><div><h2>{uiTranslate("附近设备", $uiLanguage)}</h2><p>{uiTranslate("正在通过本地网络发现设备，不经过云端。", $uiLanguage)}</p></div><button class="secondary compact" disabled={discovering} on:click={refreshNearby}><CircleNotch class={discovering ? "spinning" : ""} size={16} />{uiTranslate(discovering ? "刷新中" : "刷新", $uiLanguage)}</button></div>
        {#if snapshot.nearbyPeers.length === 0}
          <div class="empty-state"><Globe size={34} /><strong>{uiTranslate("尚未发现设备", $uiLanguage)}</strong><p>{uiTranslate("确认两台电脑连接同一网络，并已打开 ArcRelay。", $uiLanguage)}</p></div>
        {/if}
        <div class="nearby-list">
          {#each snapshot.nearbyPeers as peer}
            <button class:selected={selectedPeerId === peer.serviceInstanceId} on:click={() => selectedPeerId = peer.serviceInstanceId}>
              <span class="device-icon"><DesktopTower size={27} weight="duotone" /></span>
              <span><strong>{friendlyPeerName(peer, snapshot)}</strong><small>{primaryAddress(peer)} · {uiTranslate(peer.paired ? "已配对" : "新设备", $uiLanguage)}</small></span>
              <em class:online={peer.connected}>{uiTranslate(peer.connected ? "已连接" : peer.paired ? "可重连" : "可配对", $uiLanguage)}</em>
              <i aria-hidden="true"></i>
            </button>
          {/each}
        </div>
      </div>

      <div class="setup-card pairing-card">
        {#if selectedPeer}
          <div class="selected-device"><span class="device-icon"><DesktopTower size={30} weight="duotone" /></span><div><h2>{friendlyPeerName(selectedPeer, snapshot)}</h2><p>{primaryAddress(selectedPeer)} {uiTranslate("· 加密本地连接", $uiLanguage)}</p></div></div>
          {#if selectedPeer.connected}
            <div class="success-panel"><CheckCircle color="var(--success)" size={52} weight="fill" /><h3>{uiTranslate("设备已连接", $uiLanguage)}</h3><p>{uiTranslate("身份已验证，可以继续排列屏幕。", $uiLanguage)}</p></div>
            <button class="primary wide" on:click={() => step = 3}>{uiTranslate("继续排列屏幕", $uiLanguage)}<ArrowRight size={17} /></button>
          {:else}
            <div class="pairing-intro"><ShieldCheck color="var(--accent)" size={46} weight="duotone" /><h3>{uiTranslate(selectedPeer.paired ? "建立跨屏连接" : "先配对这台设备", $uiLanguage)}</h3><p>{uiTranslate(selectedPeer.paired ? "将使用已有设备配对和跨屏输入授权。" : "一次配对同时用于剪贴板、远程文件、跨屏输入、附近传输和打印。", $uiLanguage)}</p></div>
            <button class="primary wide" disabled={busy} on:click={connectOrPair}>{uiTranslate(busy ? "正在连接…" : selectedPeer.paired ? "重新连接" : "前往统一配对", $uiLanguage)}</button>
          {/if}
        {:else}
          <div class="empty-state"><DesktopTower size={38} /><strong>{uiTranslate("选择一台设备", $uiLanguage)}</strong><p>{uiTranslate("选中附近设备后会在这里显示配对步骤。", $uiLanguage)}</p></div>
        {/if}
      </div>
    </div>
    <div class="setup-footer"><button class="secondary" on:click={() => step = 1}><ArrowLeft size={17} />{uiTranslate("返回", $uiLanguage)}</button><button class="text-button" on:click={() => step = 3}>{uiTranslate("暂不添加设备", $uiLanguage)}</button></div>
  {:else if step === 3}
    <div class="setup-card display-step">
      <div class="display-step-copy">
        <span class="step-icon"><DesktopTower size={30} weight="duotone" /></span>
        <h2>{uiTranslate("已检测到", $uiLanguage)} {displays.length} {uiTranslate("块屏幕", $uiLanguage)}</h2>
        <p>{uiTranslate("下一步可拖动屏幕，使排列与真实桌面一致；贴近且重叠的边段会自动连通。", $uiLanguage)}</p>
      </div>
      <div class="detected-displays">
        {#each displays as display}
          <article><MonitorIcon color="var(--accent)" /><strong>{display.name}</strong><small>{display.pixelSize.width} × {display.pixelSize.height}</small><span><CheckCircle size={16} weight="fill" />{uiTranslate("已检测", $uiLanguage)}</span></article>
        {/each}
        {#if displays.length === 0}<div class="empty-state"><MonitorIcon /><strong>{uiTranslate("等待显示器信息", $uiLanguage)}</strong><p>{uiTranslate("设备连接后会自动读取显示器。", $uiLanguage)}</p></div>{/if}
      </div>
      <div class="setup-actions"><button class="secondary" on:click={() => step = 2}><ArrowLeft size={17} />{uiTranslate("返回", $uiLanguage)}</button><button class="primary" on:click={() => step = 4}>{uiTranslate("打开布局编辑器", $uiLanguage)}<ArrowRight size={17} /></button></div>
    </div>
  {:else}
    <div class="setup-card completed-step">
      <span><Check size={42} weight="bold" /></span>
      <h2>{uiTranslate("跨屏输入已就绪", $uiLanguage)}</h2>
      <p>{uiTranslate("布局和键盘行为可以随时调整。启用跨屏输入后，任一设备都能从高亮边段进入另一台电脑。", $uiLanguage)}</p>
      <div class="ready-checks"><span><CheckCircle size={18} weight="fill" />{uiTranslate("权限已开启", $uiLanguage)}</span><span><CheckCircle size={18} weight="fill" />{uiTranslate("设备已添加", $uiLanguage)}</span><span><CheckCircle size={18} weight="fill" />{uiTranslate("布局可编辑", $uiLanguage)}</span></div>
      <button class="primary" on:click={finish}>{uiTranslate("进入跨屏输入", $uiLanguage)}</button>
      <div class="emergency"><ShieldCheck size={18} />{uiTranslate("随时按", $uiLanguage)} <kbd>⌘⌥⇧ Esc</kbd> {uiTranslate("紧急释放并返回本机", $uiLanguage)}</div>
    </div>
  {/if}
</section>

<style>
  .setup-shell { display: flex; min-height: 0; flex: 1; flex-direction: column; gap: 22px; overflow: auto; padding: 4px 2px 28px; }
  .setup-header, .card-heading, .selected-device, .section-title, .setup-footer, .setup-actions { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .setup-header h1 { margin: 0; color: var(--text); font-size: 30px; letter-spacing: -.035em; }
  .setup-header p { margin: 7px 0 0; color: var(--text-secondary); font-size: 14px; }
  .icon-button { display: grid; width: 38px; height: 38px; place-items: center; border: 1px solid var(--border); border-radius: 10px; color: var(--text-secondary); background: var(--surface); }
  .stepper { display: grid; grid-template-columns: repeat(4, 1fr); margin: 0; padding: 4px 0 0; list-style: none; }
  .stepper li { position: relative; display: flex; align-items: center; gap: 10px; color: var(--text-muted); font-size: 13px; }
  .stepper li:not(:last-child)::after { content: ""; height: 1px; flex: 1; margin: 0 18px; background: var(--border-strong); }
  .stepper span { display: grid; width: 32px; height: 32px; place-items: center; flex: none; border: 1px solid var(--border-strong); border-radius: 50%; background: var(--surface); font-weight: 700; }
  .stepper li.active { color: var(--accent-strong); }.stepper li.active span { border-color: var(--accent); color: #fff; background: var(--accent); }.stepper li.done { color: var(--accent); }.stepper li.done span { border-color: color-mix(in srgb, var(--accent) 35%, var(--border)); color: var(--accent); background: var(--accent-soft); }
  .setup-card { border: 1px solid var(--border); border-radius: 16px; background: var(--surface-raised); box-shadow: var(--shadow-card); }
  .permission-layout { display: grid; grid-template-columns: minmax(300px, .8fr) minmax(420px, 1fr); min-height: 500px; overflow: hidden; }
  .setup-visual { display: grid; place-content: center; justify-items: center; padding: 44px; border-right: 1px solid var(--border); color: var(--accent); background: var(--surface-soft); }
  .device-pair { display: flex; align-items: center; gap: 18px; }.device-pair > span { width: 74px; border-top: 3px dotted color-mix(in srgb, var(--accent) 52%, transparent); }
  .setup-visual p { color: var(--text); font-size: 15px; font-weight: 700; }.setup-visual small { margin-left: 6px; color: var(--text-muted); font-weight: 500; }
  .setup-copy { display: grid; align-content: center; gap: 16px; padding: 38px 46px; }
  .section-title { justify-content: flex-start; align-items: flex-start; }.section-title h2, .card-heading h2, .selected-device h2 { margin: 0; color: var(--text); font-size: 20px; }.section-title p, .card-heading p, .selected-device p { margin: 5px 0 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5; }
  .permission-list { display: grid; gap: 9px; }.permission-list button, .permission-row { display: grid; grid-template-columns: 40px minmax(0, 1fr) auto 18px; align-items: center; gap: 11px; min-height: 68px; padding: 10px 14px; border: 1px solid var(--border); border-radius: 12px; color: var(--text); background: var(--surface); text-align: left; }
  .permission-list button:hover { border-color: color-mix(in srgb, var(--accent) 45%, var(--border)); }.permission-list strong, .permission-list small { display: block; }.permission-list small { margin-top: 3px; color: var(--text-muted); font-size: 11px; }.permission-list em { color: var(--text-muted); font-size: 12px; font-style: normal; }.permission-list em.ready { color: var(--success); font-weight: 700; }.row-icon, .device-icon, .step-icon { display: grid; place-items: center; color: var(--accent); background: var(--accent-soft); }.row-icon { width: 38px; height: 38px; border-radius: 10px; }
  .primary, .secondary, .text-button { display: inline-flex; min-height: 40px; align-items: center; justify-content: center; gap: 8px; border-radius: 10px; padding: 0 16px; font-weight: 650; }.primary { border: 1px solid var(--accent); color: #fff; background: var(--accent); }.primary:hover { background: var(--accent-strong); }.primary:disabled { border-color: var(--border-strong); color: var(--text-muted); background: var(--surface-sunken); }.secondary { border: 1px solid var(--border-strong); color: var(--text); background: var(--surface); }.secondary.compact { min-height: 36px; font-weight: 550; }.text-button { border: 0; color: var(--accent-strong); background: transparent; }.wide { width: 100%; }
  .setup-note { display: flex; min-height: 54px; align-items: center; gap: 10px; padding: 0 16px; border: 1px solid var(--border); border-radius: 13px; color: var(--text-secondary); background: var(--surface); font-size: 12px; }.setup-note kbd { margin-left: auto; }.setup-note small { color: var(--text-muted); }
  kbd { padding: 5px 8px; border: 1px solid var(--border-strong); border-radius: 7px; color: var(--text); background: var(--surface-soft); font-family: inherit; font-size: 12px; font-weight: 700; box-shadow: 0 1px 0 var(--border-strong); }
  .pairing-layout { display: grid; grid-template-columns: minmax(380px, .9fr) minmax(420px, 1.1fr); gap: 16px; min-height: 520px; }.nearby-card, .pairing-card { padding: 24px; }.card-heading { align-items: flex-start; }.nearby-list { display: grid; gap: 10px; margin-top: 20px; }.nearby-list button { display: grid; grid-template-columns: 48px minmax(0,1fr) auto 18px; align-items: center; gap: 12px; min-height: 78px; padding: 11px 14px; border: 1px solid var(--border); border-radius: 12px; color: var(--text); background: var(--surface); text-align: left; }.nearby-list button.selected { border-color: var(--accent); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 13%, transparent); }.device-icon { width: 46px; height: 46px; border-radius: 12px; }.nearby-list strong, .nearby-list small { display: block; }.nearby-list small { margin-top: 4px; color: var(--text-muted); font-size: 11px; }.nearby-list em { color: var(--warning); font-size: 12px; font-style: normal; }.nearby-list em.online { color: var(--success); }.nearby-list i { width: 15px; height: 15px; border: 1px solid var(--border-strong); border-radius: 50%; }.nearby-list button.selected i { border: 5px solid var(--accent); }
  .pairing-card { display: flex; flex-direction: column; gap: 16px; }.selected-device { justify-content: flex-start; }.pairing-intro, .success-panel, .empty-state { display: grid; place-content: center; justify-items: center; flex: 1; gap: 9px; color: var(--text-secondary); text-align: center; }.pairing-intro h3, .success-panel h3, .empty-state strong { margin: 0; color: var(--text); }.pairing-intro p, .success-panel p, .empty-state p { max-width: 340px; margin: 0; font-size: 12px; line-height: 1.55; }.setup-footer { justify-content: space-between; }
  .display-step { display: grid; gap: 28px; padding: 36px; }.display-step-copy { display: grid; justify-items: center; gap: 8px; text-align: center; }.display-step-copy .step-icon { width: 58px; height: 58px; border-radius: 16px; }.display-step h2 { margin: 0; font-size: 23px; }.display-step p { margin: 0; color: var(--text-secondary); font-size: 13px; }.detected-displays { display: grid; grid-template-columns: repeat(3,minmax(0,1fr)); gap: 12px; }.detected-displays article { display: grid; justify-items: center; gap: 7px; padding: 20px; border: 1px solid var(--border); border-radius: 13px; background: var(--surface-soft); }.detected-displays article small { color: var(--text-muted); }.detected-displays article span { display: flex; align-items: center; gap: 5px; color: var(--success); font-size: 11px; }.setup-actions { justify-content: flex-end; }
  .completed-step { display: grid; min-height: 480px; place-content: center; justify-items: center; gap: 15px; padding: 40px; text-align: center; }.completed-step > span { display: grid; width: 72px; height: 72px; place-items: center; border-radius: 50%; color: #fff; background: var(--success); }.completed-step h2 { margin: 0; font-size: 28px; }.completed-step > p { max-width: 560px; margin: 0; color: var(--text-secondary); line-height: 1.6; }.ready-checks { display: flex; flex-wrap: wrap; justify-content: center; gap: 16px; color: var(--success); font-size: 12px; }.ready-checks span { display: flex; align-items: center; gap: 6px; }.emergency { display: flex; align-items: center; gap: 8px; margin-top: 8px; color: var(--text-secondary); font-size: 12px; }
  :global(.spinning) { animation: spin .8s linear infinite; }@keyframes spin { to { transform: rotate(360deg); } }
  @media (max-width: 980px) { .permission-layout, .pairing-layout { grid-template-columns: 1fr; }.setup-visual { display: none; }.detected-displays { grid-template-columns: repeat(2,minmax(0,1fr)); }.stepper strong { display: none; } }
</style>
