import { catalogs, t, type ResolvedLanguage } from "./localization.ts";
import type { LanguagePreference } from "./types.ts";

// Developer diagnostics stay stable in English; translate templates only at the UI boundary.
const templates = Object.entries(catalogs.enUs)
  .sort((a, b) => b[1].replace(/\{[^{}]+\}/g, "").length - a[1].replace(/\{[^{}]+\}/g, "").length)
  .filter(([, value]) => value.includes("{"))
  .map(([source, english]) => {
    const names: string[] = [];
    const expression = english
      .split(/(\{[^{}]+\})/g)
      .map((part) => {
        if (part.startsWith("{") && part.endsWith("}")) {
          names.push(part.slice(1, -1));
          return "(.+)";
        }
        return part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      })
      .join("");
    return { source, names, pattern: new RegExp(`^${expression}$`, "s") };
  });
export function diagnosticMessage(value: unknown, language: LanguagePreference | ResolvedLanguage) {
  const message = value instanceof Error ? value.message : String(value);
  if (message === "capture/router/enqueue sample")
    return t("输入采集、路由与发送队列样本", language);
  for (const [pattern, source] of [
    [/^穿越到 (.+)$/, "穿越到 {name}"],
    [/^(.+) 自动重连$/, "{name} 自动重连"],
  ] as const) {
    const match = message.match(pattern);
    if (match) return t(source, language, { name: match[1] });
  }
  for (const template of templates) {
    const match = message.match(template.pattern);
    if (match)
      return t(
        template.source,
        language,
        Object.fromEntries(template.names.map((name, index) => [name, match[index + 1]])),
      );
  }
  return t(message, language);
}
