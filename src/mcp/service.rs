use super::{
    access::{Credential, McpClientView, McpPermissions},
    error::{ToolFailure, ToolResult},
    journal::{Reservation, StoredReply},
    types::*,
};
use crate::{
    application::{action_service::normalize_action, configuration},
    backend::DesktopState,
    domain::quick_action::{ActionType, QuickAction},
};
use arcrelay_automation::{AutomationDefinition, AutomationStep};
use rmcp::{service::RequestContext, Json, RoleServer};
use serde::{de::DeserializeOwned, Serialize};
use tauri::AppHandle;

#[derive(Clone)]
pub struct McpHost {
    pub state: DesktopState,
    pub app: AppHandle,
}

pub fn tool_permission(name: &str) -> Option<&'static str> {
    match name {
        "get_capabilities" => Some("authenticated"),
        "send_notification" | "list_notifications" | "get_notification_status" => {
            Some("notifications")
        }
        "create_quick_action"
        | "update_quick_action"
        | "delete_quick_action"
        | "create_automation"
        | "update_automation"
        | "delete_automation"
        | "restore_configuration"
        | "apply_configuration_bundle" => Some("manage"),
        "set_automation_enabled" => Some("enable"),
        "run_quick_action" | "run_automation" | "test_automation" | "cancel_activity" => {
            Some("execute")
        }
        "list_quick_actions"
        | "get_quick_action"
        | "list_automations"
        | "get_automation"
        | "list_installed_apps"
        | "list_devices"
        | "list_action_presets"
        | "list_automation_templates"
        | "validate_quick_action"
        | "validate_automation"
        | "preview_schedule"
        | "preview_configuration_bundle"
        | "get_activity"
        | "list_activities"
        | "list_configuration_changes"
        | "get_configuration_change" => Some("read"),
        _ => None,
    }
}

pub fn require(client: &McpClientView, permission: &str) -> Result<(), ToolFailure> {
    if permission == "authenticated" || client.permissions.allows(permission) {
        Ok(())
    } else {
        Err(ToolFailure::denied(permission))
    }
}

pub fn uses_script(action: &QuickAction) -> bool {
    match &action.action_type {
        ActionType::ShellCommand { .. }
        | ActionType::ToggleShellCommand { .. }
        | ActionType::AppleScript { .. } => true,
        ActionType::OpenUrl { url } => !url.starts_with("https://") && !url.starts_with("http://"),
        _ => false,
    }
}

pub fn action_permissions(
    action: &QuickAction,
    previous: Option<&QuickAction>,
    references: &[AutomationDefinition],
) -> Vec<&'static str> {
    let mut required = vec!["manage"];
    if uses_script(action) || previous.is_some_and(uses_script) {
        required.push("scripts");
    }
    if action.global_shortcut.is_some()
        || previous.is_some_and(|a| a.global_shortcut.is_some())
        || references.iter().any(|d| d.enabled)
    {
        required.push("enable");
    }
    required
}

impl super::ArcRelayMcpServer {
    pub(super) fn host(&self) -> Result<&McpHost, ToolFailure> {
        self.host.as_ref().ok_or_else(|| {
            ToolFailure::new(
                "service.unavailable",
                "desktop management is not initialized",
                "start ArcRelay and retry",
            )
        })
    }
    pub(super) fn caller(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<McpClientView, ToolFailure> {
        let credential = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Credential>())
            .ok_or_else(|| ToolFailure::denied("authenticated"))?;
        self.access
            .resolve(credential)
            .map_err(|_| ToolFailure::denied("authenticated"))
    }

    pub(super) async fn mutate<T, R, F, Fut>(
        &self,
        context: &RequestContext<RoleServer>,
        name: &str,
        key: &str,
        request: &R,
        operation: F,
    ) -> ToolResult<T>
    where
        T: Serialize + DeserializeOwned,
        R: Serialize,
        F: FnOnce(McpClientView, String) -> Fut,
        Fut: std::future::Future<Output = Result<T, ToolFailure>>,
    {
        let _guard = crate::action_shortcuts::MUTATION.lock().await;
        let client = self.caller(context)?;
        require(
            &client,
            tool_permission(name).ok_or_else(|| ToolFailure::denied("unknown tool"))?,
        )?;
        let journal = &self.host()?.state.mcp_journal;
        let id = match journal.reserve(&client, key, name, request).await? {
            Reservation::New(id) => id,
            Reservation::Replay(reply) => {
                return match reply.error {
                    Some(error) => Err(error),
                    None => Ok(Json(serde_json::from_value(reply.value.ok_or_else(
                        || {
                            ToolFailure::new(
                                "storage.invalid",
                                "stored operation has no result",
                                "inspect change history",
                            )
                        },
                    )?)?)),
                }
            }
        };
        let result = operation(client, id.clone()).await;
        let reply = match &result {
            Ok(value) => StoredReply {
                value: Some(serde_json::to_value(value)?),
                error: None,
            },
            Err(error) => StoredReply {
                value: None,
                error: Some(error.clone()),
            },
        };
        journal.finish(&id, &reply).await?;
        result.map(Json)
    }

