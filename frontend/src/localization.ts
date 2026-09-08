import type { LanguagePreference } from "./types";
import en from "./locales/en.json" with { type: "json" };
import ja from "./locales/ja.json" with { type: "json" };
import ko from "./locales/ko.json" with { type: "json" };
import de from "./locales/de.json" with { type: "json" };
import fr from "./locales/fr.json" with { type: "json" };
import es from "./locales/es.json" with { type: "json" };
import pt from "./locales/pt.json" with { type: "json" };

export type ResolvedLanguage = Exclude<LanguagePreference, "system">;
export const languageOptions = [
  { value: "zhCn", label: "简体中文", locale: "zh-CN" },
  { value: "enUs", label: "English", locale: "en-US" },
  { value: "jaJp", label: "日本語", locale: "ja-JP" },
  { value: "koKr", label: "한국어", locale: "ko-KR" },
  { value: "deDe", label: "Deutsch", locale: "de-DE" },
  { value: "frFr", label: "Français", locale: "fr-FR" },
  { value: "esEs", label: "Español", locale: "es-ES" },
  { value: "ptBr", label: "Português (Brasil)", locale: "pt-BR" },
] as const satisfies readonly { value: ResolvedLanguage; label: string; locale: string }[];
export type Locale = typeof languageOptions[number]["locale"];
export const catalogs: Record<ResolvedLanguage, Record<string, string>> = {
  zhCn: {}, enUs: en, jaJp: ja, koKr: ko, deDe: de, frFr: fr, esEs: es, ptBr: pt,
};

export function systemLanguages(): readonly string[] {
  return typeof navigator === "undefined" ? [] : navigator.languages?.length ? navigator.languages : [navigator.language];
}

export function resolveLanguage(preference: LanguagePreference, preferred: readonly string[] = systemLanguages()): ResolvedLanguage {
  if (preference !== "system" && languageOptions.some(option => option.value === preference)) return preference;
  for (const tag of preferred) {
    const code = tag.replaceAll("_", "-").toLowerCase().split("-")[0];
    const option = languageOptions.find(option => option.locale.toLowerCase().split("-")[0] === code);
    if (option) return option.value;
  }
  return "enUs";
}

export function localeFor(preference: LanguagePreference): Locale {
  return languageOptions.find(option => option.value === resolveLanguage(preference))!.locale;
}

const englishEntries: Record<string, string> = en;
const englishSources = new Map(Object.entries(englishEntries).map(([key, value]) => [value, key]));

export function translate(value: string | number | null | undefined, current: LanguagePreference): string {
  const source = String(value ?? "");
  if (!source) return source;
  const resolved = resolveLanguage(current);
  const key = englishSources.get(source) ?? source;
  if (resolved === "zhCn") return key;
  const catalog = catalogs[resolved];
  return catalog[key] ?? englishEntries[key] ?? translatePattern(source, resolved) ?? translateEnglishPattern(source) ?? source;
}

export function t(source: string, current: LanguagePreference, values: Record<string, string | number> = {}) {
  // Replace in a single pass: inserted names must never be interpreted as placeholders.
  return translate(source, current).replace(/\{([^{}]+)\}/g, (match, key) =>
    Object.hasOwn(values, key) ? String(values[key]) : match,
  );
}

