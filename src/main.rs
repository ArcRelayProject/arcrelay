#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod action_shortcuts;
mod app_update;
mod application;
mod application_runtime;
mod arc_input;
mod autostart;
mod backend;
mod clipboard_sync;
mod commands;
mod continuous_paste_trigger;
mod desktop_notification;
mod domain;
mod gaze;
mod gesture_debug;
mod hang_watchdog;
mod infrastructure;
mod ipc;
#[cfg(test)]
mod ipc_contract;
mod localization;
mod mcp;
mod notification;
mod observability;
mod pairing_prompt;
mod privacy;
mod remote_files;
mod retry;
mod screenshot;
mod settings;
mod sound;
mod startup;
mod system_folders;
mod system_share;
mod windowing;

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
const PRIVACY_UNLOCK_SHORTCUT: &str = "CmdOrCtrl+Shift+U";
const INPUT_EMERGENCY_SHORTCUT: &str = "CmdOrCtrl+Alt+Shift+Escape";
const INPUT_RECENTER_SHORTCUT: &str = "CmdOrCtrl+Alt+Shift+M";
static APP_EVENT_LOOP_READY: AtomicBool = AtomicBool::new(false);

fn main() {
    if mcp::stdio::requested(std::env::args_os()) {
        if let Err(error) = mcp::stdio::run() {
            eprintln!("ArcRelay MCP: {error}");
            std::process::exit(1);
        }
        return;
    }
    let _observability = observability::init();
    let runtime = application_runtime::build().expect("failed to create application runtime");
    tauri::async_runtime::set(runtime.handle().clone());
    application_runtime::configure_image_workers().expect("failed to configure image workers");

    let builder = tauri::Builder::default()
        // Keep this plugin first: share activations must be forwarded before
        // any second process can initialize durable application services.
        .manage(startup::PendingActivations::default())
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(args) = app.state::<startup::PendingActivations>().push(args) {
                let service = app.state::<Arc<system_share::SystemShareService>>();
                if let Err(error) =
                    service.handle_activation_args(args.into_iter().map(std::ffi::OsString::from))
                {
                    tracing::warn!(%error, "Could not import forwarded system share request");
                    return;
                }
                system_share::announce_pending(app, service.inner());
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin({
            let builder = tauri_plugin_autostart::Builder::new().app_name("ArcRelay");
            #[cfg(target_os = "macos")]
            let builder =
                builder.macos_launcher(tauri_plugin_autostart::MacosLauncher::AppleScript);
            builder.build()
        })
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_update::AppUpdateState::default())
        .manage(Arc::new(gesture_debug::GestureDebugState::default()))
        .invoke_handler(tauri::generate_handler![
            commands::list_system_folders,
            commands::add_system_folder,
            commands::open_system_folder,
            commands::remove_system_folder,
            commands::open_system_folder_recovery,
            gesture_debug::get_gesture_debug_snapshot,
            gesture_debug::get_gesture_debug_report,
            gesture_debug::start_gesture_debug_capture,
            gesture_debug::stop_gesture_debug,
            gesture_debug::clear_gesture_debug,
            gesture_debug::trigger_gesture_debug,
            app_update::check_for_app_update,
            app_update::install_app_update,
            arc_input::commands::get_input_runtime_snapshot,
            arc_input::commands::save_input_workspace,
            arc_input::commands::preview_input_workspace,
            arc_input::commands::arrange_input_workspace,
            arc_input::commands::set_input_sharing_enabled,
            arc_input::commands::take_input_control,
            arc_input::commands::release_input_control,
            arc_input::commands::connect_input_peer,
            arc_input::commands::forget_input_peer,
            arc_input::commands::test_input_edge,
            gaze::list_gaze_cameras,
            gaze::get_gaze_status,
            gaze::start_gaze_tracking,
            gaze::stop_gaze_tracking,
            gaze::begin_gaze_calibration,
            gaze::capture_gaze_calibration_sample,
            gaze::finish_gaze_calibration,
            gaze::clear_gaze_calibration,
            gaze::cancel_gaze_calibration,
            gaze::open_gaze_calibration_windows,
            gaze::focus_gaze_calibration_screen,
            gaze::close_gaze_calibration_windows,
            gaze::send_gaze_calibration_overlay,
            gaze::request_gaze_calibration_cancel,
            gaze::open_camera_permission_settings,
            commands::get_log_status,
            commands::set_detailed_logging,
            commands::open_log_directory,
            commands::export_diagnostic_bundle,
            commands::frontend_log,
            commands::get_bootstrap_state,
            commands::get_runtime_modules,
            commands::get_transfer_state,
            commands::take_pending_transfer_request,
            commands::take_pending_tray_navigation,
            commands::refresh_transfer_devices,
            commands::get_remote_file_state,
            commands::get_local_file_shares,
            commands::list_remote_file_shares,
            commands::list_remote_directory,
            commands::create_remote_directory,
            commands::rename_remote_entry,
            commands::delete_remote_entry,
            commands::delete_remote_entries,
            commands::stop_remote_edit,
            commands::pick_remote_upload,
            commands::upload_remote_paths,
            commands::start_remote_upload,
            commands::start_remote_upload_paths,
            commands::download_remote_entry,
            commands::download_remote_entries,
            commands::start_remote_download_entries,
            commands::list_remote_file_transfers,
            commands::get_remote_file_thumbnail,
            commands::open_remote_entry,
            commands::prepare_remote_drag,
            commands::start_remote_file_promise_drag,
            commands::add_remote_file_share,
            commands::remove_remote_file_share,
            commands::set_remote_file_share_writable,
            commands::set_remote_file_share_web_policy,
            commands::get_web_gateway_status,
            commands::revoke_web_sessions,
            commands::web_gateway_qr_code,
            commands::open_web_gateway_url,
            commands::get_printer_sharing_state,
            commands::get_print_job_activity,
            commands::observe_print_jobs,
            commands::refresh_printer_sharing_state,
            commands::install_remote_printer,
            commands::remove_remote_printer,
            commands::publish_printer,
            commands::suspend_printer_share,
            commands::pick_transfer_files,
            commands::inspect_transfer_files,
            commands::take_tray_transfer_drop,
            commands::update_tray_transfer_panel,
            commands::hide_tray_transfer_panel,
            commands::open_tray_transfer_history,
            commands::send_transfer,
            commands::send_system_share_transfer,
            commands::respond_transfer,
            commands::pause_transfer,
            commands::resume_transfer,
            commands::cancel_transfer,
            commands::choose_transfer_receive_directory,
            commands::open_transfer_receive_directory,
            commands::set_transfer_receive_policy,
            commands::list_system_share_requests,
            commands::submit_system_share_request,
            commands::discard_system_share_request,
            commands::get_action_presets,
            commands::install_action_preset,
            commands::list_installed_apps,
            commands::get_installed_app_icon,
            commands::export_actions_text,
            commands::import_actions_text,
            commands::validate_action_shortcut,
            commands::save_action,
            commands::delete_action,
            commands::execute_action,
            commands::get_action_output,
            commands::list_notifications,
            commands::create_test_notification,
            commands::mark_notification_read,
            commands::delete_notification,
            commands::get_mcp_config,
            commands::list_mcp_clients,
            commands::create_mcp_client,
            commands::set_mcp_client_permissions,
            commands::revoke_mcp_client,
            commands::rotate_mcp_client_token,
            commands::get_mcp_client_config,
            commands::list_mcp_configuration_changes,
            commands::list_automations,
            commands::pick_automation_path,
            commands::save_automation,
            commands::set_automation_enabled,
            commands::delete_automation,
            commands::run_automation,
            commands::test_automation,
            commands::automation_capabilities,
            commands::check_automation,
            commands::preview_automation_schedule,
            commands::open_automation_screen_permission,
            commands::list_automation_activities,
            commands::confirm_automation_activity,
            commands::cancel_automation_activity,
            commands::clear_automation_activities,
            commands::respond_pairing,
            commands::discover_desktop_devices,
            commands::connect_desktop_device,
            commands::connect_desktop_address,
            commands::forget_paired_device,
            commands::disconnect_device,
            commands::set_device_auto_connect,
            commands::open_input_permission_settings,
            commands::close_input_permission_guide,
            commands::permission_guide_ready,
            commands::start_permission_guide_window_drag,
            commands::open_accessibility_system_settings,
            commands::refresh_input_permission,
            commands::get_application_bundle_path,
            commands::get_application_drag_icon_path,
            commands::set_privacy_enabled,
            commands::update_privacy_settings,
            commands::get_app_settings,
            commands::preview_sound,
            commands::get_sound_mute_until,
            commands::set_sound_temporary_mute,
            commands::reset_sound_preferences,
            commands::update_app_settings,
            commands::show_test_system_notification,
            commands::open_full_disk_access_settings,
            commands::open_sniptra_settings,
            commands::start_screenshot_capture,
            commands::clipboard_history,
            commands::clipboard_timeline,
            commands::clipboard_thumbnail,
            commands::clipboard_image_preview,
            commands::clipboard_image_ocr,
            commands::clipboard_html_preview,
            commands::clipboard_copy_record,
            commands::clipboard_copy_text,
            commands::clipboard_paste_text,
            commands::clipboard_paste_record,
            commands::clipboard_paste_record_as,
            commands::clipboard_paste_records,
            commands::clipboard_start_continuous_paste,
            commands::clipboard_delete_record,
            commands::clipboard_delete_records,
            commands::clipboard_set_favorite,
            commands::clipboard_labels,
            commands::clipboard_create_label,
            commands::clipboard_update_label,
            commands::clipboard_delete_label,
            commands::clipboard_set_labels,
            commands::clipboard_set_label_membership,
            commands::set_clipboard_context_menu_open,
            commands::clipboard_edit_text,
            commands::clipboard_text_content,
            commands::clipboard_text_segments,
            commands::clipboard_join_segments,
            commands::clipboard_merge_devices,
            commands::clipboard_send_files,
            commands::clipboard_clear_history,
            commands::hide_clipboard_window,
            commands::get_clipboard_window_pinned,
            commands::set_clipboard_window_pinned,
            commands::start_clipboard_window_drag,
            commands::quit_app,
        ]);

    #[cfg(target_os = "macos")]
    let builder = builder.plugin(tauri_nspanel::init());

    let app = builder
        .setup(move |app| {
            let system_share = system_share::SystemShareService::for_user().unwrap_or_else(|error| {
                tracing::error!(%error, "Failed to prepare ArcRelay system share inbox");
                std::process::exit(1);
            });
            let prepared = tauri::async_runtime::block_on(backend::PreparedBackend::initialize()).unwrap_or_else(|error| {
                tracing::error!(%error, "Failed to prepare ArcRelay backend");
                std::process::exit(1);
            });
            let desktop_state = prepared.state.clone();
            let input_paths = arc_input::ProductPaths::default_for_user().unwrap_or_else(|error| {
                tracing::error!(%error, "cannot locate Arc Input data directory");
                std::process::exit(1);
            });
            let input_identity = Arc::new(
                arc_input::ProductIdentity::from_device_id(desktop_state.local_device_id.clone())
                    .unwrap_or_else(|error| {
                        tracing::error!(%error, "cannot load Arc Input identity");
                        std::process::exit(1);
                    }),
            );
            let input_runtime = tauri::async_runtime::block_on(arc_input::ArcInputRuntime::load(
                input_paths,
                input_identity,
                desktop_state.network.clone(),
            ))
            .unwrap_or_else(|error| {
                tracing::error!(%error, "cannot start Arc Input runtime");
                std::process::exit(1);
            });
            let input_service_id = input_runtime.identity().service_instance_id.clone();
            let (input_sessions_tx, input_sessions_rx) = tokio::sync::oneshot::channel();
            app.manage(desktop_state);
            app.manage(system_share.clone());
            app.manage(input_runtime.clone());
            app.manage(gaze::GazeService::new(input_runtime.clone()));
            for args in app.state::<startup::PendingActivations>().take() {
                if let Err(error) = system_share.handle_activation_args(args.into_iter().map(std::ffi::OsString::from)) {
                    tracing::warn!(%error, "Could not import queued system share request");
                }
            }

            commands::prepare_clipboard_preview_cache(app.handle()).map_err(std::io::Error::other)?;
            windowing::setup_tray(app)?;
            windowing::setup_main_window(app.handle())?;
            system_share.initialize().map_err(std::io::Error::other)?;
            if let Err(error) = system_share.handle_activation_args(std::env::args_os()) {
                tracing::warn!(%error, "Could not import the launch system share request");
            }
            system_share::announce_pending(app.handle(), &system_share);
            system_share::start_external_monitor(app.handle().clone(), system_share.clone());
            #[cfg(target_os = "linux")]
            if let Err(error) = system_share::install_linux_user_integrations() {
                tracing::warn!(%error, "Could not install Linux file-manager share integrations");
            }
            let clipboard_enabled = app
                .state::<backend::DesktopState>()
                .settings
                .snapshot()
                .clipboard_enabled;
            windowing::sync_clipboard_window(app.handle(), clipboard_enabled)?;
            observability::start_health_reporter();
            hang_watchdog::start(app.handle().clone());
            if let Err(error) = app.global_shortcut().on_shortcut(
                PRIVACY_UNLOCK_SHORTCUT,
                |app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let privacy = app.state::<backend::DesktopState>().privacy.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        match privacy.disable_frontmost_app() {
                            Ok(Some(app_name)) => {
                                tracing::info!(%app_name, "Disabled privacy mask from global shortcut");
                            }
                            Ok(None) => {
                                tracing::debug!("Privacy unlock shortcut ignored: frontmost app is not protected");
                            }
                            Err(error) => {
                                tracing::warn!(%error, "Failed to disable privacy mask from shortcut");
                            }
                        }
                    });
                },
            ) {
                tracing::warn!(%error, "Failed to register privacy unlock shortcut");
            }
            let emergency_runtime = input_runtime.clone();
            if let Err(error) = app.global_shortcut().on_shortcut(
                INPUT_EMERGENCY_SHORTCUT,
                move |app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let runtime = emergency_runtime.clone();
                    let gesture_debug = app.state::<Arc<gesture_debug::GestureDebugState>>().inner().clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        gesture_debug.stop();
                        if let Err(error) = runtime.release_control() {
                            tracing::error!(%error, "Arc Input emergency release failed");
                        } else {
                            tracing::warn!("Arc Input emergency release activated");
                        }
                    });
                },
            ) {
                tracing::warn!(%error, "could not register Arc Input emergency shortcut");
            }
            let recenter_runtime = input_runtime.clone();
            if let Err(error) = app.global_shortcut().on_shortcut(
                INPUT_RECENTER_SHORTCUT,
                move |_app, _shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let runtime = recenter_runtime.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(error) = runtime.recenter_local_pointer() {
                            tracing::error!(%error, "Arc Input pointer recenter failed");
                        }
                    });
                },
            ) {
                tracing::warn!(%error, "could not register Arc Input pointer recenter shortcut");
            }
            if let Err(error) = commands::apply_configured_shortcuts(app.handle()) {
                tracing::warn!(%error, "Failed to register configured shortcuts");
            }
            let shortcut_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                crate::action_shortcuts::restore(shortcut_app).await;
            });
            let state = app.state::<backend::DesktopState>();
            let mut settings = state.settings.snapshot();
            match autostart::sync_on_startup(app.handle()) {
                Ok(launch_at_startup) if settings.launch_at_startup != launch_at_startup => {
                    settings.launch_at_startup = launch_at_startup;
                    let _ = state.settings.save(settings);
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "failed to synchronize launch-at-login registration");
                }
            }
            tracing::info!(
                event = "input.runtime_ready",
                product_id = arcrelay_input::PRODUCT_ID,
                service_instance_id = %observability::pseudonym(input_service_id.as_str()),
                workspace_persisted = input_runtime.paths().workspace_store.exists(),
                "Arc Input feature ready"
            );
            let mut input_events = input_runtime.subscribe();
            let input_event_runtime = input_runtime.clone();
            let input_desktop_state = state.inner().clone();
            let input_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match input_events.recv().await {
                        Ok(arc_input::runtime::RuntimeEvent::SnapshotChanged) => {}
                        Ok(arc_input::runtime::RuntimeEvent::GazeCalibrationOverlay(event)) => {
                            if let Err(error) = gaze::apply_gaze_calibration_overlay_event(
                                &input_app,
                                &event,
                            ) {
                                tracing::warn!(%error, "failed to apply gaze calibration overlay event");
                            }
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "Arc Input UI event stream lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                    let snapshot = input_event_runtime.snapshot();
                    let connected_devices = snapshot
                        .connected_peers
                        .iter()
                        .map(|device_id| {
                            let name = snapshot
                                .nearby_peers
                                .iter()
                                .find(|peer| &peer.service_instance_id == device_id)
                                .and_then(|peer| peer.display_name.clone())
                                .unwrap_or_else(|| device_id.clone());
                            backend::ConnectedDeviceView {
                                id: device_id.clone(),
                                name,
                                auto_connect: true,
                            }
                        })
                        .collect();
                    input_desktop_state.set_input_connected_devices(connected_devices, &input_app);
                    let _ = input_app.emit("arc-input-state", snapshot);
                }
            });
            let input_background = input_runtime.clone();
            tauri::async_runtime::spawn(async move {
                match input_sessions_rx.await {
                    Ok(incoming) => {
                        if let Err(error) = input_background
                            .run_with_reserved_incoming(incoming)
                            .await
                        {
                            tracing::error!(%error, "Arc Input background runtime stopped");
                        }
                    }
                    Err(_) => tracing::error!(
                        event = "input.connection.receiver_reservation_failed",
                        "Arc Input could not reserve its startup session receiver"
                    ),
                }
            });
            prepared.start(
                app.handle().clone(),
                input_sessions_tx,
                input_runtime.workspace_input_adapter(),
            );
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build ArcRelay");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::Exit => {
            app_handle
                .state::<Arc<gesture_debug::GestureDebugState>>()
                .stop();
            let gaze = app_handle.state::<Arc<gaze::GazeService>>();
            if let Err(error) = tauri::async_runtime::block_on(gaze.stop()) {
                tracing::warn!(%error, "failed to stop gaze tracking during exit");
            }
        }
        tauri::RunEvent::Ready => {
            APP_EVENT_LOOP_READY.store(true, Ordering::SeqCst);
        }
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            let state = app_handle.state::<backend::DesktopState>();
            let prevented = code.is_none() && !state.quitting.load(Ordering::SeqCst);
            tracing::info!(
                event = "app.exit_requested",
                ?code,
                prevented,
                "ArcRelay desktop exit requested"
            );
            if prevented {
                api.prevent_exit();
            } else {
                let gaze = app_handle.state::<Arc<gaze::GazeService>>();
                if let Err(error) = tauri::async_runtime::block_on(gaze.stop()) {
                    tracing::warn!(%error, "failed to stop gaze tracking during exit");
                }
                if let Err(error) = app_handle
                    .state::<Arc<arc_input::ArcInputRuntime>>()
                    .shutdown()
                {
                    tracing::warn!(%error, "failed to stop Arc Input during exit");
                }
            }
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { api, .. },
            ..
        } if label == windowing::CLIPBOARD_WINDOW_LABEL => {
            api.prevent_close();
            let _ = windowing::hide_clipboard_window(app_handle);
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Focused(false),
            ..
        } if label == windowing::CLIPBOARD_WINDOW_LABEL && !cfg!(target_os = "macos") => {
            let app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                let should_hide = app
                    .get_webview_window(windowing::CLIPBOARD_WINDOW_LABEL)
                    .is_some_and(|window| !window.is_focused().unwrap_or(false));
                if should_hide
                    && !windowing::clipboard_window_pinned()
                    && !windowing::clipboard_context_menu_open()
                {
                    let _ = windowing::hide_clipboard_window(&app);
                }
            });
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Resized(_),
            ..
        } if label == windowing::CLIPBOARD_WINDOW_LABEL => {
            windowing::schedule_clipboard_window_size_save(app_handle);
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::ScaleFactorChanged { .. },
            ..
        } if label == windowing::CLIPBOARD_WINDOW_LABEL => {
            windowing::restore_clipboard_window_size_after_scale_change(app_handle);
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Focused(focused),
            ..
        } if label == "main" => {
            windowing::set_main_window_exists(true);
            windowing::set_main_window_active(focused);
            if !focused {
                return;
            }
            let state = app_handle.state::<backend::DesktopState>();
            state.request_input_permission_refresh();
            state.privacy.request_refresh();
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::Destroyed,
            ..
        } if label == "main" => {
            app_handle
                .state::<Arc<gesture_debug::GestureDebugState>>()
                .stop();
            app_handle
                .state::<backend::DesktopState>()
                .print_activity
                .stop(&label);
            windowing::main_window_destroyed(app_handle);
        }
        tauri::RunEvent::Resumed => {
            let state = app_handle.state::<backend::DesktopState>();
            state.request_input_permission_refresh();
            state.privacy.request_refresh();
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } if APP_EVENT_LOOP_READY.load(Ordering::SeqCst) => {
            // macOS can send Reopen during launch, before setup has applied
            // the silent-start preference. Only honor subsequent user requests.
            windowing::request_main_window(app_handle.clone());
        }
        _ => {}
    });
    tracing::info!(event = "app.stopped", "ArcRelay desktop stopped");
    observability::flush();
}
