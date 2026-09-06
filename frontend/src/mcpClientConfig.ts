import type { McpClientConfig } from "./ipc/generated";
import { translate, type ResolvedLanguage } from "./localization.ts";

export type McpConfigFormat = keyof Pick<McpClientConfig, "installPrompt" | "codex" | "claudeDesktop" | "httpJson" | "codexHttp">;

export function mcpConfigText(config: McpClientConfig, format: McpConfigFormat, language: ResolvedLanguage): string {
  if (format !== "installPrompt") return config[format];
  return [
    translate("请将 ArcRelay MCP 添加到当前桌面 Agent，保留其他 MCP 配置，只新增或更新 arcrelay 项。配置包含令牌，请勿在回复或日志中重复令牌。", language),
    translate("请选择匹配客户端的一种配置，不要混用。它们使用 ArcRelay 自带的本地接入进程，无需 Node 或桥接包，并自动跟随端口变化。", language),
    `Codex config.toml:
\`\`\`toml
${config.codex.trimEnd()}
\`\`\``,
    `Claude Desktop claude_desktop_config.json:
\`\`\`json
${config.claudeDesktop.trimEnd()}
\`\`\``,
    translate("配置后请验证连接，并说明是否需要重启或重新加载客户端。请保持 ArcRelay 运行；访问权限可在 ArcRelay 设置中调整或撤销。", language),
  ].join("\n\n") + "\n";
}