function translateEnglishPattern(source: string): string | null {
  const patterns: Array<[RegExp, (...parts: string[]) => string]> = [
    [/^全局快捷键已被动作“(.+)”使用$/, (name) => `This shortcut is already used by action “${name}”.`],
    [/^全局快捷键已被(.+)使用$/, (name) => `This shortcut is already used by ${translate(name, "enUs")}.`],
    [/^注册全局快捷键失败，可能已被系统或其他应用占用: (.+)$/, (error) => `Could not register the shortcut. The system or another app may already be using it: ${error}`],
    [/^打开设备与连接，当前 (\d+) 台设备已连接$/, (count) => `Open devices and connections; ${count} connected`],
    [/^版本 (.+)$/, (version) => `Version ${version}`],
    [/^发现新版本 (.+)$/, (version) => `ArcRelay ${version} is available`],
    [/^当前版本 (.+)，更新通道为 main。$/, (version) => `Current version ${version}; update channel: main.`],
    [/^可更新到 (.+)。更新包会在安装前验证签名。$/, (version) => `Version ${version} is available. The update signature is verified before installation.`],
    [/^安装 ArcRelay (.+)？$/, (version) => `Install ArcRelay ${version}?`],
    [/^(\d+) 台设备已连接$/, (count) => `${count} device${count === "1" ? "" : "s"} connected`],
    [/^(\d+) 台设备$/, (count) => `${count} device${count === "1" ? "" : "s"}`],
    [/^版本 (\d+) · (\d+) 个节点$/, (version, nodes) => `Version ${version} · ${nodes} node${nodes === "1" ? "" : "s"}`],
    [/^最多 (\d+) 次$/, (count) => `Up to ${count} times`],
    [/^(\d+) 个节点 · (.+) · v(.+)$/, (nodes, triggers, version) => `${nodes} node${nodes === "1" ? "" : "s"} · ${triggers} · v${version}`],
    [/^(\d+) 条$/, (count) => `${count} item${count === "1" ? "" : "s"}`],
    [/^(\d+) 台可用$/, (count) => `${count} available`],
    [/^正在请求发送 (\d+) 个文件$/, (count) => `Wants to send ${count} file${count === "1" ? "" : "s"}`],
    [/^传输进度 (\d+)%$/, (value) => `Transfer progress ${value}%`],
    [/^向 (.+) 发送文件$/, (name) => `Send files to ${name}`],
    [/^预览 (.+)$/, (name) => `Preview ${name}`],
    [/^共 (\d+) 个应用$/, (count) => `${count} app${count === "1" ? "" : "s"}`],
    [/^版本 (.+)$/, (version) => `Version ${version}`],
    [/^已发布版本 (.+)$/, (version) => `Published version ${version}`],
    [/^工作流已启动 · (.+)$/, (id) => `Workflow started · ${id}`],
    [/^已导入 (\d+) 个动作$/, (count) => `Imported ${count} action${count === "1" ? "" : "s"}`],
    [/^已添加“(.+)”$/, (name) => `Added “${name}”`],
    [/^已移除“(.+)”$/, (name) => `Removed “${name}”`],
    [/^打开 (.+)$/, (name) => `Open ${name}`],
    [/^移除 (.+)$/, (name) => `Remove ${name}`],
    [/^运行 (.+)$/, (name) => `Run ${name}`],
    [/^没有找到“(.+)”$/, (query) => `No results for “${query}”`],
    [/^已连接：(.+) · 在线$/, (name) => `Connected: ${name} · Online`],
    [/^ArcRelay (已就绪|已停止)$/, (state) => `ArcRelay ${translate(state, "enUs")}`],
    [/^端口 (\d+)$/, (port) => `Port ${port}`],
    [/^输入诊断 · (.+)$/, (name) => `Input Diagnostics · ${name}`],
    [/^当前有 (\d+) 个共享文件夹$/, (count) => `${count} shared folder${count === "1" ? "" : "s"}`],
    [/^(.+) 当前为(可读写|只读)，点击切换$/, (name, access) => `${name} is ${translate(access, "enUs")}; click to change`],
    [/^停止共享 (.+)$/, (name) => `Stop sharing ${name}`],
  ];
  for (const [pattern, formatter] of patterns) {
    const match = source.match(pattern);
    if (match) return formatter(...match.slice(1));
  }
  return null;
}

// Match legacy computed labels through the same catalogs. New messages should use t().
const messageTemplates = [
  "“{name}”会改变当前系统状态，请确认是否继续执行。",
  "移除“{name}”后，它的窗口将不再受投屏隐私保护。",
  "可更新到 {version}。更新包会在安装前验证签名。",
  "打开设备与连接，当前 {count} 台设备已连接",
  "当前版本 {version}，更新通道为 main。",
  "当前版本 {version}，更新通道为 {channel}。",
  "“{name}”将被永久删除，此操作无法撤销。",
  "已接受；以后将自动接收 {name} 的文件",
  "安装 ArcRelay {version}？",
  "正在请求发送 {count} 个文件",
  "当前有 {count} 个共享文件夹",
  "版本 {version} · {count} 个节点",
  "已接受来自 {name} 的文件",
  "已拒绝来自 {name} 的传输",
  "已向 {name} 发起连接请求",
  "工作流已启动 · {id}",
  "已连接：{name} · 在线",
  "为“{name}”输入新名称。",
  "已导入 {count} 个动作",
  "已选择 {count} 个字符",
  "已创建文件夹“{name}”",
  "删除 {count} 个项目？",
  "已接收来自 {name} 的{subject}",
  "{count} 台设备已连接",
  "向 {name} 发送文件",
  "输入诊断 · {name}",
  "已开始发送到 {name}",
  "，已选第 {count} 项",
  "按 {shortcut} 快速插入",
  "来自其他设备：{name}",
  "发现新版本 {version}",
  "传输进度 {value}%",
  "共 {count} 个应用",
  "已发布版本 {version}",
  "没有找到“{query}”",
  "已选择 {count} 项",
  "连续粘贴 {current}/{total}",
  "插入所选 {count} {unit}",
  "第 {count} 个选择",
  "{name} 请求发送 {subject}",
  "最多 {count} 次",
  "已添加“{name}”",
  "已移除“{name}”",
  "停止共享 {name}",
  "双击插入此{unit}",
  "{name}，双击插入",
  "删除标签 {name}",
  "删除“{name}”？",
  "{count} 台设备",
  "{count} 台可用",
  "{count} 个文件",
  "共 {count} 条",
  "“{name}”标签下暂无内容",
  "{name} · {count} 条",
  "已选 {count} {unit}",
  "{count} 个字符",
  "已上传 {name}",
  "已下载 {name}",
  "版本 {version}",
  "预览 {name}",
  "打开 {name}",
  "移除 {name}",
  "运行 {name}",
  "端口 {port}",
  "未选择{unit}",
  "{count} 条"
];
const messagePatterns = messageTemplates.map(key => {
  const names = [...key.matchAll(/\{([^{}]+)\}/g)].map(match => match[1]);
  const escaped = key.split(/\{[^{}]+\}/g).map(part => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  return { key, names, pattern: new RegExp(`^${escaped.join("(.+?)")}$`, "s") };
});
function translatePattern(source: string, language: ResolvedLanguage): string | null {
  for (const { key, names, pattern } of messagePatterns) {
    const match = source.match(pattern);
    if (match) return t(key, language, Object.fromEntries(names.map((name, index) => [name, match[index + 1]])));
  }
  return null;
}
