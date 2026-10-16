// Browser-only interaction preview. Production always uses the native repository.
import type {
  CollectionRequest,
  CollectionResponse,
  ClipboardCollection,
} from "../../ipc/generated";
import { item } from "./model";
import { clipboardBridge } from "../../clipboard/bridge";
import { previewTotp } from "./previewTotp";
const key = "arcrelay-collection-preview-v2";
const initial: ClipboardCollection[] = [
  ["工作账号", ["用户名", "密码", "TOTP"]],
  ["家庭地址", ["收件人", "电话", "详细地址"]],
  ["开票资料", ["公司名称", "税号", "开户信息"]],
  ["常用回复", ["问候语", "工作时间", "签名"]],
].map(([name, labels]) => ({
  id: crypto.randomUUID(),
  name: name as string,
  items: (labels as string[]).map((label) => ({
    ...item(label),
    value: label === "用户名" ? "hello@example.com" : label === "密码" ? "example-password" : "",
    // Public RFC 6238 test key, exclusively for the browser demo.
    ...(label === "TOTP"
      ? {
          totp: {
            secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
            algorithm: "SHA1",
            digits: 6,
            period: 30,
          },
        }
      : {}),
  })),
  revision: 1,
  updatedByDeviceId: "preview",
  favorite: false,
  favoriteRevision: 1,
  favoriteUpdatedByDeviceId: "preview",
  deleted: false,
}));
const pinsKey = "arcrelay-collection-app-pins-v1";
let appPins: Record<string, string[]> = JSON.parse(localStorage.getItem(pinsKey) ?? "{}");
let records: ClipboardCollection[] = JSON.parse(localStorage.getItem(key) ?? "null") ?? initial;
export async function previewRequest(request: CollectionRequest): Promise<CollectionResponse> {
  const response: CollectionResponse = {
    pinnedIds: [],
    peers: [],
    records: [],
    saved: null,
    value: null,
    items: [],
    syncEnabled: true,
    syncEdits: true,
    syncFavorites: true,
  };
  if (request.type === "appPins" || request.type === "setAppPin") {
    if (!request.app_id.trim() || request.app_id.length > 1024) throw Error("无效的目标应用");
    if (request.type === "setAppPin") {
      const target = await clipboardBridge.targetApplication();
      if (target?.id !== request.app_id) throw Error("前台应用已切换，请重新打开菜单后置顶");
      if (request.pinned && !records.some((r) => r.id === request.id && !r.deleted))
        throw Error("集合不存在");
      const ids = appPins[request.app_id] ?? [];
      appPins = {
        ...appPins,
        [request.app_id]: request.pinned
          ? [...new Set([...ids, request.id])]
          : ids.filter((id) => id !== request.id),
      };
      localStorage.setItem(pinsKey, JSON.stringify(appPins));
    }
    response.pinnedIds = (appPins[request.app_id] ?? []).filter((id) =>
      records.some((r) => r.id === id && !r.deleted),
    );
  }
  if (request.type === "fromHistory") {
    if (
      !request.ids.length ||
      request.ids.length > 64 ||
      new Set(request.ids).size !== request.ids.length
    )
      throw Error("请选择 1 至 64 条文本记录");
    const history = await clipboardBridge.history({
      search: "",
      kind: null,
      favoriteOnly: false,
      labelIds: [],
      cursor: null,
      limit: 10000,
    });
    for (const [index, id] of request.ids.entries()) {
      const entry = history.entries.find((r) => r.id === id);
      if (!entry?.available || !["text", "html"].includes(entry.kind))
        throw Error("所选记录不包含可用文本");
      const value = await clipboardBridge.textContent(entry);
      if (new TextEncoder().encode(value).length > 64 * 1024)
        throw Error("单条文本不能超过 64 KiB");
      response.items.push({ ...item("内容 " + (index + 1)), value });
    }
  }
  if (request.type === "list")
    response.records = records
      .filter((r) => r.id > request.after)
      .sort((a, b) => a.id.localeCompare(b.id))
      .slice(0, 100);
  if (request.type === "save") {
    const d = request.draft,
      old = records.find((r) => r.id === d.id);
    if (
      d.id &&
      (!old ||
        old.deleted ||
        old.revision !== d.base?.revision ||
        old.updatedByDeviceId !== d.base?.deviceId)
    )
      throw Error("COLLECTION_STALE");
    if (d.items.length > 64 || !d.name.trim() || d.items.some((i) => !i.label.trim()))
      throw Error("请填写名称和字段名称");
    for (const entry of d.items) {
      if (entry.totp) await previewTotp(entry.totp, Date.now());
    }
    const value: ClipboardCollection = {
      ...(old ?? {
        id: crypto.randomUUID(),
        favorite: false,
        favoriteRevision: 1,
        favoriteUpdatedByDeviceId: "preview",
      }),
      name: d.name,
      items: structuredClone(d.items),
      revision: (old?.revision ?? 0) + 1,
      updatedByDeviceId: "preview",
      deleted: false,
    };
    if (
      value.items.some((i) => new TextEncoder().encode(i.value).length > 64 * 1024) ||
      new TextEncoder().encode(JSON.stringify(value)).length > 256 * 1024
    )
      throw Error("集合内容超过大小限制");
    records = [...records.filter((r) => r.id !== value.id), value];
    response.saved = value;
  }
  if (request.type === "remove" || request.type === "favorite") {
    const r = records.find((r) => r.id === request.id);
    if (!r) throw Error("集合不存在");
    if (request.type === "remove") {
      if (
        r.deleted ||
        request.base.revision !== r.revision ||
        request.base.deviceId !== r.updatedByDeviceId
      )
        throw Error("COLLECTION_STALE");
      appPins = Object.fromEntries(
        Object.entries(appPins).map(([key, ids]) => [key, ids.filter((id) => id !== r.id)]),
      );
      localStorage.setItem(pinsKey, JSON.stringify(appPins));
      r.deleted = true;
      r.items = [];
      r.revision++;
    } else {
      r.favorite = request.favorite;
      r.favoriteRevision++;
    }
  }
  if (request.type === "value" || request.type === "use") {
    const i = records.find((r) => r.id === request.id)?.items.find((i) => i.id === request.item);
    if (!i) throw Error("内容不存在");
    response.value = i.totp
      ? await previewTotp(i.totp, Date.now())
      : { code: i.value, expiresAtMs: 0 };
    if (request.type === "use") {
      if (request.paste) throw Error("浏览器预览请使用复制；原生粘贴需在桌面应用验证");
      if (i.totp && response.value.expiresAtMs - Date.now() < 5000)
        throw Error("TOTP 即将更新，请稍后再试");
      await navigator.clipboard.writeText(response.value.code);
    }
  }
  if (request.type === "retry") throw Error("浏览器预览未连接设备");
  if (request.type === "openSettings") throw Error("剪贴板设置请在桌面应用中打开");
  localStorage.setItem(key, JSON.stringify(records));
  return structuredClone(response);
}
