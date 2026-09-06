<script lang="ts">
  import { t } from "./localization";
  import { translate as uiTranslate, language as uiLanguage } from "./i18n";
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { Bell, CheckCircle, Code, FileText, MagnifyingGlass, Trash, X } from "phosphor-svelte";

  import { SubscriptionScope, createInvalidationLoader } from "./subscriptions";
  import { bridge } from "./bridge";
  import { errorMessage, notificationStatusLabel, notificationTime } from "./app_helpers";
  import type { AppLanguage, NotificationView } from "./types";

  type ConfirmationRequest = {
    title: string;
    description: string;
    confirmLabel: string;
    kind: "danger" | "primary";
    onConfirm: () => Promise<boolean | void> | boolean | void;
  };

  export let language: AppLanguage;
  export let shortcutModifier: string;
  export let notify: (message: string, kind?: "success" | "error") => void;
  export let confirm: (request: ConfirmationRequest) => void;

  let searchInput: HTMLInputElement;
  let search = "";
  let notifications: NotificationView[] = [];
  let selectedId = "";
  let refreshing = false;

  $: filteredNotifications = notifications.filter((notification) => {
    const query = search.trim().toLocaleLowerCase("zh-CN");
    return (
      !query ||
      notification.title.toLocaleLowerCase("zh-CN").includes(query) ||
      notification.body.toLocaleLowerCase("zh-CN").includes(query) ||
      notification.source.toLocaleLowerCase("zh-CN").includes(query)
    );
  });
  $: selectedNotification = notifications.find((notification) => notification.id === selectedId);

  const scope = new SubscriptionScope();
  const reload = createInvalidationLoader(() => refresh(true), (error) => notify(errorMessage(error), "error"));
  onMount(() => {
    void scope.add(bridge.onNotificationsChanged(() => void reload.refresh())).then(() => reload.refresh()).catch((error) => notify(errorMessage(error), "error"));
    return () => { scope.dispose(); reload.dispose(); };
  });

  export function focusSearch() {
    searchInput?.focus();
  }

  async function refresh(preserveSelection: boolean) {
    refreshing = true;
    try {
      const next = await bridge.listNotifications(true, 200);
      if (scope.disposed) return;
      notifications = next;
      if (!preserveSelection || !next.some((item) => item.id === selectedId)) {
        selectedId = next[0]?.id ?? "";
      }
    } finally {
      refreshing = false;
    }
  }

  async function sendTestNotification() {
    try {
      notifications = await bridge.createTestNotification();
      selectedId = notifications[0]?.id ?? "";
      notify(uiTranslate("测试通知已写入 Host 队列", $uiLanguage));
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }

  async function markSelectedRead() {
    if (!selectedNotification) return;
    try {
      notifications = await bridge.markNotificationRead(selectedNotification.id);
      notify(uiTranslate("已在 Host 标记为全局已读", $uiLanguage));
    } catch (error) {
      notify(errorMessage(error), "error");
    }
  }

  function deleteSelected() {
    if (!selectedNotification) return;
    const notification = selectedNotification;
    confirm({
      title: uiTranslate("删除通知？", $uiLanguage),
      description:
        t("“{name}”将被永久删除，此操作无法撤销。", language, { name: notification.title }),
      confirmLabel: uiTranslate("确认删除", $uiLanguage),
      kind: "danger",
      onConfirm: async () => {
        notifications = await bridge.deleteNotification(notification.id);
        selectedId = notifications[0]?.id ?? "";
        notify(uiTranslate("通知已删除", $uiLanguage));
      },
    });
  }
</script>

<div class="actions-layout notifications-layout">
  <main class="content actions-content">
    <header class="page-header actions-header notification-header">
      <div class="actions-title-row">
        <div class="actions-title-copy">
          <div>
            <h1>{uiTranslate("通知", $uiLanguage)}</h1>
            <span class="action-count-pill">{filteredNotifications.length}</span>
          </div>
          <p>{uiTranslate("Agent 写入 Host 的通知会在设备上线后送达，并由 Host 聚合已读状态。", $uiLanguage)}</p>
        </div>
      </div>

      <div class="actions-toolbar">
        <label class="search-box">
          <MagnifyingGlass size={19} />
          <input bind:this={searchInput} bind:value={search} placeholder={uiTranslate("搜索通知", $uiLanguage)} />
          <kbd>{shortcutModifier} K</kbd>
        </label>
        <div class="utility-actions">
          <button class="secondary-button" on:click={sendTestNotification}>
            <Bell size={17} /> {uiTranslate("发送测试通知", $uiLanguage)}
          </button>
        </div>
      </div>
    </header>

    <div class="actions-scroll">
      <section class="action-table notification-table" aria-label={uiTranslate("通知列表", $uiLanguage)} role="table">
        <div class="notification-heading" role="row">
          <span role="columnheader">{uiTranslate("通知内容", $uiLanguage)}</span>
          <span role="columnheader">{uiTranslate("来源", $uiLanguage)}</span>
          <span role="columnheader">{uiTranslate("状态", $uiLanguage)}</span>
          <span role="columnheader">{uiTranslate("时间", $uiLanguage)}</span>
        </div>
        <div class="action-rows-scroll" role="rowgroup">
          {#if filteredNotifications.length === 0}
            <div class="empty-state">
              <Bell size={28} />
              <strong>{uiTranslate("暂无通知", $uiLanguage)}</strong>
              <span>{uiTranslate("Agent 发送的通知会保存在 Host，设备上线后再拉取。", $uiLanguage)}</span>
            </div>
          {:else}
            {#each filteredNotifications as notification (notification.id)}
              <button
                class="notification-row"
                class:selected={selectedId === notification.id}
                role="row"
                on:click={() => (selectedId = notification.id)}
              >
                <div class="action-name-cell notification-title-cell" role="cell">
                  <span class="notification-icon" class:required={notification.kind === "action_required"} class:completed={notification.kind === "task_completed"}>
                    {#if notification.kind === "action_required"}
                      <Code size={23} weight="bold" />
                    {:else if notification.kind === "task_completed"}
                      <CheckCircle size={23} weight="fill" />
                    {:else}
                      <FileText size={23} />
                    {/if}
                  </span>
                  <span>
                    <strong>{uiTranslate(notification.title, $uiLanguage)}</strong>
                    <small>{notification.body}</small>
                  </span>
                </div>
                <span class="muted-cell" role="cell">{notification.source}</span>
                <span class="notification-state" role="cell" class:read={notification.deliveryState === "read"} class:waiting={notification.deliveryState === "waitingForDevice"}>
                  <span></span>{uiTranslate(notificationStatusLabel(notification), $uiLanguage)}
                </span>
                <span class="muted-cell notification-time" role="cell">{notificationTime(notification.createdAtMs)}</span>
              </button>
            {/each}
          {/if}
        </div>
      </section>
    </div>
  </main>

  <aside class="inspector notification-inspector">
    {#if selectedNotification}
      <div class="inspector-heading">
        <strong>{uiTranslate(selectedNotification.title, $uiLanguage)}</strong>
        <button aria-label={uiTranslate("取消选择", $uiLanguage)} on:click={() => (selectedId = "")}><X size={18} /></button>
      </div>
      <div class="inspector-scroll">
        <div class="inspector-hero notification-hero">
          <span class="large-notification-icon" class:required={selectedNotification.kind === "action_required"} class:completed={selectedNotification.kind === "task_completed"}>
            {#if selectedNotification.kind === "action_required"}
              <Code size={30} weight="bold" />
            {:else if selectedNotification.kind === "task_completed"}
              <CheckCircle size={30} weight="fill" />
            {:else}
              <FileText size={30} />
            {/if}
          </span>
          {#if selectedNotification.deliveryState !== "read"}
            <button class="outline-run-button" on:click={markSelectedRead}>
              <CheckCircle size={17} /> {uiTranslate("标记已读", $uiLanguage)}
            </button>
          {:else}
            <span class="read-confirmation"><CheckCircle size={17} weight="fill" /> {uiTranslate("已在 Host 确认阅读", $uiLanguage)}</span>
          {/if}
        </div>

        <div class="inspector-section">
          <h2>{uiTranslate("基本信息", $uiLanguage)}</h2>
          <dl class="detail-list">
            <div><dt>{uiTranslate("来源", $uiLanguage)}</dt><dd>{selectedNotification.source}</dd></div>
            <div><dt>{uiTranslate("状态", $uiLanguage)}</dt><dd>{uiTranslate(notificationStatusLabel(selectedNotification), $uiLanguage)}</dd></div>
            <div><dt>{uiTranslate("时间", $uiLanguage)}</dt><dd>{notificationTime(selectedNotification.createdAtMs, true)}</dd></div>
            <div><dt>{uiTranslate("目标", $uiLanguage)}</dt><dd>{uiTranslate("所有已配对设备", $uiLanguage)}</dd></div>
            {#if selectedNotification.readByDeviceName}
              <div><dt>{uiTranslate("阅读设备", $uiLanguage)}</dt><dd>{selectedNotification.readByDeviceName}</dd></div>
            {/if}
          </dl>
        </div>

        <div class="inspector-section description-section">
          <h2>{uiTranslate("通知内容", $uiLanguage)}</h2>
          <p>{uiTranslate(selectedNotification.body || "无附加内容", $uiLanguage)}</p>
        </div>

        <div class="inspector-section notification-note">
          <h2>{uiTranslate("说明", $uiLanguage)}</h2>
          <p>{uiTranslate("任一已配对设备阅读后，Host 会记录全局已读，其他设备不再重新拉取。", $uiLanguage)}</p>
        </div>
      </div>
      <div class="inspector-footer notification-footer">
        {#if selectedNotification.deliveryState !== "read"}
          <button class="secondary-button" on:click={markSelectedRead}>
            <CheckCircle size={17} /> {uiTranslate("标记已读", $uiLanguage)}
          </button>
        {/if}
        <button class="danger-button" on:click={deleteSelected}>
          <Trash size={17} /> {uiTranslate("删除", $uiLanguage)}
        </button>
      </div>
    {:else}
      <div class="inspector-empty">
        <Bell size={28} />
        <strong>{uiTranslate("选择一条通知", $uiLanguage)}</strong>
        <span>{uiTranslate("查看来源、内容和 Host 已读状态。", $uiLanguage)}</span>
      </div>
    {/if}
  </aside>
</div>
