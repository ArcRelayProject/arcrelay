use super::*;

pub struct PreparedBackend {
    pub state: DesktopState,
    pub(super) service: Arc<ArcRelayService>,
    pub(super) socket: UdpSocket,
    pub(super) event_tx: mpsc::Sender<ServerEvent>,
    pub(super) event_rx: mpsc::Receiver<ServerEvent>,
    pub(super) pairing_tx: mpsc::Sender<PairingRequestForApproval>,
    pub(super) pairing_rx: mpsc::Receiver<PairingRequestForApproval>,
    pub(super) command_rx: mpsc::Receiver<BackendCommand>,
    pub(super) action_provider: Arc<dyn arcrelay_protocol::server::HostCapabilityProvider>,
    pub(super) mcp_server: crate::mcp::PreparedMcpServer,
}

impl PreparedBackend {
    pub async fn initialize() -> std::io::Result<Self> {
        let startup_started = Instant::now();
        let (port, socket) = bind_available_socket(DEFAULT_PORT)?;
        if port != DEFAULT_PORT {
            tracing::warn!(
                preferred_port = DEFAULT_PORT,
                fallback_port = port,
                "default port unavailable; using fallback"
            );
        }

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("ArcRelay");
        std::fs::create_dir_all(&config_dir)?;
        let settings = SettingsManager::new(&config_dir).map_err(std::io::Error::other)?;
        let saved_settings = settings.snapshot();
        let device_name = effective_device_name(&saved_settings.device_name);
        let server_name = Arc::new(RwLock::new(device_name.clone()));
        let receive_directory = dirs::download_dir().unwrap_or_else(|| config_dir.clone());
        let root_identity = DeviceIdentity::load_or_create(&config_dir.join("identity"))
            .map_err(std::io::Error::other)?;
        let transfer_config = TransferConfig::new(
            config_dir.join("transfer"),
            receive_directory,
            device_name.clone(),
        );
        let print_config = arcrelay_print::application::PrintRuntimeConfig {
            database_url: format!(
                "sqlite://{}?mode=rwc",
                config_dir.join("print.sqlite3").display()
            ),
            spool_directory: config_dir.join("print-spool"),
        };
        crate::application::automations::retire_legacy::archive_legacy_database(&config_dir)?;
        let clipboard_database = config_dir.join("clipboard.sqlite3");
        let clipboard_device_id = root_identity.device_id().to_string();
        let clipboard_device_name = device_name.clone();
        let resources = Arc::new(arcrelay_content::ContentResources::default());
        let modules = Arc::new(crate::application::runtime_modules::RuntimeModules::new(
            resources.clone(),
            transfer_config,
            print_config,
            config_dir.join("print.sqlite3").is_file(),
        ));
        let service = tokio::task::spawn_blocking(move || {
            compose_native_service(
                Some(clipboard_database),
                clipboard_device_id,
                clipboard_device_name,
                resources,
            )
            .map(Arc::new)
            .map_err(std::io::Error::other)
        })
        .await
        .map_err(std::io::Error::other)??;
        let output_manager = Arc::new(OutputManager::new());
        let screenshot = Arc::new(ScreenshotService::new(
            &config_dir,
            saved_settings.enhanced_screenshot_enabled,
        ));
        let action_service = Arc::new(Mutex::new(
            ActionService::new(
                JsonStore::new(config_dir.clone()),
                output_manager.clone(),
                service.clone(),
                screenshot.clone(),
            )
            .map_err(std::io::Error::other)?,
        ));
        let notification_store = NotificationStore::from_config_dir(&config_dir);
        let privacy = PrivacyManager::new(&config_dir);
        let automations = crate::application::automations::DesktopAutomations::new(
            &config_dir,
            action_service.clone(),
            notification_store.clone(),
        )
        .map_err(std::io::Error::other)?;
        let mcp_server = crate::mcp::prepare(&config_dir, notification_store.clone())
            .map_err(std::io::Error::other)?;
        let mut clipboard_policy = service
            .clipboard
            .policy()
            .await
            .map_err(std::io::Error::other)?;
        clipboard_policy.history_enabled = saved_settings.clipboard_enabled;
        service
            .clipboard
            .update_policy(clipboard_policy)
            .await
            .map_err(std::io::Error::other)?;
        service
            .clipboard
            .update_sync_preferences(ClipboardSyncPreferences {
                enabled: saved_settings.clipboard_sync_enabled,
                update_system_clipboard: saved_settings.clipboard_sync_update_system_clipboard,
                sync_edits_and_deletes: saved_settings.clipboard_sync_edits_and_deletes,
                sync_favorites: saved_settings.clipboard_sync_favorites,
            });

        let (event_tx, event_rx) = mpsc::channel(64);
        let (pairing_tx, pairing_rx) = mpsc::channel(4);
        let (command_tx, command_rx) = mpsc::channel(32);
        let network = Arc::new(tokio::sync::OnceCell::new());
        let remote_files = RemoteFileManager::load(&config_dir)?;
        let web_gateway = arcrelay_web_gateway::WebGatewaySupervisor::new(
            remote_files.service(),
            env!("CARGO_PKG_VERSION"),
        );
        let clipboard_sync = ClipboardSyncManager::new(
            service.clipboard.clone(),
            network.clone(),
            remote_files.clone(),
            &config_dir,
            event_tx.clone(),
        );

        let state = DesktopState::new(DesktopStateInit {
            port,
            local_device_id: root_identity.device_id().to_string(),
            action_service: action_service.clone(),
            automations: automations.clone(),
            output_manager: output_manager.clone(),
            notification_store: notification_store.clone(),
            mcp_port: mcp_server.port,
            mcp_token: mcp_server.token.clone(),
            mcp_access: mcp_server.access.clone(),
            mcp_journal: crate::mcp::journal::McpJournal::new(&config_dir),
            privacy: privacy.clone(),
            modules,
            clipboard: service.clipboard.clone(),
            clipboard_sync,
            remote_files,
            web_gateway,
            settings,
            screenshot,
            server_name,
            command_tx,
            network,
        });
        let action_provider: Arc<dyn arcrelay_protocol::server::HostCapabilityProvider> =
            Arc::new(AppActionProvider::new(
                action_service,
                automations,
                output_manager,
                notification_store,
                privacy,
            ));

        info!(
            event = "backend.prepared",
            elapsed_ms = startup_started.elapsed().as_millis(),
            "prepared ArcRelay backend"
        );

        Ok(Self {
            state,
            service,
            socket,
            event_tx,
            event_rx,
            pairing_tx,
            pairing_rx,
            command_rx,
            action_provider,
            mcp_server,
        })
    }

    pub fn start(
        self,
        app: AppHandle,
        input_sessions: oneshot::Sender<broadcast::Receiver<Arc<arcrelay_network::Session>>>,
        workspace_input_router: Arc<dyn arcrelay_protocol::server::WorkspaceInputRouter>,
    ) {
        crate::app_update::start_scheduler(app.clone());
        // Preparation and long-lived services share the application runtime.
        tauri::async_runtime::spawn(run_backend(
            self,
            app,
            input_sessions,
            workspace_input_router,
        ));
    }
}
