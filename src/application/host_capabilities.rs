use std::sync::{Arc, Mutex, MutexGuard};

use arcrelay_protocol::server::{
    ActionProvider, HostCapabilityError, HostCapabilityErrorCode, HostCapabilityResult,
    HostNotificationInfo, NotificationProvider, OutputLine, QuickActionInfo, WorkflowProvider,
};
use tokio::sync::broadcast;

use super::action_service::{execute_by_id, ActionService};
use super::output_manager::OutputManager;
use crate::notification::{HostNotification, NotificationKind, NotificationStore};
use crate::privacy::{PrivacyManager, PRIVACY_ACTION_ID};

pub struct AppActionProvider {
    action_service: Arc<Mutex<ActionService>>,
    automations: crate::application::automations::DesktopAutomations,
    output_manager: Arc<OutputManager>,
    notification_store: NotificationStore,
    privacy: Arc<PrivacyManager>,
}

impl AppActionProvider {
    pub fn new(
        action_service: Arc<Mutex<ActionService>>,
        automations: crate::application::automations::DesktopAutomations,
        output_manager: Arc<OutputManager>,
        notification_store: NotificationStore,
        privacy: Arc<PrivacyManager>,
    ) -> Self {
        Self {
            action_service,
            automations,
            output_manager,
            notification_store,
            privacy,
        }
    }
}

#[async_trait::async_trait]
impl ActionProvider for AppActionProvider {
    async fn list_actions(&self) -> Vec<QuickActionInfo> {
        let mut service = lock_unpoison(&self.action_service);
        service.reap_finished_toggles();
        let mut actions = service
            .actions()
            .iter()
            .map(|action| {
                let is_toggle = action.is_toggle();
                let is_running = is_toggle && service.is_toggle_running(&action.id);
                QuickActionInfo {
                    id: action.id.clone(),
                    name: action.name.clone(),
                    icon_id: action.icon_id.clone(),
                    icon_svg: action.icon_svg.clone(),
                    color: action.color.clone(),
                    group: action.group.clone(),
                    action_type_label: action.action_type_label().to_string(),
                    sort_order: action.sort_order,
                    is_toggle,
                    is_running,
                    requires_confirmation: action.requires_confirmation(),
                }
            })
            .collect::<Vec<_>>();
        let privacy = self.privacy.snapshot();
        if privacy.settings.allow_remote_actions {
            actions.push(QuickActionInfo {
                id: PRIVACY_ACTION_ID.to_string(),
                name: "投屏隐私模式".to_string(),
                icon_id: "masks".to_string(),
                icon_svg: include_str!("../../assets/icons/masks.svg").to_string(),
                color: "#5B5FF0".to_string(),
                group: "隐私".to_string(),
                action_type_label: "隐私保护".to_string(),
                sort_order: u32::MAX - 10,
                is_toggle: true,
                is_running: privacy.active,
                requires_confirmation: false,
            });
        }
        actions
    }

    async fn execute_action(&self, action_id: &str) -> HostCapabilityResult<String> {
        if action_id == PRIVACY_ACTION_ID {
            let enabled = self
                .privacy
                .toggle_from_remote()
                .map_err(|error| host_error(HostCapabilityErrorCode::FailedPrecondition, error))?;
            return Ok(if enabled {
                "投屏隐私模式已开启".to_string()
            } else {
                "投屏隐私模式已关闭".to_string()
            });
        }
        if !lock_unpoison(&self.action_service)
            .actions()
            .iter()
            .any(|action| action.id == action_id)
        {
            return Err(HostCapabilityError::new(
                HostCapabilityErrorCode::NotFound,
                format!("quick action {action_id} was not found"),
            ));
        }
        let service = self.action_service.clone();
        let action_id = action_id.to_string();
        tokio::task::spawn_blocking(move || execute_by_id(&service, &action_id))
            .await
            .map_err(|error| host_error(HostCapabilityErrorCode::Internal, error))?
            .map_err(|error| host_error(HostCapabilityErrorCode::FailedPrecondition, error))
    }

    fn subscribe_output(&self) -> Option<broadcast::Receiver<OutputLine>> {
        Some(self.output_manager.subscribe())
    }

    async fn get_action_output(&self, action_id: &str) -> Vec<String> {
        self.output_manager.get_lines(action_id)
    }
}

