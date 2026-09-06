use arcrelay_automation::{AutomationEngine, AutomationEvent, AutomationTrigger};
use std::{
    collections::HashSet,
    sync::{Mutex, OnceLock},
};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

static REGISTERED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

pub async fn sync(app: &AppHandle, engine: &AutomationEngine) -> Result<(), String> {
    let _guard = crate::action_shortcuts::MUTATION.lock().await;
    sync_locked(app, engine).await
}

/// Reserve new shortcuts before committing a configuration. Existing listeners remain registered.
pub async fn reserve(
    app: &AppHandle,
    engine: &AutomationEngine,
    definition: &arcrelay_automation::AutomationDefinition,
) -> Result<(), String> {
    if !definition.enabled {
        return Ok(());
    }
    let AutomationTrigger::Hotkey { shortcut } = &definition.trigger else {
        return Ok(());
    };
    let mut registered = REGISTERED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if registered.contains(shortcut) {
        return Ok(());
    }
    if app.global_shortcut().is_registered(shortcut.as_str()) {
        return Err("shortcut is already used by another ArcRelay feature".into());
    }
    let shortcut_event = shortcut.clone();
    let engine = engine.clone();
    app.global_shortcut()
        .on_shortcut(shortcut.as_str(), move |_, _, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let mut event = AutomationEvent::new("hotkey");
            event
                .variables
                .insert("event.shortcut".into(), shortcut_event.clone());
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                let _ = engine.dispatch(event).await;
            });
        })
        .map_err(|e| format!("shortcut conflicts or is unavailable: {e}"))?;
    registered.insert(shortcut.clone());
    Ok(())
}
pub async fn sync_locked(app: &AppHandle, engine: &AutomationEngine) -> Result<(), String> {
    let desired = engine
        .list()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|d| d.enabled)
        .filter_map(|d| match d.trigger {
            AutomationTrigger::Hotkey { shortcut } => Some(shortcut),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut registered = REGISTERED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    // Register additions first; a conflict leaves existing shortcuts intact.
    let mut added = Vec::<String>::new();
    for shortcut in desired.difference(&registered) {
        let shortcut_event = shortcut.clone();
        let engine = engine.clone();
        let result = if app.global_shortcut().is_registered(shortcut.as_str()) {
            Err("shortcut is already used by another ArcRelay feature".into())
        } else {
            app.global_shortcut()
                .on_shortcut(shortcut.as_str(), move |_, _, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    let mut event = AutomationEvent::new("hotkey");
                    event
                        .variables
                        .insert("event.shortcut".into(), shortcut_event.clone());
                    let engine = engine.clone();
                    tauri::async_runtime::spawn(async move {
                        let _ = engine.dispatch(event).await;
                    });
                })
                .map_err(|e| format!("shortcut conflicts or is unavailable: {e}"))
        };
        if let Err(error) = result {
            for key in added {
                let _ = app.global_shortcut().unregister(key.as_str());
            }
            return Err(error);
        }
        added.push(shortcut.clone());
    }
    for shortcut in registered.difference(&desired) {
        app.global_shortcut()
            .unregister(shortcut.as_str())
            .map_err(|e| e.to_string())?;
    }
    *registered = desired;
    Ok(())
}
