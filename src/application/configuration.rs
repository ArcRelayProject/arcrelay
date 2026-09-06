//! Shared desktop/MCP mutations. Callers hold the global configuration mutation
//! gate while checking versions, dependencies, privileges and applying a change.
use crate::{backend::DesktopState, domain::quick_action::QuickAction};
use arcrelay_automation::{AutomationDefinition, AutomationStep, AutomationTrigger};
use tauri::{AppHandle, Emitter};

pub async fn action_references(
    state: &DesktopState,
    id: &str,
) -> Result<Vec<AutomationDefinition>, String> {
    Ok(state
        .automations
        .engine
        .list()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|definition| {
            definition.steps.iter().any(
                |step| matches!(step, AutomationStep::QuickAction { action_id } if action_id == id),
            )
        })
        .collect())
}

pub async fn save_action_locked(
    app: &AppHandle,
    state: &DesktopState,
    mut action: QuickAction,
    actor: &str,
) -> Result<QuickAction, String> {
    super::action_service::normalize_action(&mut action)?;
    let previous = state
        .action_service
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .find_cloned(&action.id);
    if previous
        .as_ref()
        .is_some_and(|old| old.revision != action.revision)
        || (previous.is_none() && action.revision != 0)
    {
        return Err(
            "action.conflict: the action was modified or deleted; reopen it before saving".into(),
        );
    }
    let change = state
        .mcp_journal
        .begin_change(
            actor,
            "quickAction",
            &action.id,
            previous
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| e.to_string())?,
            Some(serde_json::to_value(&action).map_err(|e| e.to_string())?),
        )
        .await?;
    let handle = app.clone();
    let host = state.clone();
    let result = tokio::task::spawn_blocking(move || {
        let mut service = host
            .action_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::action_shortcuts::validate_binding(
            &action,
            service.actions(),
            &host.settings.snapshot(),
        )?;
        let id = action.id.clone();
        host.action_shortcuts
            .replace(&handle, &id, action.global_shortcut.as_deref())?;
        let result = if previous.is_some() {
            service.update(action)
        } else {
            service.add(action)
        };
        if let Err(error) = result {
            let shortcut = previous.as_ref().and_then(|a| a.global_shortcut.as_deref());
            return Err(
                match host.action_shortcuts.replace(&handle, &id, shortcut) {
                    Ok(()) => error,
                    Err(rollback) => format!("{error}; failed to restore shortcut: {rollback}"),
                },
            );
        }
        service
            .find_cloned(&id)
            .ok_or_else(|| "saved action not found".to_string())
    })
    .await
    .map_err(|e| e.to_string())?;
    let after = result
        .as_ref()
        .ok()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|e| e.to_string())?;
    state
        .mcp_journal
        .complete_change(change, after, result.as_ref().err().cloned())
        .await?;
    let saved = result?;
    state.emit_snapshot(app);
    Ok(saved)
}

pub async fn delete_action_locked(
    app: &AppHandle,
    state: &DesktopState,
    id: &str,
    expected_revision: Option<u64>,
    actor: &str,
) -> Result<(), String> {
    let previous = state
        .action_service
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .find_cloned(id)
        .ok_or("action not found")?;
    if expected_revision.is_some_and(|revision| revision != previous.revision) {
        return Err("action.conflict: read the latest action before deleting".into());
    }
    let references = action_references(state, id).await?;
    if !references.is_empty() {
        return Err(format!(
            "action.in_use: remove this action from these automations first: {}",
            references
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let change = state
        .mcp_journal
        .begin_change(
            actor,
            "quickAction",
            id,
            Some(serde_json::to_value(&previous).map_err(|e| e.to_string())?),
            None,
        )
        .await?;
    let handle = app.clone();
    let host = state.clone();
    let id = id.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let mut service = host
            .action_service
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        host.action_shortcuts.replace(&handle, &id, None)?;
        if let Err(error) = service.remove(&id) {
            return Err(
                match host.action_shortcuts.replace(
                    &handle,
                    &id,
                    previous.global_shortcut.as_deref(),
                ) {
                    Ok(()) => error,
                    Err(rollback) => format!("{error}; failed to restore shortcut: {rollback}"),
                },
            );
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;
    state
        .mcp_journal
        .complete_change(change, None, result.as_ref().err().cloned())
        .await?;
    result?;
    state.emit_snapshot(app);
    Ok(())
}

pub async fn save_automation_locked(
    app: &AppHandle,
    state: &DesktopState,
    mut definition: AutomationDefinition,
    actor: &str,
) -> Result<AutomationDefinition, String> {
    if let AutomationTrigger::Hotkey { shortcut } = &mut definition.trigger {
        *shortcut =
            crate::action_shortcuts::normalize(Some(shortcut))?.ok_or("record a shortcut")?;
    }
    let previous = match state.automations.engine.get(&definition.id).await {
        Ok(old) => Some(old),
        Err(arcrelay_automation::AutomationError::NotFound) => None,
        Err(error) => return Err(error.to_string()),
    };
    if previous
        .as_ref()
        .is_some_and(|old| old.revision != definition.revision)
        || (previous.is_none() && definition.revision != 0)
    {
        return Err("automation.conflict: the configuration was modified or deleted; reopen it before saving".into());
    }
    let change = state
        .mcp_journal
        .begin_change(
            actor,
            "automation",
            &definition.id,
            previous
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| e.to_string())?,
            Some(serde_json::to_value(&definition).map_err(|e| e.to_string())?),
        )
        .await?;
    let saved = async {
        super::automations::hotkeys::reserve(app, &state.automations.engine, &definition).await?;
        let result = state
            .automations
            .engine
            .save(definition)
            .await
            .map_err(|e| e.to_string());
        let cleanup =
            super::automations::hotkeys::sync_locked(app, &state.automations.engine).await;
        let saved = result?;
        cleanup.map_err(|e| {
            format!("configuration saved, but shortcut synchronization failed: {e}")
        })?;
        Ok::<_, String>(saved)
    }
    .await;
    state
        .mcp_journal
        .complete_change(
            change,
            saved
                .as_ref()
                .ok()
                .map(serde_json::to_value)
                .transpose()
                .map_err(|e| e.to_string())?,
            saved.as_ref().err().cloned(),
        )
        .await?;
    let saved = saved?;
    let _ = app.emit("automation-configuration", ());
    Ok(saved)
}

pub async fn delete_automation_locked(
    app: &AppHandle,
    state: &DesktopState,
    id: &str,
    expected_revision: Option<u64>,
    actor: &str,
) -> Result<(), String> {
    let previous = state
        .automations
        .engine
        .get(id)
        .await
        .map_err(|e| e.to_string())?;
    if expected_revision.is_some_and(|revision| revision != previous.revision) {
        return Err("automation.conflict: read the latest automation before deleting".into());
    }
    let change = state
        .mcp_journal
        .begin_change(
            actor,
            "automation",
            id,
            Some(serde_json::to_value(previous).map_err(|e| e.to_string())?),
            None,
        )
        .await?;
    let result = async {
        state
            .automations
            .engine
            .delete(id)
            .await
            .map_err(|e| e.to_string())?;
        super::automations::hotkeys::sync_locked(app, &state.automations.engine).await
    }
    .await;
    state
        .mcp_journal
        .complete_change(change, None, result.as_ref().err().cloned())
        .await?;
    result?;
    let _ = app.emit("automation-configuration", ());
    Ok(())
}
