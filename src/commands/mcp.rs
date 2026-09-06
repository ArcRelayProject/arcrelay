use super::*;
use crate::mcp::{
    access::{McpClientView, McpPermissions},
    client_config::McpClientConfig,
    journal::ConfigurationChange,
};

#[arcrelay_desktop_ipc::command]
pub async fn list_mcp_clients(
    state: State<'_, DesktopState>,
) -> Result<Vec<McpClientView>, String> {
    Ok(state.mcp_access.list())
}

#[arcrelay_desktop_ipc::command]
pub async fn create_mcp_client(
    state: State<'_, DesktopState>,
    name: String,
    permissions: McpPermissions,
) -> Result<McpClientView, String> {
    let access = state.mcp_access.clone();
    tokio::task::spawn_blocking(move || access.create(name, permissions).map(|(client, _)| client))
        .await
        .map_err(|e| e.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn set_mcp_client_permissions(
    state: State<'_, DesktopState>,
    client_id: String,
    permissions: McpPermissions,
) -> Result<(), String> {
    let access = state.mcp_access.clone();
    tokio::task::spawn_blocking(move || access.set_permissions(&client_id, permissions))
        .await
        .map_err(|e| e.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn revoke_mcp_client(
    state: State<'_, DesktopState>,
    client_id: String,
) -> Result<(), String> {
    let access = state.mcp_access.clone();
    tokio::task::spawn_blocking(move || access.revoke(&client_id))
        .await
        .map_err(|e| e.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn rotate_mcp_client_token(
    state: State<'_, DesktopState>,
    client_id: String,
) -> Result<McpClientView, String> {
    let access = state.mcp_access.clone();
    tokio::task::spawn_blocking(move || access.rotate(&client_id).map(|(client, _)| client))
        .await
        .map_err(|e| e.to_string())?
}

#[arcrelay_desktop_ipc::command]
pub async fn get_mcp_client_config(
    state: State<'_, DesktopState>,
    client_id: String,
) -> Result<McpClientConfig, String> {
    let token = state.mcp_access.configuration_token(&client_id)?;
    crate::mcp::client_config::generate(
        &client_id,
        state.mcp_port,
        &token,
        &std::env::current_exe().map_err(|e| e.to_string())?,
    )
}

#[arcrelay_desktop_ipc::command]
pub async fn list_mcp_configuration_changes(
    state: State<'_, DesktopState>,
    limit: u32,
) -> Result<Vec<ConfigurationChange>, String> {
    state
        .mcp_journal
        .changes(None, limit, false)
        .await
        .map_err(|e| e.message)
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_mcp_ipc.rs"));
