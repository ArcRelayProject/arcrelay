use super::action_service::{execute_automation_snapshot, ActionService};
use crate::{
    domain::quick_action::{ActionType, QuickAction, SystemOperation},
    notification::{NewNotification, NotificationKind, NotificationStore},
};
use arcrelay_automation::*;
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex, OnceLock, RwLock},
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;
pub mod hotkeys;
pub(crate) mod platform;
pub(crate) mod retire_legacy;

#[derive(Clone)]
pub struct DesktopAutomations {
    pub engine: AutomationEngine,
    runner: Arc<DesktopRunner>,
}
struct DesktopRunner {
    actions: Arc<Mutex<ActionService>>,
    notifications: NotificationStore,
    app: OnceLock<AppHandle>,
    state: RwLock<EnvironmentState>,
    origin: Mutex<Option<(String, std::time::Instant)>>,
}
impl DesktopAutomations {
    pub async fn set_enabled(&self, id: &str, enabled: bool) -> std::result::Result<(), String> {
        let _guard = crate::action_shortcuts::MUTATION.lock().await;
        let app = self
            .runner
            .app
            .get()
            .ok_or("automation service is not initialized")?;
        let mut definition = self.engine.get(id).await.map_err(|e| e.to_string())?;
        definition.enabled = enabled;
        hotkeys::reserve(app, &self.engine, &definition).await?;
        let saved = self
            .engine
            .save(definition)
            .await
            .map_err(|e| e.to_string());
        let cleanup = hotkeys::sync_locked(app, &self.engine).await;
        saved?;
        cleanup?;
        Ok(())
    }
    pub fn new(
        directory: &Path,
        actions: Arc<Mutex<ActionService>>,
        notifications: NotificationStore,
    ) -> Result<Self> {
        let runner = Arc::new(DesktopRunner {
            actions,
            notifications,
            app: OnceLock::new(),
            state: RwLock::new(EnvironmentState::default()),
            origin: Mutex::new(None),
        });
        let store = AutomationStore::new(&format!(
            "sqlite://{}",
            directory.join("automations.sqlite3").display()
        ))?;
        Ok(Self {
            engine: AutomationEngine::new(store, runner.clone()),
            runner,
        })
    }
    pub async fn start(&self, app: AppHandle) {
        let _ = self.runner.app.set(app.clone());
        let mut events = self.engine.subscribe();
        let mut sound_tracker = crate::sound::AutomationSoundTracker::default();
        let event_app = app.clone();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(activity) => {
                        let sound = sound_tracker.update(&activity);
                        let _ = event_app.emit("automation-activity", &activity);
                        if matches!(
                            activity.status,
                            ActivityStatus::AwaitingConfirmation
                                | ActivityStatus::Failed
                                | ActivityStatus::Interrupted
                                | ActivityStatus::Succeeded
                        ) {
                            use crate::desktop_notification::{
                                show, DesktopNotification, DesktopNotificationCategory,
                            };
                            let category =
                                if activity.status == ActivityStatus::AwaitingConfirmation {
                                    DesktopNotificationCategory::WorkflowActionRequired
                                } else if activity.status == ActivityStatus::Succeeded {
                                    DesktopNotificationCategory::WorkflowCompleted
                                } else {
                                    DesktopNotificationCategory::WorkflowFailed
                                };
                            let notification = DesktopNotification::new(
                                category,
                                &activity.definition.name,
                                activity
                                    .reason
                                    .as_deref()
                                    .unwrap_or("review the automation activity"),
                            );
                            let settings =
                                &event_app.state::<crate::backend::DesktopState>().settings;
                            let result = if let Some(sound) = sound {
                                crate::desktop_notification::show_with_sound(
                                    &event_app,
                                    settings,
                                    notification,
                                    sound,
                                    Some(&activity.automation_id),
                                )
                            } else {
                                show(&event_app, settings, notification)
                            };
                            if let Err(error) = result {
                                tracing::warn!(%error, activity_id = %activity.id, "failed to deliver automation notification");
                            }
                        } else if let Some(sound) = sound {
                            crate::sound::play_automation(sound, &activity.automation_id);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
        if let Err(error) = self.engine.recover().await {
            tracing::error!(%error,"automation recovery failed");
            return;
        }
        self.engine.start_scheduler();
        platform::start_session_events(self.clone());
        let owner = self.clone();
        tokio::spawn(async move {
            let mut config = owner.engine.subscribe_configuration();
            let mut interval = tokio::time::interval(Duration::from_millis(500));
            let mut interested = false;
            let mut targets = Vec::<ApplicationIdentity>::new();
            let mut initialized = false;
            let mut previous = HashMap::<String, ApplicationIdentity>::new();
            let mut focused = None::<String>;
            let mut candidate = None::<String>;
            let mut candidate_count = 0;
            loop {
                tokio::select! { _=interval.tick()=>{}, result=config.changed()=>{ if result.is_err(){break;} let _ = app.emit("automation-configuration", ()); initialized=false; } }
                if !initialized {
                    let definitions = owner.engine.list().await.unwrap_or_default();
                    targets.clear();
                    for definition in definitions.iter().filter(|d| d.enabled) {
                        if let AutomationTrigger::Application { apps, .. } = &definition.trigger {
                            targets.extend(apps.iter().cloned());
                        }
                        for condition in &definition.conditions {
                            if let AutomationCondition::ApplicationRunning { app, .. } = condition {
                                targets.push(app.clone());
                            }
                        }
                    }
                    interested = definitions.iter().any(|d| {
                        d.enabled
                            && (matches!(d.trigger, AutomationTrigger::Application { .. })
                                || d.conditions.iter().any(|c| {
                                    matches!(c, AutomationCondition::ApplicationRunning { .. })
                                }))
                    });
                    if let Err(error) = hotkeys::sync(&app, &owner.engine).await {
                        tracing::warn!(%error,"automation hotkey registration failed");
                    }
                }
                if !interested {
                    initialized = true;
                    continue;
                }
                let selected = targets.clone();
                let (apps, next_focused) =
                    tokio::task::spawn_blocking(move || platform::applications(&selected))
                        .await
                        .unwrap_or_default();
                owner
                    .runner
                    .state
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .running_app_ids = apps.keys().cloned().collect();
                if !initialized {
                    previous = apps;
                    focused = next_focused.clone();
                    candidate = next_focused;
                    initialized = true;
                    continue;
                }
                for (id, identity) in &apps {
                    if !previous.contains_key(id) {
                        owner
                            .application_event("application.started", identity)
                            .await;
                    }
                }
                for (id, identity) in &previous {
                    if !apps.contains_key(id) {
                        owner
                            .application_event("application.exited", identity)
                            .await;
                    }
                }
                if next_focused == candidate {
                    candidate_count += 1;
                } else {
                    candidate = next_focused;
                    candidate_count = 1;
                }
                // Two consecutive samples suppress transient focus flicker, same-app window changes do nothing.
                if candidate_count >= 2 && candidate != focused {
                    if let Some(identity) = focused.as_ref().and_then(|id| previous.get(id)) {
                        owner
                            .application_event("application.background", identity)
                            .await;
                    }
                    if let Some(identity) = candidate.as_ref().and_then(|id| apps.get(id)) {
                        owner
                            .application_event("application.foreground", identity)
                            .await;
                    }
                    focused = candidate.clone();
                }
                previous = apps;
            }
        });
        let _ = self
            .engine
            .dispatch(AutomationEvent::new("system.started"))
            .await;
    }
    async fn application_event(&self, kind: &str, app: &ApplicationIdentity) {
        let mut event = AutomationEvent::new(kind);
        event.origin_automation_id = self.recent_origin();
        event.variables.extend([
            ("event.app.id".into(), app.id.clone()),
            ("event.app.name".into(), app.name.clone()),
            ("event.app.path".into(), app.path.clone()),
        ]);
        if let Err(error) = self.engine.dispatch(event).await {
            tracing::warn!(%error,"application automation dispatch failed");
        }
    }
    pub async fn system_event(&self, kind: &str) {
        if kind == "system.locked" || kind == "system.unlocked" {
            self.runner
                .state
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .locked = kind == "system.locked";
        }
        let mut event = AutomationEvent::new(kind);
        event.origin_automation_id = self.recent_origin();
        if let Err(error) = self.engine.dispatch(event).await {
            tracing::warn!(%error,"session automation dispatch failed");
        }
    }
    fn recent_origin(&self) -> Option<String> {
        self.runner
            .origin
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(3))
            .map(|(id, _)| id.clone())
    }
    pub async fn relay_event(&self, event: &crate::domain::host_event::HostEvent) {
        let normalized = normalize_event(event);
        if let Err(error) = self.engine.dispatch(normalized).await {
            tracing::warn!(%error,"ArcRelay automation dispatch failed");
        }
    }
}

fn normalize_event(event: &crate::domain::host_event::HostEvent) -> AutomationEvent {
    let mut normalized = AutomationEvent::new(&event.kind);
    let payload = &event.payload;
    for (source, target) in [
        ("deviceId", "event.device.id"),
        ("deviceName", "event.device.name"),
    ] {
        if let Some(value) = payload.get(source).and_then(|v| v.as_str()) {
            normalized.variables.insert(target.into(), value.into());
        }
    }
    if event.kind == "transfer.completed" {
        normalized.kind = if payload["direction"] == "receiving" {
            "transfer.received"
        } else {
            "transfer.sent"
        }
        .into();
        if let Some(id) = payload["transferId"].as_str() {
            normalized.id = format!("transfer:{id}");
        }
        let files = payload["files"].as_array().cloned().unwrap_or_default();
        if normalized.kind == "transfer.received" {
            if let Some(file) = files.first() {
                if let Some(directory) = file["receiveDirectory"].as_str() {
                    normalized
                        .variables
                        .insert("event.file.directory".into(), directory.into());
                }
                if let Some(path) = file["localPath"].as_str() {
                    normalized
                        .variables
                        .insert("event.file.firstPath".into(), path.into());
                }
            }
        }
        normalized
            .variables
            .insert("event.file.count".into(), files.len().to_string());
        normalized.variables.insert(
            "event.file.kinds".into(),
            payload["fileKinds"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default(),
        );
    }
    normalized
}

#[async_trait::async_trait]
impl AutomationRunner for DesktopRunner {
    async fn application_available(
        &self,
        app: &ApplicationIdentity,
    ) -> std::result::Result<(), String> {
        if !app.path.is_empty() && Path::new(&app.path).exists() {
            Ok(())
        } else {
            Err(format!(
                "application `{}` was not found; select an installed application again",
                app.name
            ))
        }
    }
    async fn record_origin(&self, id: &str) {
        *self.origin.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((id.into(), std::time::Instant::now()));
    }
    async fn capabilities(&self) -> Vec<Capability> {
        let supported = platform::application_support();
        let mut result = vec![];
        for id in [
            "manual",
            "schedule",
            "hotkey",
            "device.connected",
            "device.disconnected",
            "transfer.received",
            "transfer.sent",
            "system.started",
            "shell.system",
        ] {
            result.push(Capability {
                id: id.into(),
                available: true,
                reason: None,
                remedy: None,
            });
        }
        for id in [
            "application.started",
            "application.exited",
            "application.foreground",
            "application.background",
        ] {
            let supported = supported
                && (!matches!(id, "application.foreground" | "application.background")
                    || platform::foreground_support());
            result.push(Capability {
                id: id.into(),
                available: supported,
                reason: (!supported).then(|| "当前桌面环境不支持稳定的应用身份监控".into()),
                remedy: Some("platform".into()),
            });
        }
        for id in [
            "system.locked",
            "system.unlocked",
            "system.sleeping",
            "system.resumed",
        ] {
            let available = platform::session_support(id);
            result.push(Capability {
                id: id.into(),
                available,
                reason: (!available).then(|| "当前系统暂不支持此会话事件".into()),
                remedy: Some("platform".into()),
            });
        }
        for (name, path) in [
            ("sh", "/bin/sh"),
            ("bash", "/bin/bash"),
            ("zsh", "/bin/zsh"),
            ("powershell", "powershell.exe"),
        ] {
            let available = if name == "powershell" {
                cfg!(windows)
            } else {
                Path::new(path).is_file()
            };
            result.push(Capability {
                id: format!("shell.{name}"),
                available,
                reason: (!available).then(|| "此 Shell 在当前系统不可用".into()),
                remedy: Some("edit".into()),
            });
        }
        for (id, available, reason, remedy) in [
            (
                "input.simulate",
                platform::input_allowed(),
                "需要辅助功能权限：请允许 ArcRelay 模拟键盘输入",
                "inputPermission",
            ),
            (
                "screen.capture",
                platform::capture_allowed(),
                "需要屏幕录制权限：请允许 ArcRelay 截图",
                "screenPermission",
            ),
            (
                "action.applescript",
                cfg!(target_os = "macos"),
                "当前系统不支持 AppleScript",
                "edit",
            ),
        ] {
            result.push(Capability {
                id: id.into(),
                available,
                reason: (!available).then(|| reason.into()),
                remedy: Some(remedy.into()),
            });
        }
        result.push(Capability {
            id: "session.confirm".into(),
            available: platform::session_support("system.locked"),
            reason: (!platform::session_support("system.locked"))
                .then(|| "无法可靠确认桌面已解锁；此会话不支持安全确认".into()),
            remedy: Some("platform".into()),
        });
        let mobile = !self.environment().await.connected_device_ids.is_empty();
        result.push(Capability {
            id: "notification.mobile".into(),
            available: mobile,
            reason: (!mobile).then(|| "没有已连接的移动设备；请连接设备或关闭移动推送".into()),
            remedy: Some("devices".into()),
        });
        result
    }
    async fn environment(&self) -> EnvironmentState {
        let mut state = self.state.read().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(app) = self.app.get() {
            state.connected_device_ids = app
                .state::<crate::backend::DesktopState>()
                .automation_device_ids();
        }
        state.locked = platform::locked();
        state
    }
    async fn snapshot_action(&self, id: &str) -> std::result::Result<ActionSnapshot, String> {
        if let Some(snapshot) = privacy_snapshot(id) {
            return Ok(snapshot);
        }
        let action = self
            .actions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find_cloned(id)
            .ok_or_else(|| "quick action was deleted; select an action again".to_string())?;
        let requires_unlocked_session = matches!(
            action.action_type,
            ActionType::Hotkey { .. }
                | ActionType::System {
                    operation: SystemOperation::ScreenshotRegion | SystemOperation::ScreenshotFull
                }
        );
        let requirements = match &action.action_type {
            ActionType::Hotkey { .. } => vec!["input.simulate".into()],
            ActionType::System {
                operation: SystemOperation::ScreenshotRegion | SystemOperation::ScreenshotFull,
            } => vec!["screen.capture".into()],
            ActionType::AppleScript { .. } => vec!["action.applescript".into()],
            _ => vec![],
        };
        Ok(ActionSnapshot {
            name: action.name.clone(),
            requires_confirmation: action.requires_confirmation(),
            requires_unlocked_session,
            requirements,
            definition: serde_json::to_value(action).map_err(|e| e.to_string())?,
        })
    }
    async fn execute_action(
        &self,
        snapshot: &ActionSnapshot,
        cancel: CancellationToken,
    ) -> StepResult {
        let capabilities = self.capabilities().await;
        if let Some(id) = snapshot
            .requirements
            .iter()
            .find(|id| !capabilities.iter().any(|c| &c.id == *id && c.available))
        {
            return StepResult {
                error: Some(
                    capabilities
                        .iter()
                        .find(|c| &c.id == id)
                        .and_then(|c| c.reason.clone())
                        .unwrap_or_else(|| {
                            "a capability required by the action is unavailable".into()
                        }),
                ),
                ..Default::default()
            };
        }
        if snapshot
            .definition
            .get("automationBuiltin")
            .and_then(|value| value.as_str())
            == Some("privacy")
        {
            let Some(enabled) = snapshot
                .definition
                .get("enabled")
                .and_then(|value| value.as_bool())
            else {
                return StepResult {
                    error: Some("invalid privacy action snapshot".into()),
                    ..Default::default()
                };
            };
            let Some(app) = self.app.get() else {
                return StepResult {
                    error: Some("desktop service is not initialized".into()),
                    ..Default::default()
                };
            };
            let privacy = app.state::<crate::backend::DesktopState>().privacy.clone();
            return match tokio::task::spawn_blocking(move || privacy.set_enabled(enabled)).await {
                Ok(()) => StepResult {
                    stdout: if enabled {
                        "已开启投屏隐私"
                    } else {
                        "已关闭投屏隐私"
                    }
                    .into(),
                    ..Default::default()
                },
                Err(error) => StepResult {
                    error: Some(error.to_string()),
                    ..Default::default()
                },
            };
        }
        let action = match serde_json::from_value::<QuickAction>(snapshot.definition.clone()) {
            Ok(a) => a,
            Err(e) => {
                return StepResult {
                    error: Some(e.to_string()),
                    ..Default::default()
                }
            }
        };
        // Ordinary shell quick actions share the same bounded cancellable runner; toggles retain ActionService ownership.
        if let ActionType::ShellCommand {
            command,
            working_dir,
        } = &action.action_type
        {
            return run_shell_literal(
                ShellKind::System,
                command,
                working_dir.as_deref(),
                300,
                cancel,
            )
            .await;
        }
        let service = self.actions.clone();
        // A synchronous OS action cannot be safely interrupted midway. Await it, then cancel subsequent steps.
        match tokio::task::spawn_blocking(move || execute_automation_snapshot(&service, action))
            .await
        {
            Ok(Ok(message)) => StepResult {
                stdout: message,
                ..Default::default()
            },
            Ok(Err(error)) => StepResult {
                error: Some(error),
                ..Default::default()
            },
            Err(error) => StepResult {
                error: Some(error.to_string()),
                ..Default::default()
            },
        }
    }
    async fn notify(&self, title: &str, body: &str, mobile: bool) -> StepResult {
        let result = if mobile {
            if self.environment().await.connected_device_ids.is_empty() {
                Err("cannot push notification because no device is connected".into())
            } else {
                self.notifications
                    .create(NewNotification {
                        title: title.into(),
                        body: body.into(),
                        source: "ArcRelay Automation".into(),
                        kind: NotificationKind::Info,
                        reference: None,
                    })
                    .map(|_| ())
            }
        } else if let Some(app) = self.app.get() {
            use crate::desktop_notification::{
                show, DesktopNotification, DesktopNotificationCategory,
            };
            show(
                app,
                &app.state::<crate::backend::DesktopState>().settings,
                DesktopNotification::new(
                    DesktopNotificationCategory::AgentNotification,
                    title,
                    body,
                ),
            )
            .map(|_| ())
        } else {
            Err("desktop notifications are not initialized".into())
        };
        StepResult {
            error: result.err(),
            ..Default::default()
        }
    }
}

fn privacy_action(id: &str) -> Option<bool> {
    match id {
        "builtin-automation-privacy-on" => Some(true),
        "builtin-automation-privacy-off" => Some(false),
        _ => None,
    }
}

fn privacy_snapshot(id: &str) -> Option<ActionSnapshot> {
    let enabled = privacy_action(id)?;
    Some(ActionSnapshot {
        name: if enabled {
            "开启投屏隐私"
        } else {
            "关闭投屏隐私"
        }
        .into(),
        definition: serde_json::json!({"automationBuiltin": "privacy", "enabled": enabled}),
        requires_confirmation: !enabled,
        requires_unlocked_session: true,
        requirements: vec![],
    })
}

#[cfg(test)]
mod event_tests {
    use super::*;
    #[test]
    fn privacy_actions_are_explicit_and_do_not_toggle() {
        assert_eq!(privacy_action("builtin-automation-privacy-on"), Some(true));
        assert_eq!(
            privacy_action("builtin-automation-privacy-off"),
            Some(false)
        );
        assert_eq!(privacy_action("builtin-privacy-mode"), None);
    }
    #[test]
    fn privacy_snapshots_require_local_safety_checks() {
        let enabled = privacy_snapshot("builtin-automation-privacy-on").unwrap();
        let disabled = privacy_snapshot("builtin-automation-privacy-off").unwrap();
        assert!(enabled.requires_unlocked_session);
        assert!(disabled.requires_unlocked_session);
        assert!(disabled.requires_confirmation);
        assert_eq!(enabled.definition["enabled"], true);
        assert_eq!(disabled.definition["enabled"], false);
        assert!(privacy_snapshot("user-action").is_none());
    }
    #[test]
    fn completed_transfer_uses_finalized_location_not_a_guessed_filename() {
        let event = crate::domain::host_event::HostEvent::new(
            "transfer.completed",
            serde_json::json!({
                "transferId":"transfer-1","direction":"receiving","deviceName":"Phone",
                "receiveDirectory":"/new-preference","files":[{"name":"report.pdf","relativePath":"nested/report.pdf","localPath":"/original/nested/report (1).pdf","receiveDirectory":"/original"}]
            }),
        );
        let actual = normalize_event(&event);
        assert_eq!(actual.variables["event.file.directory"], "/original");
        assert_eq!(
            actual.variables["event.file.firstPath"],
            "/original/nested/report (1).pdf"
        );
        let mut sent = event.clone();
        sent.payload["direction"] = serde_json::json!("sending");
        assert!(!normalize_event(&sent)
            .variables
            .contains_key("event.file.firstPath"));
    }
}
