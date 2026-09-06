import assert from "node:assert/strict";
import test from "node:test";
import { mcpConfigText } from "./mcpClientConfig.ts";
import { languageOptions } from "./localization.ts";

const config = {
  clientId: "test-agent",
  endpoint: "http://127.0.0.1:1234/mcp",
  codex: '[mcp_servers.arcrelay]\nargs = ["/Applications/ArcRelay 中文.app", "test-{token}-$1"]\n',
  claudeDesktop: '{"mcpServers":{"arcrelay":{"args":["test-{token}-$1"]}}}',
  httpJson: '{"url":"http://127.0.0.1:1234/mcp"}',
  codexHttp: '[mcp_servers.arcrelay]\nurl = "http://127.0.0.1:1234/mcp"',
  installPrompt: "Old server prompt must not override the selected language",
};

test("installation instructions follow every supported language and preserve configuration data", () => {
  const introductions = new Set<string>();
  for (const { value } of languageOptions) {
    const prompt = mcpConfigText(config, "installPrompt", value);
    introductions.add(prompt.split("\n")[0]);
    assert.ok(prompt.includes(`\`\`\`toml\n${config.codex}\`\`\``), value);
    assert.ok(prompt.includes(`\`\`\`json\n${config.claudeDesktop}\n\`\`\``), value);
    assert.ok(!prompt.includes(config.installPrompt), value);
    if (value !== "zhCn") assert.ok(!prompt.includes("请将"), value);
  }
  assert.equal(introductions.size, languageOptions.length);
  assert.match(mcpConfigText(config, "installPrompt", "enUs"), /do not repeat it in replies or logs/);
});

test("raw client configurations are copied unchanged in every language", () => {
  for (const { value } of languageOptions) {
    for (const format of ["codex", "claudeDesktop", "httpJson", "codexHttp"] as const) {
      assert.equal(mcpConfigText(config, format, value), config[format]);
    }
  }
});
