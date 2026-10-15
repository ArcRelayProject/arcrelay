<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { Bluetooth, MagnifyingGlass, CheckCircle, Info, Play, Stop } from "phosphor-svelte";
  import AppSelect from "../components/AppSelect.svelte";
  import Modal from "./Modal.svelte";
  import BluetoothSignal from "./BluetoothSignal.svelte";
  import { bridge } from "../bridge";
  import { errorMessage } from "../app_helpers";
  import { translate as tr, language } from "../i18n";
  import type { BluetoothTrigger, BluetoothSnapshot, BluetoothDetection } from "../ipc/generated";
  export let config: BluetoothTrigger;
  export let automationId = "";
  let snapshot: BluetoothSnapshot = { status: "idle", error: null, devices: [] };
  let search = "",
    error = "",
    calibrationError = "";
  let active = true,
    polling = false,
    testing = false,
    modal = false,
    sampling = false;
  let sessionId = "",
    near: number | null = null,
    far: number | null = null;
  let countdown = 0,
    phase: "near" | "away" = "near",
    generation = 0;
  let detection: BluetoothDetection | null = null;
  let pollTimer: ReturnType<typeof setInterval>;
  let countdownTimer: ReturnType<typeof setInterval>;
  let cancelCountdown: (() => void) | undefined;
  let previousConfig = "";
  $: selected = snapshot.devices.find((d) => d.id === config.deviceId);
  $: options = snapshot.devices.filter((d) =>
    `${d.name} ${d.id}`.toLowerCase().includes(search.toLowerCase()),
  );
  $: boundary = config.calibration ? config.calibration.farRssi + 3 : null;
  $: if (JSON.stringify(config) !== previousConfig) {
    previousConfig = JSON.stringify(config);
    testing = false;
    detection = null;
    sessionId = "";
  }
  const statuses: Record<string, string> = {
    waitingNear: "请将设备带到座位，稳定停留 3 秒",
    near: "设备在附近，等待离开",
    leaving: "持续远离计时中",
    far: "已满足触发条件，本次没有执行动作",
    triggered: "本次离开已检测到，返回附近后可再次检测",
    signalLost: "已满足信号丢失条件，本次没有执行动作",
    noSignal: "没有新信号，远离计时已取消",
    unavailable: "蓝牙不可用，检测已暂停",
  };
  async function poll() {
    if (!active || polling) return;
    polling = true;
    try {
      const value = await bridge.bluetoothSnapshot(
        (testing || modal) && automationId ? automationId : null,
      );
      if (!active) return;
      snapshot = value;
      error = "";
      if (testing) {
        const id = sessionId;
        const result = await bridge.previewBluetoothTrigger(id, structuredClone(config));
        if (active && testing && id === sessionId) detection = result;
      }
    } catch (e) {
      if (active) error = errorMessage(e);
    } finally {
      polling = false;
    }
  }
  function selectDevice(id: string) {
    const d = snapshot.devices.find((d) => d.id === id);
    if (!d?.readable) return;
    config = { ...config, deviceId: d.id, deviceName: d.name, calibration: null };
  }
  function openCalibration() {
    generation++;
    modal = true;
    near = null;
    far = null;
    sampling = false;
    calibrationError = "";
    phase = "near";
    void poll();
  }
  function closeCalibration() {
    generation++;
    modal = false;
    sampling = false;
    clearInterval(countdownTimer);
    cancelCountdown?.();
    cancelCountdown = undefined;
  }
  async function sample(away: boolean) {
    const attempt = ++generation,
      id = config.deviceId;
    sampling = true;
    calibrationError = "";
    phase = away ? "away" : "near";
    try {
      // Lease before leaving; existing copies of this rule must not lock during calibration.
      await bridge.bluetoothSnapshot(automationId || null);
      if (away) {
        countdown = 10;
        await new Promise<void>((resolve) => {
          cancelCountdown = resolve;
          countdownTimer = setInterval(() => {
            if (!active || generation !== attempt) {
              clearInterval(countdownTimer);
              resolve();
              return;
            }
            countdown--;
            if (countdown <= 0) {
              clearInterval(countdownTimer);
              resolve();
            }
          }, 1000);
        });
      }
      if (!active || generation !== attempt) return;
      countdown = 10;
      countdownTimer = setInterval(() => {
        countdown = Math.max(0, countdown - 1);
      }, 1000);
      const value = await bridge.calibrateBluetooth(id);
      if (!active || generation !== attempt || config.deviceId !== id) return;
      if (away) {
        if (near === null || near - value.rssi < 10)
          throw new Error(tr("两个位置的信号差异不足，请走远一些重新采样", $language));
        far = value.rssi;
      } else {
        near = value.rssi;
        far = null;
      }
    } catch (e) {
      if (active && generation === attempt) calibrationError = errorMessage(e);
    } finally {
      if (generation === attempt) {
        clearInterval(countdownTimer);
        cancelCountdown = undefined;
        sampling = false;
        countdown = 0;
      }
    }
  }
  function saveCalibration() {
    if (near === null || far === null) return;
    config = { ...config, calibration: { nearRssi: near, farRssi: far } };
    closeCalibration();
  }
  async function toggleTest() {
    if (testing) {
      testing = false;
      detection = null;
      return;
    }
    sessionId = crypto.randomUUID();
    testing = true;
    detection = null;
    await poll();
  }
  onMount(() => {
    void poll();
    pollTimer = setInterval(() => void poll(), 1000);
  });
  onDestroy(() => {
    active = false;
    generation++;
    clearInterval(pollTimer);
    clearInterval(countdownTimer);
    cancelCountdown?.();
  });
