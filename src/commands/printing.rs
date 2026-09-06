use super::*;

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrinterSharingSnapshot {
    pub hosting_supported: bool,
    pub local_printers: Vec<arcrelay_print::LocalPrinter>,
    pub shares: Vec<arcrelay_print::PrinterShare>,
    pub remote_printers: Vec<arcrelay_print::infrastructure::quic::RemotePrinter>,
    pub queue_bindings: Vec<arcrelay_print::RemoteQueueBinding>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedPrintJobView {
    pub id: String,
    pub document_name: String,
    pub document_size_bytes: u64,
    pub source_device_id: String,
    pub printer_name: String,
    pub state: arcrelay_print::PrintJobState,
    pub failure_message: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PrintJobActivitySnapshot {
    pub revision: u64,
    pub received: Vec<ReceivedPrintJobView>,
    pub sent: Vec<arcrelay_print::infrastructure::ipp::OutgoingPrintJob>,
}

#[arcrelay_desktop_ipc::command]
pub async fn get_printer_sharing_state(
    state: State<'_, DesktopState>,
) -> Result<PrinterSharingSnapshot, String> {
    if let Err(error) = state.ensure_print_network().await {
        tracing::warn!(%error, "could not activate printer network on demand");
    }
    let print = state.print().await?;
    let (local_printers, shares) =
        tokio::try_join!(print.shares.list_local_printers(), print.shares.list(),)
            .map_err(|error| error.to_string())?;
    let remote_printers = match state.print_network.read().await.as_ref() {
        Some(network) => network.snapshot().await,
        None => Vec::new(),
    };
    let queue_bindings = state
        .print()
        .await?
        .queues
        .list()
        .await
        .map_err(|error| error.to_string())?;
    Ok(PrinterSharingSnapshot {
        hosting_supported: state.print().await?.shares.hosting_supported(),
        local_printers,
        shares,
        remote_printers,
        queue_bindings,
    })
}

#[arcrelay_desktop_ipc::command]
pub async fn get_print_job_activity(
    state: State<'_, DesktopState>,
) -> Result<PrintJobActivitySnapshot, String> {
    let revision = state.print_job_revision.fetch_add(1, Ordering::Relaxed) + 1;
    if let Err(error) = state.ensure_print_network().await {
        tracing::warn!(%error, "could not activate printer network for job activity");
    }
    let shares = state
        .print()
        .await?
        .shares
        .list()
        .await
        .map_err(|error| error.to_string())?;
    let share_names = shares
        .into_iter()
        .map(|share| (share.id.to_string(), share.display_name))
        .collect::<HashMap<_, _>>();
    let received = state
        .print()
        .await?
        .jobs
        .list_recent(50, true)
        .await
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|job| ReceivedPrintJobView {
            id: job.id.to_string(),
            document_name: job.document.name,
            document_size_bytes: job.document.size_bytes,
            source_device_id: job.source_device_id.to_string(),
            printer_name: share_names
                .get(job.printer_share_id.as_str())
                .cloned()
                .unwrap_or_else(|| "local shared printer".to_string()),
            state: job.state,
            failure_message: job.failure_message,
            created_at_ms: job.created_at_ms,
            updated_at_ms: job.updated_at_ms,
        })
        .collect();
    let sent = match state.print_ipp.read().await.as_ref() {
        Some(bridge) => bridge.list_jobs(true, 50).await,
        None => Vec::new(),
    };
    Ok(PrintJobActivitySnapshot {
        revision,
        received,
        sent,
    })
}

#[arcrelay_desktop_ipc::command]
pub async fn install_remote_printer(
    state: State<'_, DesktopState>,
    source_device_id: String,
    share_id: String,
) -> Result<PrinterSharingSnapshot, String> {
    state.ensure_print_network().await?;
    let network = state
        .print_network
        .read()
        .await
        .clone()
        .ok_or_else(|| "print network service is not running".to_string())?;
    let remote = network
        .snapshot()
        .await
        .into_iter()
        .find(|remote| {
            remote.printer.source_device_id.as_str() == source_device_id
                && remote.printer.share_id.as_str() == share_id
        })
        .ok_or_else(|| "shared printer is offline; refresh and try again".to_string())?;
    let ipp = state
        .print_ipp
        .read()
        .await
        .clone()
        .ok_or_else(|| "local IPP service is not running".to_string())?;
    let queue_name = printer_queue_name(&remote.printer.display_name, &share_id);
    state
        .print()
        .await?
        .queues
        .install(remote.printer.clone(), queue_name, ipp.printer_uri(&remote))
        .await
        .map_err(|error| error.to_string())?;
    get_printer_sharing_state(state)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn remove_remote_printer(
    state: State<'_, DesktopState>,
    binding_id: String,
) -> Result<PrinterSharingSnapshot, String> {
    let id =
        arcrelay_print::QueueBindingId::parse(binding_id).map_err(|error| error.to_string())?;
    state
        .print()
        .await?
        .queues
        .remove(&id)
        .await
        .map_err(|error| error.to_string())?;
    get_printer_sharing_state(state)
        .await
        .map_err(|error| error.to_string())
}

fn printer_queue_name(display_name: &str, share_id: &str) -> String {
    let mut name = String::from("ArcRelay_");
    for character in display_name.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
            name.push(character);
        } else if !name.ends_with('_') {
            name.push('_');
        }
        if name.len() >= 96 {
            break;
        }
    }
    if name == "ArcRelay_" {
        name.push_str("Printer");
    }
    name.push('_');
    name.extend(
        share_id
            .chars()
            .filter(|value| value.is_ascii_hexdigit())
            .take(8),
    );
    name
}

#[arcrelay_desktop_ipc::command]
pub async fn refresh_printer_sharing_state(
    state: State<'_, DesktopState>,
) -> Result<PrinterSharingSnapshot, String> {
    state.ensure_print_network().await?;
    let network = state.print_network.read().await.clone();
    if let Some(network) = network {
        match tokio::time::timeout(std::time::Duration::from_secs(8), network.refresh()).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(
                event = "print.discovery.refresh_failed",
                %error,
                "could not refresh LAN printers; returning the cached snapshot"
            ),
            Err(_) => tracing::warn!(
                event = "print.discovery.refresh_timeout",
                "LAN printer refresh timed out; returning the cached snapshot"
            ),
        }
    }
    get_printer_sharing_state(state)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn publish_printer(
    state: State<'_, DesktopState>,
    local_printer_id: String,
) -> Result<PrinterSharingSnapshot, String> {
    state.ensure_print_network().await?;
    let id = arcrelay_print::LocalPrinterId::parse(local_printer_id)
        .map_err(|error| error.to_string())?;
    state
        .print()
        .await?
        .shares
        .publish(&id)
        .await
        .map_err(|error| error.to_string())?;
    get_printer_sharing_state(state)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn suspend_printer_share(
    state: State<'_, DesktopState>,
    share_id: String,
) -> Result<PrinterSharingSnapshot, String> {
    let id = arcrelay_print::PrinterShareId::parse(share_id).map_err(|error| error.to_string())?;
    state
        .print()
        .await?
        .shares
        .suspend(&id)
        .await
        .map_err(|error| error.to_string())?;
    get_printer_sharing_state(state)
        .await
        .map_err(|error| error.to_string())
}

#[arcrelay_desktop_ipc::command]
pub async fn observe_print_jobs(
    app: AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    enabled: bool,
) -> Result<(), String> {
    if enabled {
        state.print_activity.start(app, window.label().to_string());
    } else {
        state.print_activity.stop(window.label());
    }
    Ok(())
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_printing_ipc.rs"));
