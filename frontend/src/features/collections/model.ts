import type { CollectionDraft, CollectionItem, ClipboardCollection } from "../../ipc/generated";
export const templates = [
  { name: "空白集合", labels: [] },
  { name: "账号", labels: ["用户名", "密码", "TOTP"] },
  { name: "收件地址", labels: ["收件人", "电话", "详细地址"] },
  { name: "银行卡", labels: ["卡号", "有效期", "安全码"] },
  { name: "开票资料", labels: ["公司名称", "税号", "地址", "开户行", "账号"] },
];
export const item = (label = "内容"): CollectionItem => ({
  id: crypto.randomUUID(),
  label,
  value: "",
  masked: ["密码", "安全码"].includes(label),
  totp: label === "TOTP" ? { secret: "", algorithm: "SHA1", digits: 6, period: 30 } : null,
});
export function fromTemplate(index: number): CollectionDraft {
  const template = templates[index];
  return {
    id: null,
    base: null,
    name: index ? template.name : "",
    items: template.labels.map(item),
  };
}
export function toDraft(record: ClipboardCollection): CollectionDraft {
  return {
    id: record.id,
    base: { revision: record.revision, deviceId: record.updatedByDeviceId },
    name: record.name,
    items: structuredClone(record.items),
  };
}
export function matches(record: ClipboardCollection, search: string) {
  const query = search.trim().toLocaleLowerCase();
  return (
    !query ||
    [
      record.name,
      ...record.items.flatMap((i) => [i.label, !i.masked && !i.totp ? i.value : ""]),
    ].some((value) => value.toLocaleLowerCase().includes(query))
  );
}
