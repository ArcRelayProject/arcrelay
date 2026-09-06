use super::*;
use arcrelay_automation::{
    AutomationActivity, AutomationDefinition, AutomationEvent, AutomationIssue, AutomationTrigger,
    Capability,
};

#[arcrelay_desktop_ipc::command]
pub async fn preview_automation_schedule(trigger: AutomationTrigger) -> Option<String> {
    arcrelay_automation::next_schedule(&trigger, chrono::Utc::now()).map(|at| at.to_rfc3339())
}

#[arcrelay_desktop_ipc::command]
pub async fn open_automation_screen_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn()
            .map_err(|error| error.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err("allow screen recording in the system privacy settings".into())
}

#[arcrelay_desktop_ipc::command]
pub async fn pick_automation_path(directory: bool) -> Result<Option<String>, String> {
    let dialog = rfd::AsyncFileDialog::new();
    let file = if directory {
        dialog.pick_folder().await
    } else {
        dialog.pick_file().await
    };
    Ok(file.map(|f| f.path().to_string_lossy().into_owned()))
}

#[arcrelay_desktop_ipc::command]
pub async fn list_automations(
    state: State<'_, DesktopState>,
) -> Result<Vec<AutomationDefinition>, String> {
    state
        .automations
        .engine
        .list()
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn save_automation(
    app: AppHandle,
    state: State<'_, DesktopState>,
    definition: AutomationDefinition,
) -> Result<AutomationDefinition, String> {
    let _guard = crate::action_shortcuts::MUTATION.lock().await;
    crate::application::configuration::save_automation_locked(&app, &state, definition, "desktop")
        .await
}

#[arcrelay_desktop_ipc::command]
pub async fn set_automation_enabled(
    app: AppHandle,
    state: State<'_, DesktopState>,
    automation_id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut definition = state
        .automations
        .engine
        .get(&automation_id)
        .await
        .map_err(|e| e.to_string())?;
    definition.enabled = enabled;
    save_automation(app, state, definition)
        .await
        .map_err(|e| e.message)?;
    Ok(())
}
#[arcrelay_desktop_ipc::command]
pub async fn delete_automation(
    app: AppHandle,
    state: State<'_, DesktopState>,
    automation_id: String,
) -> Result<(), String> {
    let _guard = crate::action_shortcuts::MUTATION.lock().await;
    crate::application::configuration::delete_automation_locked(
        &app,
        &state,
        &automation_id,
        None,
        "desktop",
    )
    .await
}

#[arcrelay_desktop_ipc::command]
pub async fn run_automation(
    state: State<'_, DesktopState>,
    automation_id: String,
) -> Result<String, String> {
    state
        .automations
        .engine
        .run(&automation_id)
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn test_automation(
    state: State<'_, DesktopState>,
    definition: AutomationDefinition,
    event: Option<AutomationEvent>,
) -> Result<String, String> {
    state
        .automations
        .engine
        .test(definition, event)
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn automation_capabilities(
    state: State<'_, DesktopState>,
) -> Result<Vec<Capability>, String> {
    Ok(state.automations.engine.capabilities().await)
}
#[arcrelay_desktop_ipc::command]
pub async fn check_automation(
    state: State<'_, DesktopState>,
    definition: AutomationDefinition,
) -> Result<Vec<AutomationIssue>, String> {
    Ok(state.automations.engine.preflight(&definition).await)
}
#[arcrelay_desktop_ipc::command]
pub async fn list_automation_activities(
    state: State<'_, DesktopState>,
    automation_id: Option<String>,
    limit: u32,
) -> Result<Vec<AutomationActivity>, String> {
    state
        .automations
        .engine
        .activities(automation_id.as_deref(), limit)
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn confirm_automation_activity(
    state: State<'_, DesktopState>,
    activity_id: String,
) -> Result<(), String> {
    state
        .automations
        .engine
        .confirm(&activity_id)
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn cancel_automation_activity(
    state: State<'_, DesktopState>,
    activity_id: String,
) -> Result<(), String> {
    state
        .automations
        .engine
        .cancel(&activity_id)
        .await
        .map_err(|e| e.to_string())
}
#[arcrelay_desktop_ipc::command]
pub async fn clear_automation_activities(state: State<'_, DesktopState>) -> Result<(), String> {
    state
        .automations
        .engine
        .clear_activities()
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_automations_ipc.rs"));
