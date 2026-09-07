use std::collections::{HashMap, HashSet, VecDeque};
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{broadcast, mpsc, oneshot};
use tracing::info;

use arcrelay_core::application::clipboard_service::ClipboardApplicationService;
use arcrelay_core::application::service::ArcRelayService;
use arcrelay_core::application::state_coordinator::StateCoordinator;
use arcrelay_core::domain::input_control::InputPermissionState;
use arcrelay_core::domain::{
    clipboard::{ClipboardRepository, ClipboardSyncPreferences},
    input_control::InputControlRepository,
    window_manager::WindowManagerRepository,
};
use arcrelay_core::infrastructure::{
    clipboard::NativeClipboard, device::NativeDeviceRepo, input_control::NativeInputControl,
    media_control::NativeMediaControl, process::SysInfoProcessRepo, system_monitor::SysInfoMonitor,
    window_manager::NativeWindowManager,
};
use arcrelay_network::{
    DeviceIdentity, DeviceMetadata, NetworkRuntime, NetworkRuntimeConfig, SqlitePeerRepository,
};
use arcrelay_peer::{
    CapabilityId, DeviceId, Grant, GrantConstraints, GrantDirection, PeerRepository,
};
use arcrelay_protocol::server::{
    ConnectionRegistry, DesktopControlService, PairingApproval, PairingRequestForApproval,
    ServerEvent,
};
use arcrelay_transfer::{
    TransferConfig, TransferDirection, TransferManager, TransferSnapshot, TransferStatus,
    TransferView,
};

use crate::application::action_service::ActionService;
use crate::application::host_capabilities::AppActionProvider;
use crate::application::output_manager::OutputManager;
use crate::clipboard_sync::ClipboardSyncManager;
use crate::domain::quick_action::QuickAction;
use crate::infrastructure::persistence::JsonStore;
use crate::notification::{HostNotification, NotificationKind, NotificationStore};
use crate::privacy::{PrivacyManager, PrivacySnapshot};
use crate::remote_files::RemoteFileManager;
use crate::screenshot::ScreenshotService;
use crate::settings::{AppSettings, SettingsManager};
use arcrelay_web_gateway::{WebGatewaySettings, WebGatewaySupervisor};

const DEFAULT_PORT: u16 = 8765;
const PORT_SCAN_COUNT: u16 = 11;
const MAX_ACTIVITY_LINES: usize = 80;

fn web_hostname(device_id: &str) -> String {
    arcrelay_peer::DeviceId::parse(device_id)
        .map(|id| arcrelay_network::device_hostname(&id))
        .unwrap_or_else(|_| "localhost".into())
}

fn compose_native_service(
    database_path: Option<std::path::PathBuf>,
    device_id: String,
    device_name: String,
    resources: Arc<arcrelay_content::ContentResources>,
) -> arcrelay_core::Result<ArcRelayService> {
    let input_control: Arc<dyn InputControlRepository> = Arc::new(NativeInputControl::new());
    let window_manager: Arc<dyn WindowManagerRepository> = Arc::new(NativeWindowManager::new());
    let clipboard_repository: Arc<dyn ClipboardRepository> =
        Arc::new(NativeClipboard::with_resources(
            database_path,
            window_manager.clone(),
            device_id,
            device_name,
            resources,
        )?);
    Ok(ArcRelayService::compose(
        Arc::new(SysInfoMonitor::new()),
        Arc::new(SysInfoProcessRepo::new()),
        Arc::new(NativeMediaControl::new()),
        Arc::new(ClipboardApplicationService::new(
            clipboard_repository,
            input_control.clone(),
        )),
        Arc::new(NativeDeviceRepo::new()),
        window_manager,
        input_control,
    ))
}

