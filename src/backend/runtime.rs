use super::*;

pub(super) async fn run_backend(
    prepared: PreparedBackend,
    app: AppHandle,
    input_sessions: oneshot::Sender<broadcast::Receiver<Arc<arcrelay_network::Session>>>,
    workspace_input_router: Arc<dyn arcrelay_protocol::server::WorkspaceInputRouter>,
) {
    let PreparedBackend {
        state,
        service,
        socket,
        event_tx,
        mut event_rx,
        pairing_tx,
        mut pairing_rx,
        mut command_rx,
        action_provider,
        mcp_server,
    } = prepared;

    crate::sound::initialize(state.settings.snapshot().sounds, state.privacy.clone());

    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("ArcRelay");
    let repository = match SqlitePeerRepository::open(&config_dir.join("peers.sqlite3")).await {
        Ok(repository) => Arc::new(repository),
        Err(error) => {
            tracing::error!(
                event = "network.peer_repository_open_failed",
                %error,
                "failed to open unified peer repository"
            );
            state.set_server_running(false, &app);
            return;
        }
    };
    let peer_repository: Arc<dyn PeerRepository> = repository.clone();
    let device_name = state.server_name.read().await.clone();
    let mut network_config = NetworkRuntimeConfig::new(
        config_dir.join("identity"),
        DeviceMetadata {
            name: device_name,
            platform: std::env::consts::OS.into(),
            model: hostname::get()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        },
        peer_repository,
    );
    network_config.endpoint_repository = repository;
    let network = match NetworkRuntime::bind_on_socket(network_config, socket).await {
        Ok(network) => network,
        Err(error) => {
            tracing::error!(
                event = "network.runtime_start_failed",
                %error,
                "failed to start unified LAN runtime"
            );
            state.set_server_running(false, &app);
            return;
        }
    };
    // Subscribe every eager feature service before publishing the shared
    // runtime. These receivers buffer sessions that arrive while the rest of
    // backend startup is still running. Without the Arc Input reservation, a
    // fast reconnect can be observed by the control subscriber only and leave
    // the input workspace stuck in `Connecting` until a manual reconnect.
    let control_incoming = network.subscribe();
    let input_incoming = network.subscribe();
    if input_sessions.send(input_incoming).is_err() {
        tracing::warn!(
            event = "input.connection.receiver_unavailable",
            "Arc Input startup receiver was dropped before network initialization"
        );
    }
    let _ = state.network.set(network.clone());
    let web_gateway_settings = state.effective_web_gateway_settings().await;
    match state.web_gateway.reconfigure(web_gateway_settings).await {
        Ok(status) if status.running => {
            if let Err(error) = network
                .discovery()
                .publish_web_gateway(&status.site_name, status.port)
            {
                tracing::warn!(event = "web.discovery_publish_failed", %error);
            }
        }
        Ok(_) => network.discovery().unpublish_web_gateway(),
        Err(error) => {
            network.discovery().unpublish_web_gateway();
            tracing::error!(event = "web.server.start_failed", %error, "failed to start Web Gateway");
        }
    }
    if state.web_gateway_requires_background().await {
        tracing::info!(
            event = "web.background_required",
            "Web Gateway keeps ArcRelay available in the tray"
        );
    }
    let mut web_status_events = state.web_gateway.subscribe_status();
    let web_status_app = app.clone();
    tokio::spawn(async move {
        while web_status_events.changed().await.is_ok() {
            let status = web_status_events.borrow().clone();
            let _ = web_status_app.emit("web-gateway-status", status);
        }
    });
    info!(
        event = "network.endpoint_started",
        device_id = %crate::observability::pseudonym(network.device_id().as_str()),
        port = state.port,
        "unified ArcRelay v1 endpoint started"
    );

    // Control is the shared capability plane for clipboard sync and remote
    // files. Start it immediately after the endpoint is bound, before any
    // feature-specific recovery can block the backend initialization path.
    let coordinator = StateCoordinator::new(service.clone());
    coordinator.start();
    let control_service =
        Arc::new(DesktopControlService::new().with_workspace_input_router(workspace_input_router));
    let registry = control_service.connection_registry();
    state
        .clipboard_sync
        .attach_connection_registry(registry.clone())
        .await;
    let control_state = state.clone();
    let control_app = app.clone();
    let control_task = tokio::spawn(control_service.start_with_incoming(
        network.clone(),
        control_incoming,
        coordinator.clone(),
        event_tx.clone(),
        pairing_tx.clone(),
        Some(action_provider.clone()),
        Some(state.remote_files.clone()),
    ));
    state.clipboard_sync.start();
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    match crate::system_folders::SystemFolders::start(
        state.clipboard_sync.clone(),
        config_dir.join("system-folders"),
    )
    .await
    {
        Ok(folders) => {
            let _ = state.system_folders.set(folders);
        }
        Err(error) => tracing::warn!(%error, "native system folders could not start"),
    }

    let print_notification_state = state.clone();
    let print_notification_app = app.clone();
    tokio::spawn(async move {
        let mut ready = print_notification_state.modules.subscribe();
        let print = loop {
            if let Some(print) = print_notification_state.modules.initialized_print() {
                break print;
            }
            if ready.changed().await.is_err() {
                return;
            }
            // Readiness publication can precede OnceCell's return by one yield.
            tokio::task::yield_now().await;
        };
        let mut print_events = print.jobs.subscribe();
        let mut notified_states = HashMap::new();
        loop {
            let event = match print_events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            let arcrelay_print::PrintDomainEvent::JobStateChanged { job_id, state } = event else {
                continue;
            };
            let category = match state {
                arcrelay_print::PrintJobState::Completed => {
                    crate::desktop_notification::DesktopNotificationCategory::PrintCompleted
                }
                arcrelay_print::PrintJobState::Failed
                | arcrelay_print::PrintJobState::Held
                | arcrelay_print::PrintJobState::Ambiguous => {
                    crate::desktop_notification::DesktopNotificationCategory::PrintFailed
                }
                _ => continue,
            };
            if notified_states.insert(job_id.clone(), state) == Some(state) {
                continue;
            }
            let jobs = match print.jobs.list_recent(100, false).await {
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(%error, "failed to load print job for notification");
                    continue;
                }
            };
            let Some(job) = jobs.into_iter().find(|job| job.id == job_id) else {
                continue;
            };
            let language = print_notification_state.settings.snapshot().language;
            let completed = state == arcrelay_print::PrintJobState::Completed;
            let notification = crate::desktop_notification::DesktopNotification::new(
                category,
                crate::desktop_notification::localized(
                    language,
                    if completed {
                        "打印任务已完成"
                    } else {
                        "打印任务需要处理"
                    },
                    if completed {
                        "Print job completed"
                    } else {
                        "Print job needs attention"
                    },
                ),
                if completed {
                    job.document.name
                } else {
                    format!(
                        "{}: {}",
                        job.document.name,
                        job.failure_message.unwrap_or_else(|| {
                            crate::desktop_notification::localized(
                                language,
                                "请检查系统打印队列。",
                                "Check the system print queue.",
                            )
                        })
                    )
                },
            );
            if let Err(error) = crate::desktop_notification::show(
                &print_notification_app,
                &print_notification_state.settings,
                notification,
            ) {
                tracing::warn!(%error, "failed to show print notification");
            }
        }
    });
    let print_state = state.clone();
    tokio::spawn(async move {
        if print_state.print_requires_background().await {
            match tokio::time::timeout(
                std::time::Duration::from_secs(20),
                print_state.ensure_print_network(),
            )
            .await
            {
                Ok(Ok(())) => tracing::info!(
                    event = "print.network_ready",
                    "unified printer service is ready"
                ),
                Ok(Err(error)) => {
                    tracing::error!(%error, "Failed to start unified printer service")
                }
                Err(_) => tracing::error!(
                    event = "print.network_start_timeout",
                    "timed out while starting unified printer service"
                ),
            }
        }
    });

    state.privacy.start(app.clone());
    let mut module_changes = state.modules.subscribe();
    let module_app = app.clone();
    tokio::spawn(async move {
        while module_changes.changed().await.is_ok() {
            let statuses = module_changes.borrow_and_update().clone();
            let _ = module_app.emit("runtime-modules", statuses);
        }
    });
    let transfer_owner = state.clone();
    let transfer_network = network.clone();
    let transfer_app = app.clone();
    tokio::spawn(async move {
        let transfer_state = loop {
            match transfer_owner.transfer().await {
                Ok(transfer) => break transfer,
                Err(error) => {
                    tracing::warn!(%error, "nearby transfer unavailable; retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            }
        };
        let started = loop {
            match transfer_state.start(transfer_network.clone()).await {
                Ok(()) => {
                    transfer_owner.modules.status(
                        "transfer",
                        crate::application::runtime_modules::ModuleState::Ready,
                        None,
                    );
                    break Ok::<(), arcrelay_transfer::TransferError>(());
                }
                Err(error) => {
                    transfer_owner.modules.status(
                        "transfer",
                        crate::application::runtime_modules::ModuleState::Unavailable,
                        Some(error.to_string()),
                    );
                    tracing::warn!(%error, "transfer startup failed; retrying");
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                }
            }
        };
        match started {
            Ok(()) => {
                let mut progress = transfer_state.subscribe_progress();
                let progress_app = transfer_app.clone();
                let progress_state = transfer_state.clone();
                tokio::spawn(async move {
                    loop {
                        match progress.recv().await {
                            Ok(progress) => {
                                let _ = progress_app.emit("transfer-progress", progress);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                let _ = progress_app
                                    .emit("transfer-state", progress_state.snapshot().await);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
                let mut receiver = transfer_state.subscribe();
                let mut transfer_errors = transfer_state.subscribe_errors();
                let initial_snapshot = transfer_state.snapshot().await;
                let mut notification_tracker =
                    TransferNotificationTracker::from_snapshot(&initial_snapshot);
                let mut workflow_tracker =
                    AutomationTransferEventTracker::from_snapshot(&initial_snapshot);
                let transfer_automations = transfer_owner.automations.clone();
                let transfer_settings = transfer_owner.settings.clone();
                let system_share = transfer_app
                    .state::<Arc<crate::system_share::SystemShareService>>()
                    .inner()
                    .clone();
                if let Err(error) = system_share.update_peer_cache(&initial_snapshot.peers) {
                    tracing::warn!(%error, "could not publish system share peer cache");
                }
                match system_share.observe_transfers(&initial_snapshot) {
                    Ok(true) => crate::system_share::announce_pending(&transfer_app, &system_share),
                    Ok(false) => {}
                    Err(error) => {
                        tracing::warn!(%error, "could not reconcile system share requests")
                    }
                }
                // The menu-bar sender remains subscribed even when main is closed.
                let _ = transfer_app.emit("transfer-state", initial_snapshot);
                tokio::spawn(async move {
                    while receiver.changed().await.is_ok() {
                        let snapshot = receiver.borrow_and_update().as_ref().clone();
                        if let Err(error) = system_share.update_peer_cache(&snapshot.peers) {
                            tracing::warn!(%error, "could not publish system share peer cache");
                        }
                        match system_share.observe_transfers(&snapshot) {
                            Ok(true) => {
                                crate::system_share::announce_pending(&transfer_app, &system_share)
                            }
                            Ok(false) => {}
                            Err(error) => {
                                tracing::warn!(%error, "could not reconcile system share requests")
                            }
                        }
                        let language = transfer_settings.snapshot().language;
                        for notification in notification_tracker.update(&snapshot, language) {
                            show_transfer_system_notification(
                                &transfer_app,
                                &transfer_settings,
                                notification,
                            );
                        }
                        for event in workflow_tracker.update(&snapshot) {
                            transfer_automations.relay_event(&event).await;
                        }
                        let _ = transfer_app.emit("transfer-state", snapshot);
                    }
                });
                tokio::spawn(async move {
                    while let Ok(message) = transfer_errors.recv().await {
                        tracing::warn!(%message, "nearby transfer service reported a warning");
                    }
                });
            }
            Err(error) => tracing::error!(%error, "failed to start nearby transfer service"),
        }
    });
    let mut notification_changes = state.notification_store.subscribe();
    let notification_state = state.clone();
    let notification_app = app.clone();
    let initial_host_notifications = notification_state
        .notification_store
        .list(true, 500)
        .unwrap_or_default();
    let mut host_notification_tracker =
        crate::desktop_notification::HostNotificationTracker::from_notifications(
            &initial_host_notifications,
        );
    tokio::spawn(async move {
        while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
            notification_changes.recv().await
        {
            let notifications = notification_state
                .notification_store
                .list(true, 500)
                .unwrap_or_default();
            for notification in host_notification_tracker.update(&notifications) {
                let mut desktop_notification =
                    crate::desktop_notification::DesktopNotification::new(
                        crate::desktop_notification::DesktopNotificationCategory::AgentNotification,
                        notification.title,
                        if notification.body.is_empty() {
                            notification.source
                        } else {
                            notification.body
                        },
                    );
                if notification.kind == crate::notification::NotificationKind::ActionRequired {
                    desktop_notification = desktop_notification.error();
                }
                if let Err(error) = crate::desktop_notification::show(
                    &notification_app,
                    &notification_state.settings,
                    desktop_notification,
                ) {
                    tracing::warn!(%error, "failed to show Agent notification");
                }
            }
            if let Some(window) = notification_app.get_webview_window("main") {
                let count = notification_state
                    .notification_store
                    .unread_count()
                    .unwrap_or_default();
                let _ = window.emit("notification-count", count);
                let _ = window.emit("notifications-changed", ());
            }
        }
    });

    if let Some(mut captures) = service.clipboard.subscribe_captures() {
        tokio::spawn(async move {
            loop {
                match captures.recv().await {
                    Ok(capture)
                        if capture.occurred_at.elapsed()
                            <= std::time::Duration::from_millis(500) =>
                    {
                        use arcrelay_core::domain::clipboard::ClipboardCaptureOrigin;
                        crate::sound::play(match capture.origin {
                            ClipboardCaptureOrigin::Local => {
                                crate::sound::SoundEvent::ClipboardAdded
                            }
                            ClipboardCaptureOrigin::Remote => {
                                crate::sound::SoundEvent::ClipboardReceived
                            }
                        });
                    }
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    if let Some(mut clipboard_changes) = service.clipboard.subscribe_changes() {
        let clipboard_app = app.clone();
        tokio::spawn(async move {
            while let Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) =
                clipboard_changes.recv().await
            {
                if let Some(window) =
                    clipboard_app.get_webview_window(crate::windowing::CLIPBOARD_WINDOW_LABEL)
                {
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.emit("clipboard-changed", ());
                    }
                }
            }
        });
    }
    if let Some(mut clipboard_ocr_changes) = service.clipboard.subscribe_ocr_changes() {
        let clipboard_app = app.clone();
        tokio::spawn(async move {
            loop {
                match clipboard_ocr_changes.recv().await {
                    Ok(id) => {
                        if let Some(window) = clipboard_app
                            .get_webview_window(crate::windowing::CLIPBOARD_WINDOW_LABEL)
                        {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.emit("clipboard-ocr-changed", id);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
    state.automations.start(app.clone()).await;

    state.mcp_running.store(true, Ordering::SeqCst);
    state.emit_runtime_snapshot(&app);
    let mcp_state = state.clone();
    let mcp_app = app.clone();
    tokio::spawn(async move {
        if let Err(error) = mcp_server.serve(mcp_app.clone(), mcp_state.clone()).await {
            tracing::error!(%error, "MCP server error");
        }
        mcp_state.mcp_running.store(false, Ordering::SeqCst);
        mcp_state.emit_runtime_snapshot(&mcp_app);
    });

    state.set_paired_devices(paired_device_views(&network).await, &app);
    let input_permission = service.input_control.permission_state();
    state.set_input_permission(input_permission, &app);

    let event_state = state.clone();
    let event_app = app.clone();
    let event_network = network.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let pairing_completed =
                matches!(&event, ServerEvent::PairingResult { accepted: true, .. });
            let device_id = server_event_device_id(&event);
            let was_connected = device_id
                .as_deref()
                .is_some_and(|device_id| event_state.is_device_connected(device_id));
            event_state.apply_server_event(event.clone(), &event_app);
            let is_connected = device_id
                .as_deref()
                .is_some_and(|device_id| event_state.is_device_connected(device_id));
            if let Some(notification) = server_desktop_notification(
                &event,
                was_connected,
                is_connected,
                event_state.settings.snapshot().language,
            ) {
                if let Err(error) = crate::desktop_notification::show(
                    &event_app,
                    &event_state.settings,
                    notification,
                ) {
                    tracing::warn!(%error, "failed to show device notification");
                }
            }
            if let Some(event) = server_automation_event(&event, was_connected, is_connected) {
                event_state.automations.relay_event(&event).await;
            }
            if matches!(&event, ServerEvent::DeviceDisconnected { .. }) {
                continue;
            }
            if pairing_completed {
                event_state
                    .set_paired_devices(paired_device_views(&event_network).await, &event_app);
            }
        }
    });

    let pairing_state = state.clone();
    let pairing_app = app.clone();
    tokio::spawn(async move {
        while let Some(request) = pairing_rx.recv().await {
            let device_id = request.device_id.clone();
            let device_name = request.device_name.clone();
            let pairing_code = request.pairing_code.clone();
            let permissions = pairing_permission_labels(
                &request.requested_grants,
                pairing_state.settings.snapshot().language,
            );
            pairing_state.set_pending_pairing(request, &pairing_app);

            // A minimized, hidden, or background window is not a visible
            // approval surface. Use the native prompt unless the main window
            // is actually active so pairing cannot silently wait for timeout.
            if should_use_native_pairing_prompt(crate::windowing::main_window_active()) {
                let accepted = crate::pairing_prompt::request_pairing_approval(
                    &device_name,
                    &pairing_code,
                    &permissions,
                    pairing_state.settings.snapshot().language,
                )
                .await;
                if let Some((sender, decision)) =
                    pairing_state.take_pairing_response(Some(&device_id), accepted, None)
                {
                    if sender.send(decision).await.is_ok() {
                        pairing_state.finish_pairing(accepted, &pairing_app);
                    }
                }
            } else {
                while pairing_state.is_pairing_pending_for(&device_id) {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
        }
    });

    let output_state = state.clone();
    let output_app = app.clone();
    tokio::spawn(async move {
        let mut receiver = output_state.output_manager.subscribe();
        loop {
            let mut changed = HashSet::new();
            let mut lagged = false;
            match receiver.recv().await {
                Ok(line) => {
                    changed.insert(line.action_id);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => lagged = true,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            loop {
                match receiver.try_recv() {
                    Ok(line) => {
                        changed.insert(line.action_id);
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => lagged = true,
                    _ => break,
                }
            }
            if let Some(window) = output_app.get_webview_window("main") {
                let snapshots = if lagged {
                    output_state.output_manager.snapshots()
                } else {
                    changed
                        .iter()
                        .map(|id| output_state.output_manager.snapshot(id))
                        .collect()
                };
                for snapshot in snapshots {
                    let _ = window.emit("action-output", snapshot);
                }
            }
        }
    });

    let command_service = service.clone();
    let command_registry = registry.clone();
    let command_network = network.clone();
    let command_state = state.clone();
    let command_app = app.clone();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            handle_backend_command(
                &command_service,
                &command_network,
                &command_registry,
                &command_state,
                &command_app,
                command,
            )
            .await;
        }
    });

    match control_task.await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::error!(%error, "Server error");
            control_state.set_server_running(false, &control_app);
        }
        Err(error) => {
            tracing::error!(%error, "Control service task failed");
            control_state.set_server_running(false, &control_app);
        }
    }
}

fn server_desktop_notification(
    event: &ServerEvent,
    was_connected: bool,
    is_connected: bool,
    language: crate::settings::LanguagePreference,
) -> Option<crate::desktop_notification::DesktopNotification> {
    use crate::desktop_notification::{
        localized, DesktopNotification, DesktopNotificationCategory,
    };

    match event {
        ServerEvent::PairingRequest {
            device_name,
            pairing_code,
            ..
        } => Some(DesktopNotification::new(
            DesktopNotificationCategory::PairingRequest,
            localized(
                language,
                &format!("{device_name} 请求配对"),
                &format!("Pairing request from {device_name}"),
            ),
            localized(
                language,
                &format!("请核对配对码 {pairing_code} 后决定是否允许。"),
                &format!("Verify code {pairing_code} before allowing the device."),
            ),
        )),
        ServerEvent::DeviceConnected { device_name, .. } if !was_connected && is_connected => {
            Some(DesktopNotification::new(
                DesktopNotificationCategory::DeviceConnection,
                localized(language, "设备已连接", "Device connected"),
                device_name,
            ))
        }
        ServerEvent::DeviceDisconnected { device_name, .. }
            if was_connected && !is_connected =>
        {
            Some(DesktopNotification::new(
                DesktopNotificationCategory::DeviceConnection,
                localized(language, "设备已断开", "Device disconnected"),
                device_name,
            ))
        }
        ServerEvent::InputPermissionRequired { device_name } => {
            Some(DesktopNotification::new(
                DesktopNotificationCategory::InputPermissionRequired,
                localized(
                    language,
                    "远程输入需要系统权限",
                    "Remote input needs system permission",
                ),
                localized(
                    language,
                    &format!("{device_name} 请求控制此电脑，请在系统设置中开启辅助功能权限。"),
                    &format!("{device_name} requested control. Enable Accessibility permission in System Settings."),
                ),
            ))
        }
        _ => None,
    }
}

fn server_event_device_id(event: &ServerEvent) -> Option<String> {
    match event {
        ServerEvent::DeviceConnected { device_id, .. }
        | ServerEvent::DeviceDisconnected { device_id, .. } => Some(device_id.clone()),
        _ => None,
    }
}

fn server_automation_event(
    event: &ServerEvent,
    was_connected: bool,
    is_connected: bool,
) -> Option<crate::domain::host_event::HostEvent> {
    match event {
        ServerEvent::DeviceConnected {
            device_id,
            device_name,
        } if !was_connected && is_connected => Some(crate::domain::host_event::HostEvent::new(
            "device.connected",
            serde_json::json!({ "deviceId": device_id, "deviceName": device_name }),
        )),
        ServerEvent::DeviceDisconnected {
            device_id,
            device_name,
        } if was_connected && !is_connected => Some(crate::domain::host_event::HostEvent::new(
            "device.disconnected",
            serde_json::json!({ "deviceId": device_id, "deviceName": device_name }),
        )),
        _ => None,
    }
}

pub(super) async fn handle_backend_command(
    service: &Arc<ArcRelayService>,
    network: &Arc<NetworkRuntime>,
    registry: &ConnectionRegistry,
    state: &DesktopState,
    app: &AppHandle,
    command: BackendCommand,
) {
    match command {
        BackendCommand::ForgetPairedDevice(device_id) => {
            if let Ok(peer_id) = DeviceId::parse(&device_id) {
                if network.forget(&peer_id).await.unwrap_or(false) {
                    registry.disconnect(&device_id).await;
                }
            }
            state.set_paired_devices(paired_device_views(network).await, app);
        }
        BackendCommand::DisconnectDevice(device_id) => {
            registry.disconnect(&device_id).await;
            if let Ok(peer_id) = DeviceId::parse(&device_id) {
                network.disconnect(&peer_id, "disconnected by user").await;
            }
        }
        BackendCommand::SetDeviceAutoConnect {
            device_id,
            enabled,
            response,
        } => {
            let result = async {
                let peer_id = DeviceId::parse(&device_id).map_err(|error| error.to_string())?;
                let updated = network
                    .set_peer_auto_connect(&peer_id, enabled)
                    .await
                    .map_err(|error| error.to_string())?;
                if !updated {
                    return Err("paired device not found".to_string());
                }
                state.set_paired_devices(paired_device_views(network).await, app);
                Ok(())
            }
            .await;
            let _ = response.send(result);
        }
        BackendCommand::RefreshInputPermission(response) => {
            let permission = service.input_control.permission_state();
            state.set_input_permission(permission, app);
            if let Some(response) = response {
                let _ = response.send(permission);
            }
        }
        BackendCommand::OpenInputPermissionSettings => {
            let _ = service.input_control.open_permission_settings();
        }
        BackendCommand::DisconnectAllDevices => {
            for peer_id in network.connected_peers().await {
                registry.disconnect(peer_id.as_str()).await;
                network.disconnect(&peer_id, "disconnected by user").await;
            }
        }
        BackendCommand::Shutdown(done) => {
            state.modules.shutdown().await;
            service.clipboard.shutdown().await;
            state.remote_file_service.shutdown();
            if let Some(folders) = state.system_folders.get() {
                folders.shutdown();
            }
            state.text_selection.clear();
            let _ = service.input_control.release_all().await;
            for peer_id in network.connected_peers().await {
                registry.disconnect(peer_id.as_str()).await;
            }
            network.shutdown("application shutdown");
            let _ = done.send(());
        }
    }
}

pub(super) async fn paired_device_views(network: &NetworkRuntime) -> Vec<ConnectedDeviceView> {
    network
        .paired_peers()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|device| ConnectedDeviceView {
            id: device.device_id.to_string(),
            name: device.display_name,
            auto_connect: device.auto_connect,
        })
        .collect()
}

pub(super) fn bind_available_socket(preferred: u16) -> std::io::Result<(u16, UdpSocket)> {
    let mut last_error = None;
    for port in preferred..preferred.saturating_add(PORT_SCAN_COUNT) {
        match arcrelay_network::reserve_udp_socket(
            std::net::IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            port,
        ) {
            Ok(socket) => return Ok((port, socket)),
            Err(error) => last_error = Some(std::io::Error::other(error)),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AddrNotAvailable,
            "no ArcRelay ports were available",
        )
    }))
}

pub(super) fn get_stable_device_name() -> String {
    #[cfg(target_os = "macos")]
    {
        let mut command = std::process::Command::new("scutil");
        command.args(["--get", "ComputerName"]);
        if let Ok(output) = crate::infrastructure::executor::output_with_timeout(
            command,
            std::time::Duration::from_secs(2),
        ) {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return normalize_device_name(name);
            }
        }
    }
    let raw = hostname::get()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    normalize_device_name(raw.strip_suffix(".local").unwrap_or(&raw).to_string())
}

pub(crate) fn effective_device_name(configured_name: &str) -> String {
    let configured_name = configured_name.trim();
    if configured_name.is_empty() {
        get_stable_device_name()
    } else {
        normalize_device_name(configured_name.to_string())
    }
}

pub(super) fn normalize_device_name(value: String) -> String {
    let value: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut value = value;
    if value.len() > 128 {
        let mut end = 128;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    if value.is_empty() {
        "ArcRelay Desktop".to_string()
    } else {
        value
    }
}

pub(super) fn notification_view(
    notification: HostNotification,
    has_connected_device: bool,
) -> NotificationView {
    let delivery_state = if notification.read_at_ms.is_some() {
        "read"
    } else if has_connected_device {
        "unread"
    } else {
        "waitingForDevice"
    };
    NotificationView {
        id: notification.id,
        title: notification.title,
        body: notification.body,
        source: notification.source,
        kind: notification.kind,
        reference: notification.reference,
        created_at_ms: notification.created_at_ms,
        read_at_ms: notification.read_at_ms,
        read_by_device_name: notification.read_by_device_name,
        delivery_state: delivery_state.to_string(),
    }
}

pub(super) fn should_use_native_pairing_prompt(main_window_active: bool) -> bool {
    !main_window_active
}

pub(super) fn lock_unpoison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

pub(super) fn read_unpoison<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|error| error.into_inner())
}

pub(super) fn write_unpoison<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|error| error.into_inner())
}

#[cfg(test)]
mod workflow_event_tests {
    use super::server_automation_event;
    use arcrelay_protocol::server::ServerEvent;

    #[test]
    fn device_events_only_emit_for_real_connection_state_transitions() {
        let connected = ServerEvent::DeviceConnected {
            device_id: "peer-1".into(),
            device_name: "Peer".into(),
        };
        assert!(server_automation_event(&connected, false, true).is_some());
        assert!(server_automation_event(&connected, true, true).is_none());

        let disconnected = ServerEvent::DeviceDisconnected {
            device_id: "peer-1".into(),
            device_name: "Peer".into(),
        };
        assert!(server_automation_event(&disconnected, true, false).is_some());
        assert!(server_automation_event(&disconnected, true, true).is_none());
    }
}
