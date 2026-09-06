//! Native quick-action triggers. The WebView is never on the execution path.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

use crate::backend::DesktopState;
use crate::domain::quick_action::QuickAction;
use crate::settings::AppSettings;

/// All runtime shortcut writers share this lock, including settings/workflows.
pub static MUTATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Default)]
pub struct ActionShortcuts {
    registered: Mutex<HashMap<String, Binding>>,
    errors: Mutex<HashMap<String, String>>,
}

#[derive(Clone)]
struct Binding {
    shortcut: Shortcut,
    gate: Arc<TriggerGate>,
}

#[derive(Default)]
struct TriggerGate {
    enabled: AtomicBool,
    held: AtomicBool,
    running: AtomicBool,
}

impl TriggerGate {
    fn begin(&self, state: ShortcutState) -> bool {
        if state == ShortcutState::Released {
            self.held.store(false, Ordering::Release);
            return false;
        }
        self.enabled.load(Ordering::Acquire)
            && !self.held.swap(true, Ordering::AcqRel)
            && !self.running.swap(true, Ordering::AcqRel)
    }
}

struct Running(Arc<TriggerGate>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

pub fn normalize(value: Option<&str>) -> Result<Option<String>, String> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let shortcut = value.parse::<Shortcut>().map_err(|_| {
        "invalid global shortcut; use a format such as CmdOrCtrl+Shift+L".to_string()
    })?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
    {
        return Err("global shortcut must include Ctrl, Alt, or Cmd/Super".to_string());
    }
    Ok(Some(shortcut.to_string()))
}

pub fn validate_binding(
    action: &QuickAction,
    actions: &[QuickAction],
    settings: &AppSettings,
) -> Result<(), String> {
    validate_shortcut(
        &action.id,
        action.global_shortcut.as_deref(),
        actions,
        settings,
    )
}

pub fn validate_shortcut(
    action_id: &str,
    value: Option<&str>,
    actions: &[QuickAction],
    settings: &AppSettings,
) -> Result<(), String> {
    let Some(value) = normalize(value)? else {
        return Ok(());
    };
    let shortcut = value.parse::<Shortcut>().expect("normalized shortcut");
    for other in actions.iter().filter(|other| other.id != action_id) {
        if other
            .global_shortcut
            .as_deref()
            .and_then(|s| s.parse::<Shortcut>().ok())
            == Some(shortcut)
        {
            return Err(format!(
                "global shortcut is already used by action `{}`",
                other.name
            ));
        }
    }
    for (enabled, value, name) in [
        (true, crate::PRIVACY_UNLOCK_SHORTCUT, "隐私解锁"),
        (true, crate::INPUT_EMERGENCY_SHORTCUT, "跨屏输入紧急释放"),
        (true, crate::INPUT_RECENTER_SHORTCUT, "跨屏输入指针居中"),
        (
            settings.clipboard_enabled,
            settings.clipboard_shortcut.as_str(),
            "剪贴板",
        ),
        (
            settings.enhanced_screenshot_enabled,
            settings.screenshot_shortcut.as_str(),
            "截图",
        ),
    ] {
        if enabled && value.parse::<Shortcut>().ok() == Some(shortcut) {
            return Err(format!("global shortcut is already used by {name}"));
        }
    }
    Ok(())
}

/// Acquire the new key before releasing the old one. Never unregister a key
/// whose registration failed (it may belong to another ArcRelay feature).
fn replace_registration(
    previous: Option<Shortcut>,
    next: Option<Shortcut>,
    mut register: impl FnMut(Shortcut) -> Result<(), String>,
    mut unregister: impl FnMut(Shortcut) -> Result<(), String>,
) -> Result<(), String> {
    if previous == next {
        return Ok(());
    }
    if let Some(next) = next {
        register(next)?;
    }
    if let Some(previous) = previous {
        if let Err(error) = unregister(previous) {
            if let Some(next) = next {
                if let Err(rollback) = unregister(next) {
                    tracing::error!(%rollback, "failed to roll back action shortcut registration");
                }
            }
            return Err(error);
        }
    }
    Ok(())
}

impl ActionShortcuts {
    /// Read-only preview. Native registration is still checked atomically on save.
    pub fn validate_available(
        &self,
        app: &AppHandle,
        action_id: &str,
        value: Option<&str>,
    ) -> Result<(), String> {
        let Some(value) = normalize(value)? else {
            return Ok(());
        };
        let shortcut = value.parse::<Shortcut>().expect("normalized shortcut");
        let owns_shortcut = self
            .registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(action_id)
            .is_some_and(|binding| binding.shortcut == shortcut);
        if !owns_shortcut && app.global_shortcut().is_registered(shortcut) {
            return Err("global shortcut is already used by another ArcRelay feature".to_string());
        }
        Ok(())
    }

    pub fn error(&self, action_id: &str) -> Option<String> {
        self.errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(action_id)
            .cloned()
    }

