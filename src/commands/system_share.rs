use super::*;
use std::sync::Arc;

#[arcrelay_desktop_ipc::command]
pub fn list_system_share_requests(
    service: State<'_, Arc<crate::system_share::SystemShareService>>,
) -> Vec<crate::system_share::SystemShareRequest> {
    service.pending()
}

#[arcrelay_desktop_ipc::command]
pub async fn submit_system_share_request(
    service: State<'_, Arc<crate::system_share::SystemShareService>>,
    state: State<'_, crate::backend::DesktopState>,
    request_id: String,
    transfer_id: String,
    peer_id: String,
) -> Result<(), String> {
    service
        .submit(&request_id, &transfer_id, &peer_id)
        .map_err(|error| error.to_string())?;
    let snapshot = state.transfer().await?.snapshot().await;
    service
        .observe_transfers(&snapshot)
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub fn discard_system_share_request(
    service: State<'_, Arc<crate::system_share::SystemShareService>>,
    request_id: String,
) -> Result<(), String> {
    service
        .discard(&request_id)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
include!(concat!(
    env!("OUT_DIR"),
    "/src_commands_system_share_ipc.rs"
));