#[derive(Debug)]
pub enum BackendCommand {
    ForgetPairedDevice(String),
    DisconnectDevice(String),
    SetDeviceAutoConnect {
        device_id: String,
        enabled: bool,
        response: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    RefreshInputPermission(Option<tokio::sync::oneshot::Sender<InputPermissionState>>),
    OpenInputPermissionSettings,
    DisconnectAllDevices,
    Shutdown(oneshot::Sender<()>),
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ConnectedDeviceView {
    pub id: String,
    pub name: String,
    pub auto_connect: bool,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct InputMetricsView {
    pub receive_hz: f32,
    pub quartz_hz: f32,
    pub average_gap_us: u64,
    pub maximum_gap_us: u64,
    pub maximum_source_gap_us: u64,
    pub maximum_transport_stall_us: u64,
    pub average_apply_us: u64,
    pub maximum_apply_us: u64,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairingView {
    pub device_name: String,
    pub device_id: String,
    pub pairing_code: String,
    pub permissions: Vec<String>,
    pub grants: Vec<PendingPairingGrantView>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingPairingGrantView {
    pub id: String,
    pub capability: String,
    pub direction: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct OutgoingPairingView {
    pub device_name: String,
    pub device_id: String,
    pub pairing_code: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActionView {
    #[serde(flatten)]
    pub action: QuickAction,
    pub action_type_label: String,
    pub is_toggle: bool,
    pub is_running: bool,
    pub requires_confirmation: bool,
    pub global_shortcut_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapState {
    #[serde(flatten)]
    pub runtime: DesktopRuntimeState,
    pub actions: Vec<ActionView>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DesktopRuntimeState {
    pub revision: u64,
    pub port: u16,
    pub server_running: bool,
    pub connected_devices: Vec<ConnectedDeviceView>,
    pub paired_devices: Vec<ConnectedDeviceView>,
    pub pending_pairing: Option<PendingPairingView>,
    pub outgoing_pairings: Vec<OutgoingPairingView>,
    pub input_permission: InputPermissionState,
    pub active_input_device: Option<ConnectedDeviceView>,
    pub input_metrics: Option<InputMetricsView>,
    pub activity: Vec<String>,
    pub unread_notification_count: usize,
    pub mcp_running: bool,
    pub mcp_port: u16,
    pub privacy: PrivacySnapshot,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct NotificationView {
    pub id: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub kind: NotificationKind,
    pub reference: Option<String>,
    pub created_at_ms: i64,
    pub read_at_ms: Option<i64>,
    pub read_by_device_name: Option<String>,
    pub delivery_state: String,
}

mod prepared;
mod runtime;
mod transfer_notifications;

pub use prepared::PreparedBackend;
pub(crate) use runtime::effective_device_name;
use runtime::*;
use transfer_notifications::*;

struct RuntimeViewState {
    server_running: bool,
    connected_devices: Vec<ConnectedDeviceView>,
    connected_device_counts: HashMap<String, usize>,
    input_connected_device_ids: HashSet<String>,
    paired_devices: Vec<ConnectedDeviceView>,
    pending_pairing: Option<PendingPairingView>,
    outgoing_pairings: Vec<OutgoingPairingView>,
    input_permission: InputPermissionState,
    active_input_device: Option<ConnectedDeviceView>,
    active_input_session_id: Option<String>,
    input_metrics: Option<InputMetricsView>,
    activity: VecDeque<String>,
}

struct PendingPairingResponse {
    device_id: String,
    respond: mpsc::Sender<PairingApproval>,
    grants: Vec<Grant>,
}

impl RuntimeViewState {
    fn new(port: u16) -> Self {
        let mut activity = VecDeque::with_capacity(MAX_ACTIVITY_LINES);
        activity.push_back(format!("service started on port {port}"));
        activity.push_back("waiting for device connections".to_string());
        Self {
            server_running: true,
            connected_devices: Vec::new(),
            connected_device_counts: HashMap::new(),
            input_connected_device_ids: HashSet::new(),
            paired_devices: Vec::new(),
            pending_pairing: None,
            outgoing_pairings: Vec::new(),
            input_permission: InputPermissionState::Unsupported,
            active_input_device: None,
            active_input_session_id: None,
            input_metrics: None,
            activity,
        }
    }

    fn push_activity(&mut self, line: impl Into<String>) {
        self.activity.push_back(line.into());
        while self.activity.len() > MAX_ACTIVITY_LINES {
            self.activity.pop_front();
        }
    }

    fn register_connected_device(&mut self, device_id: String, device_name: String) {
        // A live session is stronger evidence than the transient outgoing
        // pairing state. Clear it here as a defensive invariant in case the
        // explicit pairing-finished event is delayed or lost.
        self.outgoing_pairings
            .retain(|pairing| pairing.device_id != device_id);
        *self
            .connected_device_counts
            .entry(device_id.clone())
            .or_default() += 1;
        if let Some(device) = self
            .connected_devices
            .iter_mut()
            .find(|device| device.id == device_id)
        {
            device.name = device_name;
        } else {
            self.connected_devices.push(ConnectedDeviceView {
                id: device_id,
                name: device_name,
                auto_connect: true,
            });
        }
    }

    fn unregister_connected_device(&mut self, device_id: &str) {
        let remove_control_source = match self.connected_device_counts.get_mut(device_id) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            _ => true,
        };
        if remove_control_source {
            self.connected_device_counts.remove(device_id);
            if !self.input_connected_device_ids.contains(device_id) {
                self.connected_devices
                    .retain(|device| device.id != device_id);
            }
        }
    }

    fn sync_input_connected_devices(&mut self, devices: Vec<ConnectedDeviceView>) -> bool {
        let next_ids = devices
            .iter()
            .map(|device| device.id.clone())
            .collect::<HashSet<_>>();
        let mut changed = next_ids != self.input_connected_device_ids;
        self.input_connected_device_ids = next_ids;

        for device in devices {
            let previous_pairing_count = self.outgoing_pairings.len();
            self.outgoing_pairings
                .retain(|pairing| pairing.device_id != device.id);
            changed |= previous_pairing_count != self.outgoing_pairings.len();
            if let Some(existing) = self
                .connected_devices
                .iter_mut()
                .find(|existing| existing.id == device.id)
            {
                if existing.name != device.name {
                    existing.name = device.name;
                    changed = true;
                }
            } else {
                self.connected_devices.push(device);
                changed = true;
            }
        }

        let previous_len = self.connected_devices.len();
        self.connected_devices.retain(|device| {
            self.connected_device_counts.contains_key(&device.id)
                || self.input_connected_device_ids.contains(&device.id)
        });
        changed || previous_len != self.connected_devices.len()
    }
}

#[derive(Clone)]
pub struct DesktopState {
    pub port: u16,
    pub local_device_id: String,
    pub action_service: Arc<Mutex<ActionService>>,
    pub action_shortcuts: Arc<crate::action_shortcuts::ActionShortcuts>,
    pub automations: crate::application::automations::DesktopAutomations,
    pub output_manager: Arc<OutputManager>,
    pub notification_store: NotificationStore,
    pub mcp_port: u16,
    pub mcp_token: String,
    pub mcp_access: Arc<crate::mcp::access::McpAccess>,
    pub mcp_journal: crate::mcp::journal::McpJournal,
    pub mcp_running: Arc<AtomicBool>,
    pub privacy: Arc<PrivacyManager>,
    pub print_activity: Arc<crate::application::print_activity::PrintActivityObservers>,
    pub print_job_revision: Arc<std::sync::atomic::AtomicU64>,
    pub modules: Arc<crate::application::runtime_modules::RuntimeModules>,
    pub print_network: Arc<
        tokio::sync::RwLock<Option<Arc<arcrelay_print::infrastructure::quic::PrintQuicService>>>,
    >,
    pub print_ipp:
        Arc<tokio::sync::RwLock<Option<Arc<arcrelay_print::infrastructure::ipp::IppBridge>>>>,
    print_activation: Arc<tokio::sync::Mutex<()>>,
    pub clipboard: Arc<ClipboardApplicationService>,
    pub clipboard_sync: Arc<ClipboardSyncManager>,
    pub remote_files: Arc<RemoteFileManager>,
    pub web_gateway: WebGatewaySupervisor,
    pub settings: SettingsManager,
    pub screenshot: Arc<ScreenshotService>,
    pub server_name: Arc<tokio::sync::RwLock<String>>,
    pub command_tx: mpsc::Sender<BackendCommand>,
    pub quitting: Arc<AtomicBool>,
    pub network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
    snapshot_revision: Arc<Mutex<u64>>,
    pub remote_file_service: Arc<crate::application::remote_file_service::RemoteFileService>,
    pub system_folders: Arc<tokio::sync::OnceCell<Arc<crate::system_folders::SystemFolders>>>,
    pub text_selection: Arc<crate::application::text_selection::TextSelectionService>,
    runtime: Arc<RwLock<RuntimeViewState>>,
    pairing_response: Arc<Mutex<Option<PendingPairingResponse>>>,
}

struct DesktopStateInit {
    port: u16,
    local_device_id: String,
    action_service: Arc<Mutex<ActionService>>,
    automations: crate::application::automations::DesktopAutomations,
    output_manager: Arc<OutputManager>,
    notification_store: NotificationStore,
    mcp_port: u16,
    mcp_token: String,
    mcp_access: Arc<crate::mcp::access::McpAccess>,
    mcp_journal: crate::mcp::journal::McpJournal,
    privacy: Arc<PrivacyManager>,
    modules: Arc<crate::application::runtime_modules::RuntimeModules>,
    clipboard: Arc<ClipboardApplicationService>,
    clipboard_sync: Arc<ClipboardSyncManager>,
    remote_files: Arc<RemoteFileManager>,
    web_gateway: WebGatewaySupervisor,
    settings: SettingsManager,
    screenshot: Arc<ScreenshotService>,
    server_name: Arc<tokio::sync::RwLock<String>>,
    command_tx: mpsc::Sender<BackendCommand>,
    network: Arc<tokio::sync::OnceCell<Arc<NetworkRuntime>>>,
}

impl DesktopState {
    fn new(init: DesktopStateInit) -> Self {
        let DesktopStateInit {
            port,
            local_device_id,
            action_service,
            automations,
            output_manager,
            notification_store,
            mcp_port,
            mcp_token,
            mcp_access,
            mcp_journal,
            privacy,
            modules,
            clipboard,
            clipboard_sync,
            remote_files,
            web_gateway,
            settings,
            screenshot,
            server_name,
            command_tx,
            network,
        } = init;
        Self {
            port,
            local_device_id,
            action_service,
            action_shortcuts: Arc::default(),
            automations,
            output_manager,
            notification_store,
            mcp_port,
            mcp_token,
            mcp_access,
            mcp_journal,
            mcp_running: Arc::new(AtomicBool::new(false)),
            privacy,
            modules,
            print_network: Arc::new(tokio::sync::RwLock::new(None)),
            print_ipp: Arc::new(tokio::sync::RwLock::new(None)),
            print_activation: Arc::new(tokio::sync::Mutex::new(())),
            clipboard,
            clipboard_sync,
            remote_files,
            web_gateway,
            settings,
            screenshot,
            server_name,
            command_tx,
            quitting: Arc::new(AtomicBool::new(false)),
            network,
            snapshot_revision: Arc::new(Mutex::new(0)),
            print_activity: Arc::default(),
            print_job_revision: Arc::default(),
            text_selection: Arc::default(),
            remote_file_service: Arc::default(),
            system_folders: crate::system_folders::empty_slot(),
            runtime: Arc::new(RwLock::new(RuntimeViewState::new(port))),
            pairing_response: Arc::new(Mutex::new(None)),
        }
    }

    pub fn snapshot(&self) -> BootstrapState {
        let mut revision = lock_unpoison(&self.snapshot_revision);
        *revision += 1;
        let actions = {
            let mut service = lock_unpoison(&self.action_service);
            service.reap_finished_toggles();
            let items = service.actions().to_vec();
            items
                .into_iter()
                .map(|action| {
                    let is_toggle = action.is_toggle();
                    let is_running = is_toggle && service.is_toggle_running(&action.id);
                    ActionView {
                        action_type_label: action.action_type_label().to_string(),
                        is_toggle,
                        is_running,
                        requires_confirmation: action.requires_confirmation(),
                        global_shortcut_error: self.action_shortcuts.error(&action.id),
                        action,
                    }
                })
                .collect()
        };

        BootstrapState {
            runtime: self.runtime_snapshot(*revision),
            actions,
        }
    }

    /// Lightweight device view for native surfaces, without action snapshots or revision changes.
    pub fn device_views(&self) -> (Vec<ConnectedDeviceView>, Vec<ConnectedDeviceView>) {
        let runtime = read_unpoison(&self.runtime);
        (
            runtime.connected_devices.clone(),
            runtime.paired_devices.clone(),
        )
    }

    pub fn automation_device_ids(&self) -> Vec<String> {
        read_unpoison(&self.runtime)
            .connected_devices
            .iter()
            .map(|d| d.id.clone())
            .collect()
    }

    fn runtime_snapshot(&self, revision: u64) -> DesktopRuntimeState {
        let (
            server_running,
            connected_devices,
            paired_devices,
            pending_pairing,
            outgoing_pairings,
            input_permission,
            active_input_device,
            input_metrics,
            activity,
        ) = {
            let runtime = read_unpoison(&self.runtime);
            (
                runtime.server_running,
                runtime.connected_devices.clone(),
                runtime.paired_devices.clone(),
                runtime.pending_pairing.clone(),
                runtime.outgoing_pairings.clone(),
                runtime.input_permission,
                runtime.active_input_device.clone(),
                runtime.input_metrics.clone(),
                runtime.activity.iter().cloned().collect(),
            )
        };
        let unread_notification_count = self.notification_store.unread_count().unwrap_or_default();
        DesktopRuntimeState {
            revision,
            port: self.port,
            server_running,
            connected_devices,
            paired_devices,
            pending_pairing,
            outgoing_pairings,
            input_permission,
            active_input_device,
            input_metrics,
            activity,
            unread_notification_count,
            mcp_running: self.mcp_running.load(Ordering::SeqCst),
            mcp_port: self.mcp_port,
            privacy: self.privacy.snapshot(),
        }
    }

    fn is_device_connected(&self, device_id: &str) -> bool {
        let runtime = read_unpoison(&self.runtime);
        runtime.connected_device_counts.contains_key(device_id)
            || runtime.input_connected_device_ids.contains(device_id)
    }

    pub fn notification_views(
        &self,
        include_read: bool,
        limit: usize,
    ) -> Result<Vec<NotificationView>, String> {
        let has_connected_device = !read_unpoison(&self.runtime).connected_devices.is_empty();
        self.notification_store
            .list(include_read, limit)
            .map(|notifications| {
                notifications
                    .into_iter()
                    .map(|notification| notification_view(notification, has_connected_device))
                    .collect()
            })
    }

    pub async fn transfer(&self) -> Result<Arc<TransferManager>, String> {
        let _mutation = crate::action_shortcuts::MUTATION.lock().await;
        let settings = self.settings.snapshot();
        self.modules
            .transfer(
                effective_device_name(&settings.device_name),
                settings.nearby_discoverable,
            )
            .await
    }
    pub async fn print(&self) -> Result<Arc<arcrelay_print::application::PrintRuntime>, String> {
        self.modules.print().await
    }

    pub async fn print_requires_background(&self) -> bool {
        if !self.modules.print_has_saved_state && self.modules.initialized_print().is_none() {
            return false;
        }
        let Ok(print) = self.modules.print().await else {
            return false;
        };
        let has_published_share = print
            .shares
            .list()
            .await
            .map(|shares| {
                shares
                    .into_iter()
                    .any(|share| share.state == arcrelay_print::ShareState::Published)
            })
            .unwrap_or(false);
        if has_published_share {
            return true;
        }
        print
            .queues
            .list()
            .await
            .map(|queues| !queues.is_empty())
            .unwrap_or(false)
    }

    pub async fn effective_web_gateway_settings(&self) -> WebGatewaySettings {
        self.effective_web_gateway_settings_for(&self.settings.snapshot())
            .await
    }

    pub async fn effective_web_gateway_settings_for(
        &self,
        app_settings: &AppSettings,
    ) -> WebGatewaySettings {
        let mut settings = app_settings.web_files.clone();
        if settings.site_name.trim().is_empty() {
            settings.site_name = format!(
                "{} 的 ArcRelay",
                effective_device_name(&app_settings.device_name)
            );
        }
        settings
            .allowed_hostnames
            .push(web_hostname(&self.local_device_id));
        settings
    }

    pub async fn web_gateway_requires_background(&self) -> bool {
        self.web_gateway.requires_background().await
    }

    pub async fn ensure_print_network(&self) -> Result<(), String> {
        let _activation = self.print_activation.lock().await;
        // Snapshot the option in its own scope. Keeping the read-guard
        // temporary as the `match` scrutinee would retain it through the
        // `None` arm and deadlock when that arm acquires the write lock.
        let initialized = clone_initialized_service(&self.print_network).await;
        let network = match initialized {
            Some(network) => network,
            None => {
                let runtime = self
                    .network
                    .get()
                    .cloned()
                    .ok_or_else(|| "unified network service is not running".to_string())?;
                let network = arcrelay_print::infrastructure::quic::PrintQuicService::start(
                    self.print().await?,
                    runtime,
                )
                .await
                .map_err(|error| error.to_string())?;
                *self.print_network.write().await = Some(network.clone());
                network
            }
        };

        let ipp_initialized = clone_initialized_service(&self.print_ipp).await;
        if ipp_initialized.is_none() {
            let config_dir = dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("ArcRelay");
            let bridge = arcrelay_print::infrastructure::ipp::IppBridge::start(
                network,
                config_dir.join("print-ipp-jobs.json"),
            )
            .await
            .map_err(|error| error.to_string())?;
            *self.print_ipp.write().await = Some(bridge);
        }
        Ok(())
    }

    pub fn emit_runtime_snapshot(&self, app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let mut revision = lock_unpoison(&self.snapshot_revision);
            *revision += 1;
            let _ = window.emit("desktop-runtime", self.runtime_snapshot(*revision));
        }
    }

    pub fn emit_snapshot(&self, app: &AppHandle) {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.emit("desktop-state", self.snapshot());
        }
    }

    pub fn set_pending_pairing(&self, request: PairingRequestForApproval, app: &AppHandle) {
        let view = PendingPairingView {
            device_name: request.device_name.clone(),
            device_id: request.device_id.clone(),
            pairing_code: request.pairing_code.clone(),
            permissions: pairing_permission_labels(
                &request.requested_grants,
                self.settings.snapshot().language,
            ),
            grants: pairing_grant_views(
                &request.requested_grants,
                self.settings.snapshot().language,
            ),
        };
        *lock_unpoison(&self.pairing_response) = Some(PendingPairingResponse {
            device_id: request.device_id.clone(),
            respond: request.respond,
            grants: request.requested_grants,
        });
        let mut runtime = write_unpoison(&self.runtime);
        runtime.push_activity(format!(
            "received pairing request from {} (code {})",
            view.device_name, view.pairing_code
        ));
        runtime.pending_pairing = Some(view);
        drop(runtime);
        self.emit_runtime_snapshot(app);
    }

    pub fn take_pairing_response(
        &self,
        expected_device_id: Option<&str>,
        accepted: bool,
        approved_grant_ids: Option<&[String]>,
    ) -> Option<(mpsc::Sender<PairingApproval>, PairingApproval)> {
        let mut pending = lock_unpoison(&self.pairing_response);
        if expected_device_id.is_some_and(|expected| {
            pending
                .as_ref()
                .is_some_and(|current| current.device_id != expected)
        }) {
            return None;
        }
        pending.take().map(|current| {
            let selected = approved_grant_ids.map(|ids| ids.iter().collect::<HashSet<_>>());
            let approved_grants = if accepted {
                current
                    .grants
                    .iter()
                    .filter(|grant| {
                        selected
                            .as_ref()
                            .is_none_or(|selected| selected.contains(&pairing_grant_id(grant)))
                    })
                    .map(|grant| (grant.capability, grant.direction))
                    .collect()
            } else {
                Default::default()
            };
            (
                current.respond,
                PairingApproval {
                    accepted,
                    approved_grants,
                },
            )
        })
    }

    pub fn is_pairing_pending_for(&self, device_id: &str) -> bool {
        lock_unpoison(&self.pairing_response)
            .as_ref()
            .is_some_and(|pending| pending.device_id == device_id)
    }

    pub fn finish_pairing(&self, accepted: bool, app: &AppHandle) {
        let mut runtime = write_unpoison(&self.runtime);
        if let Some(pairing) = runtime.pending_pairing.take() {
            runtime.push_activity(format!(
                "{} pairing request from {}",
                if accepted { "accepted" } else { "rejected" },
                pairing.device_name
            ));
        }
        drop(runtime);
        self.emit_runtime_snapshot(app);
    }

    fn apply_server_event(&self, event: ServerEvent, app: &AppHandle) {
        log_server_event(&event);
        let metrics_only = matches!(&event, ServerEvent::InputMetrics { .. });
        let permission_required = matches!(&event, ServerEvent::InputPermissionRequired { .. });
        let mut runtime = write_unpoison(&self.runtime);
        match event {
            ServerEvent::PairingRequest {
                device_name,
                pairing_code,
                ..
            } => runtime.push_activity(format!(
                "device {device_name} requested pairing (code {pairing_code})"
            )),
            ServerEvent::PairingResult {
                device_name,
                accepted,
            } => runtime.push_activity(format!(
                "device {device_name} pairing {}",
                if accepted {
                    "succeeded"
                } else {
                    "was rejected"
                }
            )),
            ServerEvent::PairingCancelled {
                device_id,
                device_name,
            } => {
                if runtime
                    .pending_pairing
                    .as_ref()
                    .is_some_and(|pairing| pairing.device_id == device_id)
                {
                    runtime.pending_pairing = None;
                    lock_unpoison(&self.pairing_response).take();
                }
                runtime.push_activity(format!(
                    "pairing with device {device_name} was cancelled or timed out"
                ));
            }
            ServerEvent::OutgoingPairingCode {
                device_id,
                device_name,
                pairing_code,
            } => {
                if let Some(pairing) = runtime
                    .outgoing_pairings
                    .iter_mut()
                    .find(|pairing| pairing.device_id == device_id)
                {
                    pairing.device_name = device_name.clone();
                    pairing.pairing_code = pairing_code.clone();
                } else {
                    runtime.outgoing_pairings.push(OutgoingPairingView {
                        device_name: device_name.clone(),
                        device_id,
                        pairing_code: pairing_code.clone(),
                    });
                }
                runtime.push_activity(format!(
                    "pairing with {device_name} (verify code {pairing_code})"
                ));
            }
            ServerEvent::OutgoingPairingFinished { device_id } => {
                runtime
                    .outgoing_pairings
                    .retain(|pairing| pairing.device_id != device_id);
            }
            ServerEvent::DeviceConnected {
                device_id,
                device_name,
                ..
            } => {
                runtime.register_connected_device(device_id, device_name.clone());
                runtime.push_activity(format!("device {device_name} connected"));
            }
            ServerEvent::DeviceDisconnected {
                device_id,
                device_name,
            } => {
                runtime.unregister_connected_device(&device_id);
                let still_connected = runtime.connected_device_counts.contains_key(&device_id)
                    || runtime.input_connected_device_ids.contains(&device_id);
                if !still_connected {
                    if runtime
                        .active_input_device
                        .as_ref()
                        .is_some_and(|device| device.id == device_id)
                    {
                        runtime.active_input_device = None;
                        runtime.active_input_session_id = None;
                        runtime.input_metrics = None;
                    }
                    runtime.push_activity(format!("device {device_name} disconnected"));
                }
            }
            ServerEvent::InputPermissionRequired { device_name } => {
                runtime.input_permission = InputPermissionState::Denied;
                runtime.push_activity(format!(
                    "device {device_name} requested remote input; accessibility permission is required"
                ));
            }
            ServerEvent::InputSessionStarted {
                session_id,
                device_id,
                device_name,
            } => {
                runtime.active_input_session_id = Some(session_id);
                runtime.active_input_device = Some(ConnectedDeviceView {
                    id: device_id,
                    name: device_name.clone(),
                    auto_connect: true,
                });
                runtime.input_metrics = None;
                runtime.push_activity(format!("device {device_name} started remote input"));
            }
            ServerEvent::InputSessionEnded {
                session_id,
                device_name,
                ..
            } => {
                if runtime.active_input_session_id.as_deref() == Some(session_id.as_str()) {
                    runtime.active_input_session_id = None;
                    runtime.active_input_device = None;
                    runtime.input_metrics = None;
                }
                runtime.push_activity(format!("device {device_name} ended remote input"));
            }
            ServerEvent::InputMetrics {
                session_id,
                receive_hz,
                quartz_hz,
                average_gap_us,
                maximum_gap_us,
                maximum_source_gap_us,
                maximum_transport_stall_us,
                average_apply_us,
                maximum_apply_us,
            } => {
                if runtime.active_input_session_id.as_deref() == Some(session_id.as_str()) {
                    runtime.input_metrics = Some(InputMetricsView {
                        receive_hz,
                        quartz_hz,
                        average_gap_us,
                        maximum_gap_us,
                        maximum_source_gap_us,
                        maximum_transport_stall_us,
                        average_apply_us,
                        maximum_apply_us,
                    });
                }
            }
        }
        drop(runtime);
        if permission_required {
            if let Err(error) = crate::windowing::ensure_permission_guide_window(app) {
                tracing::warn!(%error, "Failed to show permission guide window");
            }
        }
        if metrics_only {
            if let Some(window) = app.get_webview_window("main") {
                let metrics = read_unpoison(&self.runtime).input_metrics.clone();
                let _ = window.emit("input-metrics", metrics);
            }
        } else {
            self.emit_runtime_snapshot(app);
        }
    }

    pub fn set_input_connected_devices(&self, devices: Vec<ConnectedDeviceView>, app: &AppHandle) {
        let changed = write_unpoison(&self.runtime).sync_input_connected_devices(devices);
        if changed {
            self.emit_runtime_snapshot(app);
        }
    }

    pub fn set_paired_devices(&self, devices: Vec<ConnectedDeviceView>, app: &AppHandle) {
        write_unpoison(&self.runtime).paired_devices = devices;
        self.emit_runtime_snapshot(app);
    }

    pub fn set_input_permission(&self, permission: InputPermissionState, app: &AppHandle) {
        let previous = {
            let mut runtime = write_unpoison(&self.runtime);
            let previous = runtime.input_permission;
            runtime.input_permission = permission;
            previous
        };
        if previous != permission {
            tracing::info!(
                event = "remote_input.permission_changed",
                previous = ?previous,
                current = ?permission,
                "remote input permission changed"
            );
        }
        self.emit_runtime_snapshot(app);
    }

    pub fn request_input_permission_refresh(&self) {
        let _ = self
            .command_tx
            .try_send(BackendCommand::RefreshInputPermission(None));
    }

    fn set_server_running(&self, running: bool, app: &AppHandle) {
        let mut runtime = write_unpoison(&self.runtime);
        runtime.server_running = running;
        if !running {
            runtime.push_activity("background service stopped");
        }
        drop(runtime);
        self.emit_runtime_snapshot(app);
    }

    pub async fn shutdown_and_exit(&self, app: AppHandle) {
        if self.quitting.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::task::spawn_blocking(crate::sound::shutdown),
        )
        .await;
        let (done_tx, done_rx) = oneshot::channel();
        let _ = self
            .command_tx
            .send(BackendCommand::Shutdown(done_tx))
            .await;
        if tokio::time::timeout(std::time::Duration::from_secs(10), done_rx)
            .await
            .is_err()
        {
            tracing::warn!("application shutdown exceeded its drain deadline");
        }
        app.exit(0);
    }
}

async fn clone_initialized_service<T: Clone>(slot: &tokio::sync::RwLock<Option<T>>) -> Option<T> {
    let initialized = slot.read().await;
    initialized.clone()
}

fn log_server_event(event: &ServerEvent) {
    match event {
        ServerEvent::PairingRequest { device_id, .. } => tracing::info!(
            event = "pairing.request_received",
            peer_id = %crate::observability::pseudonym(device_id),
            "pairing request received"
        ),
        ServerEvent::PairingResult { accepted, .. } => {
            tracing::info!(event = "pairing.completed", accepted, "pairing completed")
        }
        ServerEvent::PairingCancelled { device_id, .. } => tracing::warn!(
            event = "pairing.cancelled",
            peer_id = %crate::observability::pseudonym(device_id),
            "pairing cancelled or timed out"
        ),
        ServerEvent::OutgoingPairingCode { device_id, .. } => tracing::info!(
            event = "pairing.outgoing_verification_ready",
            peer_id = %crate::observability::pseudonym(device_id),
            "outgoing pairing verification is ready"
        ),
        ServerEvent::OutgoingPairingFinished { device_id } => tracing::info!(
            event = "pairing.outgoing_finished",
            peer_id = %crate::observability::pseudonym(device_id),
            "outgoing pairing finished"
        ),
        ServerEvent::DeviceConnected { device_id, .. } => tracing::info!(
            event = "connection.device_connected",
            peer_id = %crate::observability::pseudonym(device_id),
            "device connected"
        ),
        ServerEvent::DeviceDisconnected { device_id, .. } => tracing::warn!(
            event = "connection.device_disconnected",
            peer_id = %crate::observability::pseudonym(device_id),
            "device disconnected"
        ),
        ServerEvent::InputPermissionRequired { .. } => tracing::warn!(
            event = "remote_input.permission_required",
            "remote input permission is required"
        ),
        ServerEvent::InputSessionStarted {
            session_id,
            device_id,
            ..
        } => tracing::info!(
            event = "remote_input.session_started",
            session_id,
            peer_id = %crate::observability::pseudonym(device_id),
            "remote input session started"
        ),
        ServerEvent::InputSessionEnded {
            session_id,
            device_id,
            ..
        } => tracing::info!(
            event = "remote_input.session_ended",
            session_id,
            peer_id = %crate::observability::pseudonym(device_id),
            "remote input session ended"
        ),
        ServerEvent::InputMetrics {
            session_id,
            receive_hz,
            quartz_hz,
            average_gap_us,
            maximum_gap_us,
            maximum_source_gap_us,
            maximum_transport_stall_us,
            average_apply_us,
            maximum_apply_us,
        } => tracing::info!(
            event = "remote_input.pipeline.summary",
            session_id,
            receive_hz,
            quartz_hz,
            average_gap_us,
            maximum_gap_us,
            maximum_source_gap_us,
            maximum_transport_stall_us,
            average_apply_us,
            maximum_apply_us,
            "remote input pipeline summary"
        ),
    }
}

fn pairing_permission_labels(
    grants: &[Grant],
    language: crate::settings::LanguagePreference,
) -> Vec<String> {
    grants
        .iter()
        .map(|grant| pairing_grant_label(grant, language))
        .collect()
}

fn pairing_grant_views(
    grants: &[Grant],
    language: crate::settings::LanguagePreference,
) -> Vec<PendingPairingGrantView> {
    grants
        .iter()
        .map(|grant| PendingPairingGrantView {
            id: pairing_grant_id(grant),
            capability: grant.capability.token().to_string(),
            direction: match grant.direction {
                GrantDirection::Inbound => "inbound",
                GrantDirection::Outbound => "outbound",
            }
            .to_string(),
            label: pairing_grant_label(grant, language),
        })
        .collect()
}

fn pairing_grant_id(grant: &Grant) -> String {
    format!(
        "{}:{}",
        grant.capability.token(),
        match grant.direction {
            GrantDirection::Inbound => "inbound",
            GrantDirection::Outbound => "outbound",
        }
    )
}

fn pairing_grant_label(grant: &Grant, language: crate::settings::LanguagePreference) -> String {
    let capability = capability_label(grant.capability, language);
    let direction = match (grant.direction, language.uses_english_fallback()) {
        (GrantDirection::Inbound, true) => "requested device → this device",
        (GrantDirection::Outbound, true) => "this device → requested device",
        (GrantDirection::Inbound, false) => "对方可操作本机",
        (GrantDirection::Outbound, false) => "本机可操作对方",
    };
    let constraints = match &grant.constraints {
        GrantConstraints::None => String::new(),
        GrantConstraints::RemoteFileShares {
            share_ids,
            writable,
        } => {
            let mode = if language.uses_english_fallback() {
                if *writable {
                    "read/write"
                } else {
                    "read-only"
                }
            } else if *writable {
                "读写"
            } else {
                "只读"
            };
            format!("；{mode}: {}", share_ids.join(", "))
        }
    };
    format!("{capability}（{direction}{constraints}）")
}

fn capability_label(
    capability: CapabilityId,
    language: crate::settings::LanguagePreference,
) -> String {
    let english = language.uses_english_fallback();
    match capability {
        CapabilityId::SystemRead => {
            if english {
                "Read system status"
            } else {
                "读取系统状态"
            }
        }
        CapabilityId::ProcessRead => {
            if english {
                "Read process list"
            } else {
                "读取进程列表"
            }
        }
        CapabilityId::ProcessManage => {
            if english {
                "Manage processes"
            } else {
                "管理进程"
            }
        }
        CapabilityId::MediaRead => {
            if english {
                "Read media status"
            } else {
                "读取媒体状态"
            }
        }
        CapabilityId::MediaControl => {
            if english {
                "Control media and volume"
            } else {
                "控制媒体与音量"
            }
        }
        CapabilityId::ClipboardRead => {
            if english {
                "Read clipboard"
            } else {
                "读取剪贴板"
            }
        }
        CapabilityId::ClipboardWrite => {
            if english {
                "Write clipboard"
            } else {
                "写入剪贴板"
            }
        }
        CapabilityId::ClipboardSync => {
            if english {
                "Synchronize clipboard history"
            } else {
                "同步剪贴板历史"
            }
        }
        CapabilityId::WindowRead => {
            if english {
                "Read window list"
            } else {
                "读取窗口列表"
            }
        }
        CapabilityId::WindowControl => {
            if english {
                "Control windows and desktops"
            } else {
                "控制窗口与桌面"
            }
        }
        CapabilityId::ActionRead => {
            if english {
                "Read quick actions"
            } else {
                "读取快捷动作"
            }
        }
        CapabilityId::ActionExecute => {
            if english {
                "Run quick actions"
            } else {
                "执行快捷动作"
            }
        }
        CapabilityId::NotificationRead => {
            if english {
                "Read notifications"
            } else {
                "读取通知"
            }
        }
        CapabilityId::NotificationAcknowledge => {
            if english {
                "Acknowledge notifications"
            } else {
                "反馈通知已读"
            }
        }
        CapabilityId::RemoteInputInject => {
            if english {
                "Remote keyboard and pointer control"
            } else {
                "远程键盘鼠标控制"
            }
        }
        CapabilityId::CrossScreenInject => {
            if english {
                "Inject cross-screen input"
            } else {
                "注入跨屏输入"
            }
        }
        CapabilityId::RemoteFilesRead => {
            if english {
                "Read shared remote files"
            } else {
                "读取远程共享文件"
            }
        }
        CapabilityId::RemoteFilesWrite => {
            if english {
                "Modify shared remote files"
            } else {
                "修改远程共享文件"
            }
        }
        CapabilityId::NearbyTransferSend => {
            if english {
                "Send nearby files"
            } else {
                "发送附近文件"
            }
        }
        CapabilityId::PrintSubmit => {
            if english {
                "Submit print jobs"
            } else {
                "提交打印任务"
            }
        }
    }
    .to_string()
}

#[cfg(test)]
mod tests;
