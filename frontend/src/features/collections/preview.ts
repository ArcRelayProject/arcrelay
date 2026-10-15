// Browser-only interaction preview. Production always uses the native repository.
import type {
  CollectionRequest,
  CollectionResponse,
  ClipboardCollection,
} from "../../ipc/generated";
import { item } from "./model";
const key = "arcrelay-collection-preview-v1";
const initial: ClipboardCollection[] = [
  ["工作账号", ["用户名", "密码", "验证码"]],
  ["家庭地址", ["收件人", "电话", "详细地址"]],
  ["开票资料", ["公司名称", "税号", "开户信息"]],
  ["常用回复", ["问候语", "工作时间", "签名"]],
].map(([name, labels]) => ({
  id: crypto.randomUUID(),
  name: name as string,
  items: (labels as string[]).map((label) => ({
    ...item(label),
    value:
      label === "用户名"
        ? "hello@example.com"
        : label === "密码"
          ? "example-password"
          : label === "验证码"
            ? "482196"
            : "",
  })),
  revision: 1,
  updatedByDeviceId: "preview",
  favorite: false,
  favoriteRevision: 1,
  favoriteUpdatedByDeviceId: "preview",
  deleted: false,
}));
let records: ClipboardCollection[] = JSON.parse(localStorage.getItem(key) ?? "null") ?? initial;
export async function previewRequest(request: CollectionRequest): Promise<CollectionResponse> {
  const response: CollectionResponse = {
    peers: [],
    records: [],
    saved: null,
    value: null,
    items: [],
    syncEnabled: true,
    syncEdits: true,
    syncFavorites: true,
  };
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
    if (!d.name.trim() || d.items.some((i) => !i.label.trim())) throw Error("请填写名称和字段名称");
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
    records = [...records.filter((r) => r.id !== value.id), value];
    response.saved = value;
  }
  if (request.type === "remove" || request.type === "favorite") {
    const r = records.find((r) => r.id === request.id);
    if (!r) throw Error("集合不存在");
    if (request.type === "remove") {
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
    response.value = { code: i.value, expiresAtMs: 0 };
    if (request.type === "use") {
      if (request.paste) throw Error("浏览器预览请使用复制；原生粘贴需在桌面应用验证");
      await navigator.clipboard.writeText(i.value);
    }
  }
  if (request.type === "retry") throw Error("浏览器预览未连接设备");
  if (request.type === "openSettings") throw Error("剪贴板设置请在桌面应用中打开");
  localStorage.setItem(key, JSON.stringify(records));
  return structuredClone(response);
}