    /// Called under MUTATION, without holding the action catalog or registry
    /// lock while the plugin dispatches native work onto the main thread.
    pub fn replace(
        &self,
        app: &AppHandle,
        action_id: &str,
        value: Option<&str>,
    ) -> Result<(), String> {
        let next = normalize(value)?.map(|s| s.parse::<Shortcut>().expect("normalized shortcut"));
        let previous = self
            .registered
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(action_id)
            .cloned();
        let gate = Arc::new(TriggerGate::default());
        replace_registration(
            previous.as_ref().map(|binding| binding.shortcut),
            next,
            |shortcut| {
                if app.global_shortcut().is_registered(shortcut) {
                    return Err(
                        "global shortcut is already used by another ArcRelay feature".to_string(),
                    );
                }
                let gate = gate.clone();
                let action_id = action_id.to_string();
                app.global_shortcut()
                    .on_shortcut(shortcut, move |app, _, event| {
                        if !gate.begin(event.state) {
                            return;
                        }
                        let running = Running(gate.clone());
                        let app = app.clone();
                        let action_id = action_id.clone();
                        tauri::async_runtime::spawn(async move {
                            run(app, action_id, shortcut, running).await;
                        });
                    })
                    .map_err(|error| {
                        format!("failed to register global shortcut; it may be used by the system or another application: {error}")
                    })
            },
            |shortcut| {
                app.global_shortcut()
                    .unregister(shortcut)
                    .map_err(|error| format!("failed to unregister global shortcut: {error}"))
            },
        )?;
        if previous.as_ref().map(|binding| binding.shortcut) != next {
            if let Some(previous) = previous {
                previous.gate.enabled.store(false, Ordering::Release);
            }
            let mut registered = self.registered.lock().unwrap_or_else(|e| e.into_inner());
            registered.remove(action_id);
            if let Some(shortcut) = next {
                gate.enabled.store(true, Ordering::Release);
                registered.insert(action_id.to_string(), Binding { shortcut, gate });
            }
        }
        self.errors
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(action_id);
        Ok(())
    }
}

pub async fn restore(app: AppHandle) {
    let _mutation = MUTATION.lock().await;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<DesktopState>();
        let actions = state
            .action_service
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .actions()
            .to_vec();
        let settings = state.settings.snapshot();
        for action in &actions {
            if action.global_shortcut.is_none() {
                continue;
            }
            if let Err(error) = validate_binding(action, &actions, &settings).and_then(|()| {
                state
                    .action_shortcuts
                    .replace(&app, &action.id, action.global_shortcut.as_deref())
            }) {
                tracing::warn!(action_id = %action.id, %error, "could not restore action shortcut");
                state
                    .action_shortcuts
                    .errors
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(action.id.clone(), error);
            }
        }
        state.emit_snapshot(&app);
    })
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, "action shortcut startup failed");
    }
}

async fn run(app: AppHandle, action_id: String, shortcut: Shortcut, running: Running) {
    let state = app.state::<DesktopState>().inner().clone();
    let service = state.action_service.clone();
    let lookup_id = action_id.clone();
    let Ok(Some(action)) = tauri::async_runtime::spawn_blocking(move || {
        service
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find_cloned(&lookup_id)
    })
    .await
    else {
        return;
    };
    if !running.0.enabled.load(Ordering::Acquire)
        || action
            .global_shortcut
            .as_deref()
            .and_then(|value| value.parse::<Shortcut>().ok())
            != Some(shortcut)
    {
        return;
    }
    let english = state.settings.snapshot().language.uses_english_fallback();
    if action.requires_confirmation() {
        let run = if english { "Run" } else { "确认运行" };
        let result = rfd::AsyncMessageDialog::new()
            .set_title(if english {
                "Run quick action?"
            } else {
                "运行快捷动作？"
            })
            .set_description(if english {
                format!(
                    "Run “{}”? This action changes the current system state.",
                    action.name
                )
            } else {
                format!("“{}”会改变当前系统状态，请确认是否继续执行。", action.name)
            })
            .set_buttons(rfd::MessageButtons::OkCancelCustom(
                run.into(),
                if english { "Cancel" } else { "取消" }.into(),
            ))
            .show()
            .await;
        if !matches!(
            result,
            rfd::MessageDialogResult::Ok | rfd::MessageDialogResult::Yes
        ) && result != rfd::MessageDialogResult::Custom(run.into())
        {
            return;
        }
    }
    let name = action.name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result =
            crate::application::action_service::execute_definition(&state.action_service, action);
        state.emit_snapshot(&app);
        result
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result);
    if let Err(error) = result {
        tracing::warn!(%action_id, %error, "global quick action failed");
        rfd::AsyncMessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title(if english {
                "Quick action failed"
            } else {
                "快捷动作执行失败"
            })
            .set_description(format!("{name}\n{error}"))
            .show()
            .await;
    }
}

#[cfg(test)]
mod tests;
