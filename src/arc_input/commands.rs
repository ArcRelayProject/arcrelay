use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use arcrelay_input::{align_system_display_groups, DeskPointUm, DeskVectorUm, DisplayId};
use arcrelay_peer::ServiceInstanceId;

use super::runtime::RuntimeSnapshot;
use super::store::WorkspaceConfiguration;
use super::ArcInputRuntime;

#[arcrelay_desktop_ipc::command]
pub fn get_input_runtime_snapshot(runtime: State<'_, Arc<ArcInputRuntime>>) -> RuntimeSnapshot {
    runtime.snapshot()
}

#[arcrelay_desktop_ipc::command]
pub fn save_input_workspace(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    configuration: WorkspaceConfiguration,
) -> Result<RuntimeSnapshot, String> {
    let topology_revision = configuration
        .layout
        .as_ref()
        .map_or(0, |layout| layout.revision.0);
    let display_count = configuration
        .layout
        .as_ref()
        .map_or(0, |layout| layout.displays.len());
    let portal_count = configuration
        .layout
        .as_ref()
        .map_or(0, |layout| layout.portals.len());
    runtime
        .update_configuration(configuration)
        .map_err(|error| {
            tracing::warn!(
                event = "input.workspace.save_failed",
                topology_revision,
                display_count,
                portal_count,
                %error,
                "Arc Input workspace save failed"
            );
            error.to_string()
        })?;
    tracing::info!(
        event = "input.workspace.saved",
        topology_revision,
        display_count,
        portal_count,
        "Arc Input workspace saved"
    );
    Ok(runtime.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn take_input_control(
    runtime: State<'_, Arc<ArcInputRuntime>>,
) -> Result<RuntimeSnapshot, String> {
    runtime.take_control().await.map_err(|error| {
        tracing::warn!(
            event = "input.control.take_failed",
            %error,
            "Arc Input control request failed"
        );
        error.to_string()
    })?;
    Ok(runtime.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub fn set_input_sharing_enabled(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    enabled: bool,
) -> Result<RuntimeSnapshot, String> {
    runtime
        .set_input_sharing_enabled(enabled)
        .map_err(|error| {
            tracing::warn!(
                event = "input.sharing.state_change_failed",
                enabled,
                %error,
                "Arc Input continuous sharing state change failed"
            );
            error.to_string()
        })?;
    Ok(runtime.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub fn release_input_control(
    runtime: State<'_, Arc<ArcInputRuntime>>,
) -> Result<RuntimeSnapshot, String> {
    runtime.release_control().map_err(|error| {
        tracing::error!(
            event = "input.control.release_failed",
            %error,
            "Arc Input control release failed"
        );
        error.to_string()
    })?;
    Ok(runtime.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn connect_input_peer(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    service_instance_id: String,
    addresses: Vec<String>,
    port: u16,
) -> Result<RuntimeSnapshot, String> {
    let id = ServiceInstanceId::parse(service_instance_id).map_err(|error| error.to_string())?;
    let addresses = addresses
        .into_iter()
        .map(|address| address.parse::<IpAddr>().map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    runtime
        .connect_peer(id, addresses, port)
        .await
        .map_err(|error| error.to_string())?;
    Ok(runtime.snapshot())
}

#[arcrelay_desktop_ipc::command]
pub async fn forget_input_peer(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    service_instance_id: String,
) -> Result<RuntimeSnapshot, String> {
    let id = ServiceInstanceId::parse(service_instance_id).map_err(|error| error.to_string())?;
    runtime.forget_peer(id).await.map_err(|error| {
        tracing::warn!(
            event = "input.pairing.forget_failed",
            %error,
            "Arc Input peer removal failed"
        );
        error.to_string()
    })?;
    Ok(runtime.snapshot())
}

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct EdgeTestRequest {
    pub display_id: String,
    pub point_x_um: i64,
    pub point_y_um: i64,
    pub delta_x_um: i64,
    pub delta_y_um: i64,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct EdgeTestResult {
    pub display_id: String,
    pub point_x_um: i64,
    pub point_y_um: i64,
    pub portal_ids: Vec<String>,
}

#[arcrelay_desktop_ipc::command]
pub fn test_input_edge(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    request: EdgeTestRequest,
    configuration: Option<WorkspaceConfiguration>,
) -> Result<EdgeTestResult, String> {
    let display = DisplayId::parse(request.display_id).map_err(|e| e.to_string())?;
    let point = DeskPointUm {
        x: request.point_x_um,
        y: request.point_y_um,
    };
    let delta = DeskVectorUm {
        x: request.delta_x_um,
        y: request.delta_y_um,
    };
    let outcome = if let Some(configuration) = configuration {
        let preview = runtime
            .preview_configuration(configuration)
            .map_err(|e| e.to_string())?;
        let layout = preview.layout.ok_or("workspace is not configured")?;
        let router = arcrelay_input::RuntimeRouter::new(
            arcrelay_input::RuntimeRoutingSnapshot::compile(layout).map_err(|e| e.to_string())?,
        );
        router
            .route(
                router.revision(),
                display,
                point,
                delta,
                arcrelay_input::RoutingContext::default(),
            )
            .map_err(|e| e.to_string())?
    } else {
        runtime
            .route_motion(display, point, delta)
            .map_err(|e| e.to_string())?
    };
    tracing::info!(
        event = "input.portal.test_completed",
        crossing_count = outcome.crossings.len(),
        "Arc Input portal test completed"
    );
    Ok(EdgeTestResult {
        display_id: outcome.display_id.to_string(),
        point_x_um: outcome.desk_point.x,
        point_y_um: outcome.desk_point.y,
        portal_ids: outcome
            .crossings
            .into_iter()
            .map(|crossing| crossing.portal_id.to_string())
            .collect(),
    })
}

#[arcrelay_desktop_ipc::command]
pub fn preview_input_workspace(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    configuration: WorkspaceConfiguration,
) -> Result<WorkspaceConfiguration, String> {
    runtime
        .preview_configuration(configuration)
        .map_err(|e| e.to_string())
}

#[arcrelay_desktop_ipc::command]
pub fn arrange_input_workspace(
    runtime: State<'_, Arc<ArcInputRuntime>>,
    mut configuration: WorkspaceConfiguration,
) -> Result<WorkspaceConfiguration, String> {
    if let Some(layout) = configuration.layout.as_mut() {
        align_system_display_groups(&mut layout.displays).map_err(|error| error.to_string())?;
        let devices = layout
            .displays
            .values()
            .map(|display| display.device_id.clone())
            .collect::<BTreeSet<_>>();
        let mut cursor = 0i64;
        for device in devices {
            let group = layout
                .displays
                .values()
                .filter(|display| display.device_id == device)
                .collect::<Vec<_>>();
            let min_x = group
                .iter()
                .map(|display| display.desk_rect_um.x)
                .min()
                .unwrap_or(0);
            let min_y = group
                .iter()
                .map(|display| display.desk_rect_um.y)
                .min()
                .unwrap_or(0);
            let max_x = group
                .iter()
                .map(|display| {
                    display
                        .desk_rect_um
                        .x
                        .saturating_add(display.desk_rect_um.width)
                })
                .max()
                .unwrap_or(min_x);
            let width = max_x.saturating_sub(min_x);
            let dx = cursor.saturating_sub(min_x);
            let dy = min_y.saturating_neg();
            for display in layout
                .displays
                .values_mut()
                .filter(|display| display.device_id == device)
            {
                display.desk_rect_um.x = display.desk_rect_um.x.saturating_add(dx);
                display.desk_rect_um.y = display.desk_rect_um.y.saturating_add(dy);
            }
            cursor = cursor
                .checked_add(width)
                .ok_or("layout dimensions overflowed")?;
        }
    }
    runtime
        .preview_configuration(configuration)
        .map_err(|e| e.to_string())
}
