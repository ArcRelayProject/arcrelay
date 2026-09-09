use arcrelay_core::domain::clipboard::ClipboardSyncPreferences;
use tauri::{AppHandle, Emitter};

use crate::backend::{BackendCommand, DesktopState};
use crate::settings::{AppSettings, AppSettingsPatch};

#[derive(Clone, Copy)]
enum Step {
    Shortcuts,
    Autostart,
    Screenshot,
    Clipboard,
    DeviceName,
    WebGateway,
}

async fn apply_step(
    step: Step,
    app: &AppHandle,
    state: &DesktopState,
    from: &AppSettings,
    to: &AppSettings,
) -> Result<(), String> {
    match step {
        Step::Shortcuts => crate::commands::settings::replace_global_shortcuts(app, from, to),
        Step::Autostart => {
            crate::commands::settings::apply_launch_at_startup(app, to.launch_at_startup)
        }
        Step::Screenshot => state.screenshot.sync_preferences(to),
        Step::Clipboard => {
            let mut policy = state.clipboard.policy().await.map_err(|e| e.to_string())?;
            policy.history_enabled = to.clipboard_enabled;
            state
                .clipboard
                .update_policy(policy)
                .await
                .map_err(|e| e.to_string())
        }
        Step::DeviceName => {
            let name = crate::backend::effective_device_name(&to.device_name);
            if let Some(transfer) = state.modules.initialized_transfer() {
                transfer
                    .set_device_name(name.clone())
                    .await
                    .map_err(|e| e.to_string())?;
            }
            state.set_local_device_name(name);
            Ok(())
        }
        Step::WebGateway => {
            state
                .web_gateway
                .reconfigure(state.effective_web_gateway_settings_for(to).await)
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        }
    }
}

pub async fn update(
    app: &AppHandle,
    state: &DesktopState,
    patch: AppSettingsPatch,
) -> Result<AppSettings, String> {
    let _mutation = crate::action_shortcuts::MUTATION.lock().await;
    let previous = state.settings.snapshot();
    let next = patch.apply(previous.clone());
    crate::settings::validate(&next)?;
    if next == previous {
        return Ok(next);
    }
    if next.enhanced_screenshot_enabled && !previous.enhanced_screenshot_enabled {
        state.screenshot.ensure_available()?;
    }
    let mut steps = Vec::new();
    if (
        previous.clipboard_enabled,
        &previous.clipboard_shortcut,
        previous.enhanced_screenshot_enabled,
        &previous.screenshot_shortcut,
    ) != (
        next.clipboard_enabled,
        &next.clipboard_shortcut,
        next.enhanced_screenshot_enabled,
        &next.screenshot_shortcut,
    ) {
        steps.push(Step::Shortcuts);
    }
    if previous.launch_at_startup != next.launch_at_startup {
        steps.push(Step::Autostart);
    }
    if (
        previous.screenshot_include_cursor,
        previous.screenshot_format,
        &previous.screenshot_file_name_template,
    ) != (
        next.screenshot_include_cursor,
        next.screenshot_format,
        &next.screenshot_file_name_template,
    ) {
        steps.push(Step::Screenshot);
    }
    if previous.clipboard_enabled != next.clipboard_enabled {
        steps.push(Step::Clipboard);
    }
    let name_changed = previous.device_name != next.device_name;
    if name_changed {
        steps.push(Step::DeviceName);
    }
    if previous.web_files != next.web_files || name_changed {
        steps.push(Step::WebGateway);
    }

    let mut attempted = Vec::new();
    let result = async {
        for step in steps {
            // Include a failing step because an OS adapter can partially apply it.
            attempted.push(step);
            apply_step(step, app, state, &previous, &next).await?;
        }
        let manager = state.settings.clone();
        let saved = next.clone();
        tauri::async_runtime::spawn_blocking(move || manager.save(saved))
            .await
            .map_err(|e| e.to_string())??;
        Ok::<_, String>(())
    }
    .await;
    if let Err(error) = result {
        let mut failures = Vec::new();
        for step in attempted.into_iter().rev() {
            if let Err(rollback) = apply_step(step, app, state, &next, &previous).await {
                failures.push(rollback);
            }
        }
        return Err(if failures.is_empty() {
            error
        } else {
            format!(
                "{error}; failed to restore some system settings: {}",
                failures.join("; ")
            )
        });
    }
    let next = state.settings.snapshot();
    if next.sounds != previous.sounds {
        crate::sound::configure(next.sounds.clone());
    }
    // Only committed settings are published to consumers and non-fallible adapters.
    if let Some(transfer) = state.modules.initialized_transfer() {
        transfer.set_discoverable(next.nearby_discoverable).await;
    }
    state
        .clipboard
        .update_sync_preferences(ClipboardSyncPreferences {
            enabled: next.clipboard_sync_enabled,
            update_system_clipboard: next.clipboard_sync_update_system_clipboard,
            sync_edits_and_deletes: next.clipboard_sync_edits_and_deletes,
            sync_favorites: next.clipboard_sync_favorites,
        });
    state
        .screenshot
        .set_enabled(next.enhanced_screenshot_enabled);
    if previous.clipboard_enabled != next.clipboard_enabled {
        if let Err(error) = crate::windowing::sync_clipboard_window(app, next.clipboard_enabled) {
            tracing::warn!(
                enabled = next.clipboard_enabled,
                %error,
                "could not synchronize clipboard WebView lifecycle"
            );
        }
    }
    if name_changed {
        state.emit_runtime_snapshot(app);
        if let Err(error) = state
            .command_tx
            .send(BackendCommand::DisconnectAllDevices)
            .await
        {
            tracing::warn!(%error, "could not refresh device sessions");
        }
    }
    if let Some(network) = state.network.get() {
        let status = state.web_gateway.status().await;
        if status.running {
            if let Err(error) = network
                .discovery()
                .publish_web_gateway(&status.site_name, status.port)
            {
                tracing::warn!(%error, "could not publish web gateway");
            }
        } else {
            network.discovery().unpublish_web_gateway();
        }
    }
    if let Err(error) = crate::windowing::refresh_tray_menu(app) {
        tracing::warn!(%error, "could not refresh tray menu");
    }
    let _ = app.emit("app-settings-changed", &next);
    Ok(next)
}
