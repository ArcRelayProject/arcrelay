use serde::Serialize;
use std::path::Path;

#[derive(Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct McpClientConfig {
    pub client_id: String,
    pub endpoint: String,
    pub codex: String,
    pub claude_desktop: String,
    pub http_json: String,
    pub codex_http: String,
    pub install_prompt: String,
}

pub fn generate(
    client_id: &str,
    port: u16,
    token: &str,
    executable: &Path,
) -> Result<McpClientConfig, String> {
    let endpoint = format!("http://127.0.0.1:{port}/mcp");
    // JSON basic strings are also valid TOML basic strings for these paths/tokens.
    let command = executable
        .to_str()
        .ok_or("application executable path is not UTF-8")?;
    let quoted_command = serde_json::to_string(command).map_err(|e| e.to_string())?;
    let quoted_token = serde_json::to_string(token).map_err(|e| e.to_string())?;
    let codex = format!("[mcp_servers.arcrelay]\ncommand = {quoted_command}\nargs = [\"--mcp-stdio\"]\n\n[mcp_servers.arcrelay.env]\nARCRELAY_MCP_TOKEN = {quoted_token}\n");
    let claude_desktop =
        serde_json::to_string_pretty(&serde_json::json!({"mcpServers":{"arcrelay":{
            "command":command, "args":["--mcp-stdio"], "env":{"ARCRELAY_MCP_TOKEN":token}
        }}}))
        .map_err(|e| e.to_string())?;
    let http_json = serde_json::to_string_pretty(&serde_json::json!({"mcpServers":{"arcrelay":{
        "type":"http", "url":endpoint, "headers":{"Authorization":format!("Bearer {token}")}
    }}}))
    .map_err(|e| e.to_string())?;
    let codex_http = format!("[mcp_servers.arcrelay]\nurl = \"{endpoint}\"\nhttp_headers = {{ Authorization = \"Bearer {token}\" }}\n");
    let install_prompt = format!("请帮我把 ArcRelay MCP 添加到当前桌面 Agent。请保留已有 MCP 配置，只新增或更新 arcrelay 项；令牌已内置，请不要在回复或日志里重复令牌。\n\nCodex 使用下面的 TOML 配置。Claude Desktop 使用下面的 JSON 配置。这两种配置调用 ArcRelay 自带的本地接入进程，不需要安装 Node 或其他桥接包，并会自动跟随 ArcRelay 的端口变化。选择与你当前客户端匹配的格式，不要把两种格式混合。\n\nCodex config.toml:\n```toml\n{codex}```\n\nClaude Desktop claude_desktop_config.json:\n```json\n{claude_desktop}\n```\n\n配置后验证连接，并告诉我是否需要重启或重新加载客户端。ArcRelay 需要保持运行。权限由 ArcRelay 设置页中的独立令牌控制，可随时撤销。\n");
    Ok(McpClientConfig {
        client_id: client_id.into(),
        endpoint,
        codex,
        claude_desktop,
        http_json,
        codex_http,
        install_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_configs_include_auth_and_escape_executable_paths() {
        let config = generate(
            "test",
            8766,
            "example-only-token",
            Path::new("C:\\Program Files\\ArcRelay\\arcrelay.exe"),
        )
        .unwrap();
        let desktop: serde_json::Value = serde_json::from_str(&config.claude_desktop).unwrap();
        assert_eq!(
            desktop["mcpServers"]["arcrelay"]["env"]["ARCRELAY_MCP_TOKEN"],
            "example-only-token"
        );
        assert_eq!(desktop["mcpServers"]["arcrelay"]["args"][0], "--mcp-stdio");
        let http: serde_json::Value = serde_json::from_str(&config.http_json).unwrap();
        assert_eq!(
            http["mcpServers"]["arcrelay"]["headers"]["Authorization"],
            "Bearer example-only-token"
        );
        assert!(config.codex.contains("C:\\\\Program Files"));
        let codex: toml::Value = toml::from_str(&config.codex).unwrap();
        assert_eq!(
            codex["mcp_servers"]["arcrelay"]["command"].as_str(),
            desktop["mcpServers"]["arcrelay"]["command"].as_str()
        );
        assert_eq!(
            codex["mcp_servers"]["arcrelay"]["env"]["ARCRELAY_MCP_TOKEN"].as_str(),
            Some("example-only-token")
        );
        let codex_http: toml::Value = toml::from_str(&config.codex_http).unwrap();
        assert_eq!(
            codex_http["mcp_servers"]["arcrelay"]["http_headers"]["Authorization"].as_str(),
            Some("Bearer example-only-token")
        );
        assert!(!config.claude_desktop.contains("npx"));
    }
}
