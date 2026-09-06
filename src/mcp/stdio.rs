//! Bundled stdio → local HTTP adapter for desktop MCP clients. This mode exits
//! before Tauri, logging, single-instance forwarding or backend initialization.
use serde_json::{json, Value};
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub fn requested(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    args.into_iter()
        .skip(1)
        .any(|argument| argument == "--mcp-stdio")
}
pub fn read_port(directory: &Path) -> Option<u16> {
    let value: Value =
        serde_json::from_slice(&std::fs::read(directory.join("mcp/endpoint.json")).ok()?).ok()?;
    u16::try_from(value.get("port")?.as_u64()?)
        .ok()
        .filter(|port| *port > 0)
}
pub fn run() -> Result<(), String> {
    let token = std::env::var("ARCRELAY_MCP_TOKEN").map_err(|_| {
        "ARCRELAY_MCP_TOKEN is missing; copy a client configuration from ArcRelay settings"
    })?;
    let directory = dirs::config_dir()
        .ok_or("local configuration directory is unavailable")?
        .join("ArcRelay");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(bridge(
            tokio::io::stdin(),
            tokio::io::stdout(),
            &directory,
            &token,
        ))
}

async fn forward(
    client: &reqwest::Client,
    port: u16,
    token: &str,
    message: &Value,
    version: &str,
) -> Result<Option<Value>, String> {
    let response = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .bearer_auth(token)
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", version)
        .json(message)
        .send()
        .await
        .map_err(|_| {
            "ArcRelay is not reachable; start the desktop application and retry".to_string()
        })?;
    if !response.status().is_success() {
        return Err(if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            "ArcRelay rejected this token; its authorization may have been revoked"
        } else {
            "ArcRelay rejected the MCP request; check protocol compatibility"
        }
        .into());
    }
    if response.status() == reqwest::StatusCode::ACCEPTED
        || response.status() == reqwest::StatusCode::NO_CONTENT
    {
        return Ok(None);
    }
    // The built-in server runs statelessly with JSON responses and does not send
    // sampling/elicitation requests. Every operation returns promptly with a run ID.
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "could not read ArcRelay's MCP response")?;
    if bytes.is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "ArcRelay returned an invalid JSON MCP response")?;
    Ok(Some(value))
}

pub(super) async fn bridge<R, W>(
    input: R,
    mut output: W,
    directory: &Path,
    token: &str,
) -> Result<(), String>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let mut input = BufReader::new(input);
    let mut version = "2025-11-25".to_string();
    loop {
        let mut line = String::new();
        let count = (&mut input)
            .take(4 * 1024 * 1024 + 1)
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        if count > 4 * 1024 * 1024 {
            return Err("MCP request exceeds 4 MiB".into());
        }
        let message: Value =
            serde_json::from_str(line.trim()).map_err(|_| "invalid JSON on MCP stdin")?;
        let id = message.get("id").cloned();
        if message.get("method").and_then(Value::as_str) == Some("initialize") {
            if let Some(requested) = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
            {
                version = requested.to_string();
            }
        }
        let result = match read_port(directory) {
            Some(port) => forward(&client, port, token, &message, &version).await,
            None => {
                Err("ArcRelay has no local endpoint; start the desktop application first".into())
            }
        };
        let reply = match result {
            Ok(Some(reply)) => {
                if let Some(negotiated) = reply
                    .pointer("/result/protocolVersion")
                    .and_then(Value::as_str)
                {
                    version = negotiated.to_string();
                }
                Some(reply)
            }
            Ok(None) => None,
            Err(error) => id
                .map(|id| json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":error}})),
        };
        if let Some(reply) = reply {
            let mut bytes = serde_json::to_vec(&reply).map_err(|e| e.to_string())?;
            bytes.push(b'\n');
            output.write_all(&bytes).await.map_err(|e| e.to_string())?;
            output.flush().await.map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stdio_mode_is_selected_before_any_runtime_or_app_initialization() {
        assert!(requested(["ArcRelay".into(), "--mcp-stdio".into()]));
        assert!(!requested(["ArcRelay".into()]));
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(read_port(directory.path()), None);
    }
    #[tokio::test]
    async fn stopped_host_returns_protocol_error_without_leaking_token() {
        let directory = tempfile::tempdir().unwrap();
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n";
        let mut output = vec![];
        bridge(&input[..], &mut output, directory.path(), "example-secret")
            .await
            .unwrap();
        let result: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(result["id"], 1);
        assert!(result.get("error").is_some());
        assert!(!String::from_utf8(output)
            .unwrap()
            .contains("example-secret"));
    }
}