#[async_trait::async_trait]
impl WorkflowProvider for AppActionProvider {
    async fn list_automations(&self) -> Vec<arcrelay_protocol::server::AutomationInfo> {
        let engine = &self.automations.engine;
        let names = lock_unpoison(&self.action_service)
            .actions()
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        let Ok(definitions) = engine.list().await else {
            return vec![];
        };
        let mut result = vec![];
        for definition in definitions {
            let latest = engine
                .activities(Some(&definition.id), 1)
                .await
                .ok()
                .and_then(|mut a| a.pop());
            let issues = engine.preflight(&definition).await;
            result.push(arcrelay_protocol::server::AutomationInfo {
                id: definition.id.clone(),
                name: definition.name.clone(),
                summary: definition.summary_with_actions(&names),
                enabled: definition.enabled,
                latest_activity_id: latest.as_ref().map(|a| a.id.clone()),
                latest_status: latest.as_ref().map(|a| a.status.key().into()),
                reason: issues
                    .first()
                    .map(|i| i.message.clone())
                    .or_else(|| latest.as_ref().and_then(|a| a.reason.clone())),
                next_run_at_ms: arcrelay_automation::next_schedule(
                    &definition.trigger,
                    chrono::Utc::now(),
                )
                .filter(|_| definition.enabled)
                .map(|t| t.timestamp_millis()),
                completed_steps: latest
                    .as_ref()
                    .map(|a| {
                        a.steps
                            .iter()
                            .filter(|s| s.status == arcrelay_automation::ActivityStatus::Succeeded)
                            .count() as u32
                    })
                    .unwrap_or(0),
                total_steps: definition.steps.len() as u32,
            });
        }
        result
    }
    async fn run_automation(&self, id: &str) -> HostCapabilityResult<String> {
        self.automations
            .engine
            .run(id)
            .await
            .map_err(automation_error)
    }
    async fn set_automation_enabled(&self, id: &str, enabled: bool) -> HostCapabilityResult<()> {
        self.automations
            .set_enabled(id, enabled)
            .await
            .map_err(|error| host_error(HostCapabilityErrorCode::FailedPrecondition, error))
    }
    async fn cancel_automation(&self, id: &str) -> HostCapabilityResult<()> {
        self.automations
            .engine
            .cancel(id)
            .await
            .map_err(automation_error)
    }
}

#[async_trait::async_trait]
impl NotificationProvider for AppActionProvider {
    fn notifications_available(&self) -> bool {
        true
    }

    fn subscribe_notifications(&self) -> Option<broadcast::Receiver<()>> {
        Some(self.notification_store.subscribe())
    }

    async fn list_notifications(
        &self,
        include_read: bool,
        limit: usize,
    ) -> HostCapabilityResult<Vec<HostNotificationInfo>> {
        let store = self.notification_store.clone();
        tokio::task::spawn_blocking(move || store.list(include_read, limit))
            .await
            .map_err(|error| host_error(HostCapabilityErrorCode::Internal, error))?
            .map_err(|error| host_error(HostCapabilityErrorCode::Unavailable, error))
            .map(|notifications| {
                notifications
                    .into_iter()
                    .map(protocol_notification_info)
                    .collect()
            })
    }

    async fn mark_notification_read(
        &self,
        notification_id: &str,
        device_id: &str,
        device_name: &str,
    ) -> HostCapabilityResult<HostNotificationInfo> {
        let store = self.notification_store.clone();
        let notification_id = notification_id.to_string();
        let device_id = device_id.to_string();
        let device_name = device_name.to_string();
        tokio::task::spawn_blocking(move || {
            let exists = store
                .get(&notification_id)
                .map_err(|error| host_error(HostCapabilityErrorCode::Unavailable, error))?
                .is_some();
            if !exists {
                return Err(HostCapabilityError::new(
                    HostCapabilityErrorCode::NotFound,
                    format!("notification {notification_id} was not found"),
                ));
            }
            store
                .mark_read(&notification_id, Some(&device_id), Some(&device_name))
                .map_err(|error| host_error(HostCapabilityErrorCode::Unavailable, error))
        })
        .await
        .map_err(|error| host_error(HostCapabilityErrorCode::Internal, error))?
        .map(protocol_notification_info)
    }
}

fn protocol_notification_info(notification: HostNotification) -> HostNotificationInfo {
    use arcrelay_protocol::proto_msg::proto;
    let kind = match notification.kind {
        NotificationKind::Info => proto::NotificationKind::Info,
        NotificationKind::TaskCompleted => proto::NotificationKind::TaskCompleted,
        NotificationKind::ActionRequired => proto::NotificationKind::ActionRequired,
    } as i32;
    HostNotificationInfo {
        id: notification.id,
        title: notification.title,
        body: notification.body,
        source: notification.source,
        kind,
        reference: notification.reference,
        created_at_ms: notification.created_at_ms,
        read_at_ms: notification.read_at_ms,
        read_by_device_name: notification.read_by_device_name,
    }
}

fn lock_unpoison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|error| error.into_inner())
}

fn host_error(code: HostCapabilityErrorCode, error: impl std::fmt::Display) -> HostCapabilityError {
    HostCapabilityError::new(code, error.to_string())
}

fn automation_error(error: arcrelay_automation::AutomationError) -> HostCapabilityError {
    use arcrelay_automation::AutomationError;
    let code = match &error {
        AutomationError::Invalid(_) => HostCapabilityErrorCode::InvalidArgument,
        AutomationError::Conflict => HostCapabilityErrorCode::Conflict,
        AutomationError::NotFound => HostCapabilityErrorCode::NotFound,
        AutomationError::Database(_) | AutomationError::Json(_) => {
            HostCapabilityErrorCode::Internal
        }
    };
    host_error(code, error)
}