    pub(super) fn action(&self, id: &str) -> Result<QuickAction, ToolFailure> {
        self.host()?
            .state
            .action_service
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find_cloned(id)
            .ok_or_else(|| {
                ToolFailure::new(
                    "action.not_found",
                    "quick action not found",
                    "list quick actions and choose an existing ID",
                )
            })
    }

    pub(super) async fn authorize_automation(
        &self,
        client: &McpClientView,
        definition: &AutomationDefinition,
        execution: bool,
    ) -> Result<(), ToolFailure> {
        if definition.enabled && !execution {
            require(client, "enable")?;
        }
        for step in &definition.steps {
            match step {
                AutomationStep::Shell { .. } => require(client, "scripts")?,
                AutomationStep::QuickAction { action_id }
                    if !action_id.starts_with("builtin-automation-privacy-")
                        && uses_script(&self.action(action_id)?) =>
                {
                    require(client, "scripts")?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) async fn save_action(
        &self,
        client: &McpClientView,
        mut action: QuickAction,
        creating: bool,
    ) -> Result<QuickAction, ToolFailure> {
        normalize_action(&mut action)?;
        let host = self.host()?;
        let previous = host
            .state
            .action_service
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .find_cloned(&action.id);
        if let Some(previous) = &previous {
            action.source_preset_id = previous.source_preset_id.clone();
            // MCP cannot remove a local per-action confirmation requirement.
            action.confirm_before_run |= previous.confirm_before_run;
        }
        if creating == previous.is_some() {
            return Err(ToolFailure::new(
                "action.conflict",
                "the target was created or deleted",
                "read the current action and retry with a new key",
            ));
        }
        let references = configuration::action_references(&host.state, &action.id).await?;
        for permission in action_permissions(&action, previous.as_ref(), &references) {
            require(client, permission)?;
        }
        let issues = self.action_issues(&action).await;
        if !issues.is_empty() {
            return Err(issues[0].clone());
        }
        Ok(configuration::save_action_locked(
            &host.app,
            &host.state,
            action,
            &format!("mcp:{}:{}", client.id, client.name),
        )
        .await?)
    }

    pub(super) async fn save_automation(
        &self,
        client: &McpClientView,
        definition: AutomationDefinition,
        creating: bool,
    ) -> Result<AutomationDefinition, ToolFailure> {
        if definition.name.trim().is_empty() {
            return Err(ToolFailure::invalid("name", "enter an automation name"));
        }
        let host = self.host()?;
        match host.state.automations.engine.get(&definition.id).await {
            Ok(previous) => {
                if creating {
                    return Err(ToolFailure::new(
                        "automation.conflict",
                        "automation already exists",
                        "use update_automation with its current revision",
                    ));
                }
                if previous.enabled {
                    require(client, "enable")?;
                }
                if previous.run_mode == arcrelay_automation::RunMode::AskBeforeRun
                    && definition.run_mode == arcrelay_automation::RunMode::Automatic
                {
                    require(client, "enable")?;
                }
            }
            Err(arcrelay_automation::AutomationError::NotFound) if creating => {}
            Err(error) => return Err(error.into()),
        }
        self.authorize_automation(client, &definition, false)
            .await?;
        Ok(configuration::save_automation_locked(
            &host.app,
            &host.state,
            definition,
            &format!("mcp:{}:{}", client.id, client.name),
        )
        .await?)
    }

    pub(super) async fn action_issues(&self, action: &QuickAction) -> Vec<ToolFailure> {
        let mut issues = vec![];
        let mut normalized = action.clone();
        if let Err(error) = normalize_action(&mut normalized) {
            issues.push(error.into());
            return issues;
        }
        if action.name.len() > 256
            || action.group.len() > 256
            || serde_json::to_vec(action).is_ok_and(|bytes| bytes.len() > 128 * 1024)
        {
            issues.push(ToolFailure::invalid(
                "action",
                "name and group are limited to 256 bytes; a quick action is limited to 128 KiB",
            ));
        }
        if let ActionType::OpenUrl { url } = &action.action_type {
            if let Err(error) = crate::infrastructure::executor::validate_external_url(url) {
                issues.push(ToolFailure::invalid("actionType.url", error));
            }
        }
        let Ok(host) = self.host() else { return issues };
        {
            let service = host
                .state
                .action_service
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Err(error) = crate::action_shortcuts::validate_binding(
                &normalized,
                service.actions(),
                &host.state.settings.snapshot(),
            ) {
                issues.push(ToolFailure::invalid("globalShortcut", error));
            }
        }
        let path = match &action.action_type {
            ActionType::LaunchApp { app_path, .. } => app_path.as_deref(),
            ActionType::OpenPath { path } => Some(path.as_str()),
            ActionType::ShellCommand { working_dir, .. }
            | ActionType::ToggleShellCommand { working_dir, .. } => working_dir.as_deref(),
            _ => None,
        };
        if let Some(path) = path {
            match crate::infrastructure::executor::expand_path(path) {
                Ok(path) if path.exists() && (!matches!(action.action_type, ActionType::ShellCommand { .. } | ActionType::ToggleShellCommand { .. }) || path.is_dir()) => {}
                _ => issues.push(ToolFailure::invalid(
                    "actionType",
                    "the selected local path does not exist or the working directory is not a directory",
                )),
            }
        }
        let required = match &action.action_type {
            ActionType::AppleScript { .. } => Some("action.applescript"),
            ActionType::Hotkey { .. } => Some("input.simulate"),
            ActionType::System {
                operation:
                    crate::domain::quick_action::SystemOperation::ScreenshotFull
                    | crate::domain::quick_action::SystemOperation::ScreenshotRegion,
            } => Some("screen.capture"),
            _ => None,
        };
        if let Some(required) = required {
            let capabilities = host.state.automations.engine.capabilities().await;
            if !capabilities.iter().any(|c| c.id == required && c.available) {
                let capability = capabilities.iter().find(|c| c.id == required);
                issues.push(ToolFailure::new(
                    "capability.unavailable",
                    capability
                        .and_then(|c| c.reason.clone())
                        .unwrap_or_else(|| format!("{required} is unavailable")),
                    capability
                        .and_then(|c| c.remedy.as_deref())
                        .unwrap_or("check system permissions"),
                ));
            }
        }
        issues
    }

    pub(super) async fn automation_validation(
        &self,
        definition: &AutomationDefinition,
        permissions: McpPermissions,
    ) -> Result<ValidationOutput, ToolFailure> {
        let host = self.host()?;
        let mut issues: Vec<_> = host
            .state
            .automations
            .engine
            .preflight(definition)
            .await
            .into_iter()
            .map(|issue| {
                let mut error = ToolFailure::new(&issue.code, issue.message, &issue.remedy);
                error.field = issue.step_index.map(|i| format!("steps[{i}]"));
                error
            })
            .collect();
        let mut required = vec!["manage".to_string()];
        let previous = match host.state.automations.engine.get(&definition.id).await {
            Ok(previous) => Some(previous),
            Err(arcrelay_automation::AutomationError::NotFound) => None,
            Err(error) => return Err(error.into()),
        };
        if definition.enabled
            || previous.is_some_and(|old| {
                old.enabled
                    || (old.run_mode == arcrelay_automation::RunMode::AskBeforeRun
                        && definition.run_mode == arcrelay_automation::RunMode::Automatic)
            })
        {
            required.push("enable".into());
        }
        if let arcrelay_automation::AutomationTrigger::Hotkey { shortcut } = &definition.trigger {
            if let Err(error) = crate::action_shortcuts::normalize(Some(shortcut)) {
                issues.push(ToolFailure::invalid("trigger.shortcut", error));
            }
            if definition.enabled {
                let service = host
                    .state
                    .action_service
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if let Err(error) = crate::action_shortcuts::validate_shortcut(
                    &definition.id,
                    Some(shortcut),
                    service.actions(),
                    &host.state.settings.snapshot(),
                ) {
                    issues.push(ToolFailure::invalid("trigger.shortcut", error));
                }
            }
        }
        for step in &definition.steps {
            let scripted = matches!(step, AutomationStep::Shell { .. })
                || matches!(step, AutomationStep::QuickAction { action_id } if self.action(action_id).is_ok_and(|action| uses_script(&action)));
            if scripted && !required.iter().any(|p| p == "scripts") {
                required.push("scripts".into());
            }
        }
        if definition.name.trim().is_empty() {
            issues.push(ToolFailure::invalid("name", "enter an automation name"));
        }
        for p in &required {
            if !permissions.allows(p) {
                issues.push(ToolFailure::denied(p));
            }
        }
        Ok(ValidationOutput {
            valid: issues.is_empty(),
            issues,
            required_permissions: required,
            summary: definition.summary(),
            next_scheduled_at: arcrelay_automation::next_schedule(
                &definition.trigger,
                chrono::Utc::now(),
            )
            .map(|at| at.to_rfc3339()),
            available_variables: arcrelay_automation::available_variables(&definition.trigger)
                .into_iter()
                .map(str::to_string)
                .collect(),
            affected_automations: vec![],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_action_edits_and_scripts_require_separate_grants() {
        let action = QuickAction::new(
            "Command".into(),
            "Test".into(),
            ActionType::ShellCommand {
                command: "echo example".into(),
                working_dir: None,
            },
        );
        let mut definition =
            AutomationDefinition::new(arcrelay_automation::AutomationTrigger::Manual);
        definition.enabled = true;
        let required = action_permissions(&action, None, &[definition]);
        assert!(required.contains(&"scripts"));
        assert!(required.contains(&"enable"));
        assert_eq!(tool_permission("confirm_automation_activity"), None);
    }
}