</script>

<label class="au-field"
  >{tr("何时触发", $language)}<AppSelect
    value="far"
    options={[{ value: "far", label: tr("设备持续远离", $language) }]}
    aria-label={tr("何时触发", $language)}
  /></label
>
<div class="au-label-row">
  <h3>{tr("选择蓝牙设备", $language)}</h3>
  <span class="au-muted">{tr("选择一台随身设备", $language)}</span>
</div>
<label class="au-search"
  ><MagnifyingGlass size={18} /><input
    bind:value={search}
    placeholder={tr("搜索蓝牙设备", $language)}
    aria-label={tr("搜索蓝牙设备", $language)}
  /></label
>
<div class="au-app-list">
  {#each options as device (device.id)}
    <button
      class="au-app-option"
      class:chosen={device.id === config.deviceId}
      aria-pressed={device.id === config.deviceId}
      disabled={!device.readable}
      on:click={() => selectDevice(device.id)}
    >
      <Bluetooth size={24} /><span
        ><strong>{device.name}</strong><small
          >{device.id} · {device.readable
            ? tr("信号可读取", $language)
            : tr("等待新信号", $language)}{device.rssi !== null
            ? ` · ${device.rssi} dBm`
            : ""}</small
        ></span
      >{#if device.id === config.deviceId}<CheckCircle size={21} weight="fill" />{/if}
    </button>
  {:else}<p class="au-muted">
      {tr(
        snapshot.status === "unavailable"
          ? "蓝牙不可用，检测已暂停"
          : search
            ? "没有匹配的蓝牙设备"
            : snapshot.status === "scanning"
              ? "暂未发现设备。请将设备靠近电脑并保持 BLE 广播。"
              : "正在准备蓝牙扫描…",
        $language,
      )}
    </p>{/each}
</div>
{#if snapshot.error || error}<p class="au-error" role="alert">
    {tr(snapshot.error || error, $language)}
  </p>{/if}
<p class="au-note">
  <Info size={16} />{tr(
    "仅支持能持续读取信号的 BLE 设备。手机开启蓝牙不保证可被检测；锁屏或后台时请先测试。",
    $language,
  )}
</p>
{#if config.deviceId}
  <div class="au-label-row">
    <strong>{config.deviceName}</strong><span
      >{tr("当前信号", $language)}：{selected?.rssi ?? "—"}
      {selected?.rssi != null ? "dBm" : tr("暂无新信号", $language)}</span
    >
  </div>
  <BluetoothSignal samples={selected?.history ?? []} {boundary} />
  <div class="au-label-row">
    <span
      >{tr("离开边界", $language)}：{boundary === null
        ? tr("尚未校准", $language)
        : `${boundary} dBm`}</span
    ><button class="au-button" disabled={!selected?.readable || testing} on:click={openCalibration}
      >{tr(config.calibration ? "重新校准" : "开始校准", $language)}</button
    >
  </div>
{/if}
<div class="au-two-fields bt-fields">
  <label class="au-field"
    >{tr("持续时间", $language)}<AppSelect
      bind:value={config.durationSeconds}
      options={[5, 10, 15, 30, 60, 120, 300].map((value) => ({
        value,
        label: `${value} ${tr("秒", $language)}`,
      }))}
      aria-label={tr("持续时间", $language)}
    /></label
  >
  <label class="au-field"
    >{tr("设备信号消失时", $language)}<AppSelect
      value={config.signalLossSeconds ?? 0}
      onValueChange={(value) => {
        config = { ...config, signalLossSeconds: Number(value) || null };
      }}
      options={[
        { value: 0, label: tr("不触发，仅显示信号丢失", $language) },
        ...[15, 30, 60, 120, 300].map((value) => ({
          value,
          label: `${value} ${tr("秒后触发", $language)}`,
        })),
      ]}
      aria-label={tr("设备信号消失时", $language)}
    /></label
  >
</div>
<p class="au-note">
  {tr("信号恢复会取消远离计时。蓝牙关闭或权限失效时暂停检测，不触发动作。", $language)}
</p>
{#if config.signalLossSeconds !== null}<p class="au-note">
    {tr("手机省电、停止广播也可能造成信号丢失，请先检测效果。", $language)}
  </p>{/if}
<div class="au-inline">
  <button class="au-button" disabled={!config.calibration || !config.deviceId} on:click={toggleTest}
    >{#if testing}<Stop size={17} />{:else}<Play size={17} />{/if}{tr(
      testing ? "停止检测" : "检测触发条件",
      $language,
    )}</button
  ><span class="au-muted">{tr("只检测，不执行动作", $language)}</span>
</div>
{#if testing}<p role="status" class="au-note">
    {tr(statuses[detection?.state ?? "waitingNear"] ?? "等待信号", $language)}{detection?.state ===
    "leaving"
      ? ` · ${detection.elapsedSeconds} / ${config.durationSeconds} ${tr("秒", $language)}`
      : ""}
  </p>{/if}
<p class="au-note">
  {tr("校准和检测期间暂停本条自动化，结束后 15 秒内恢复。其他已启用规则仍会运行。", $language)}
</p>
{#if modal}
  <Modal title={tr("校准离开边界", $language)} close={closeCalibration}>
    <p class="au-note">
      {tr("按平时方式携带设备，例如放入口袋。校准信号边界，不估算精确米数。", $language)}
    </p>
    <h3>{tr("1. 在座位记录信号", $language)}</h3>
    <p>
      {near === null
        ? tr("将设备带到电脑旁，保持稳定 10 秒。", $language)
        : `${tr("座位信号", $language)}：${near} dBm`}
    </p>
    <button class="au-button" disabled={sampling} on:click={() => sample(false)}
      >{tr(near === null ? "记录座位信号" : "重新记录座位信号", $language)}</button
    >
    <h3>{tr("2. 记录离开位置", $language)}</h3>
    <p>{tr("点击后有 10 秒走开，随后自动采样 10 秒。", $language)}</p>
    <button class="au-button" disabled={sampling || near === null} on:click={() => sample(true)}
      >{tr("开始离开位置采样", $language)}</button
    >
    {#if sampling}<p role="status">
        {tr(phase === "near" ? "正在记录座位信号" : "请带设备走开并保持稳定", $language)} · {countdown}
        {tr("秒", $language)}
      </p>{/if}
    {#if far !== null}<p>{tr("离开位置信号", $language)}：{far} dBm</p>{/if}
    {#if calibrationError}<p class="au-error" role="alert">
        {tr(calibrationError, $language)}
      </p>{/if}
    <footer>
      <button class="au-button" on:click={closeCalibration}>{tr("取消", $language)}</button><button
        class="au-button primary"
        disabled={sampling || near === null || far === null}
        on:click={saveCalibration}>{tr("保存校准", $language)}</button
      >
    </footer>
  </Modal>
{/if}

<style>
  .bt-fields {
    margin-top: 20px;
  }
  h3 {
    margin-top: 20px;
  }
</style>
