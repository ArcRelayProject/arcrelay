use super::{
    error::ToolFailure,
    service::{require, uses_script},
    types::*,
    ArcRelayMcpServer,
};
use crate::{application::configuration, domain::quick_action::QuickAction};
use arcrelay_automation::{AutomationDefinition, AutomationStep, AutomationTrigger, RunMode};
use rmcp::{
    handler::server::wrapper::Parameters, service::RequestContext, tool, tool_router, Json,
    RoleServer,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundleAction {
    /// Unique local key, e.g. editor. Reference it in steps with actionId="$editor".
    key: String,
    action: ActionInput,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BundleInput {
    #[serde(default)]
    actions: Vec<BundleAction>,
    #[serde(default)]
    automations: Vec<AutomationInput>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ApplyBundleRequest {
    idempotency_key: String,
    bundle: BundleInput,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct BundlePlan {
    valid: bool,
    issues: Vec<ToolFailure>,
    action_ids: BTreeMap<String, String>,
    actions: Vec<QuickAction>,
    automations: Vec<AutomationDefinition>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct BundleOutput {
    actions: Vec<QuickAction>,
    automations: Vec<AutomationDefinition>,
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct Template {
    id: String,
    name: String,
    description: String,
    required_inputs: Vec<String>,
    automation: AutomationInput,
}
#[derive(Debug, Serialize, JsonSchema)]
struct TemplatesOutput {
    items: Vec<Template>,
}

fn resolve_bundle(bundle: &BundleInput) -> Result<BundlePlan, ToolFailure> {
    if bundle.actions.len() > 16
        || bundle.automations.len() > 16
        || (bundle.actions.is_empty() && bundle.automations.is_empty())
    {
        return Err(ToolFailure::invalid(
            "bundle",
            "include 1–16 actions and/or 1–16 automations",
        ));
    }
    if serde_json::to_vec(bundle)?.len() > 2 * 1024 * 1024 {
        return Err(ToolFailure::invalid("bundle", "bundle exceeds 2 MiB"));
    }
    let mut action_ids = BTreeMap::new();
    let mut actions = vec![];
    for entry in &bundle.actions {
        if entry.key.is_empty()
            || entry.key.len() > 64
            || !entry
                .key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ToolFailure::invalid(
                "actions.key",
                "use a key of 1–64 ASCII letters, digits, underscores or hyphens",
            ));
        }
        if entry.action.global_shortcut.is_some() {
            return Err(ToolFailure::invalid("actions.globalShortcut", "bundles create manual actions; bind global shortcuts after the entire bundle succeeds"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        if action_ids.insert(entry.key.clone(), id.clone()).is_some() {
            return Err(ToolFailure::invalid(
                "actions.key",
                "bundle action keys must be unique",
            ));
        }
        let mut action = entry.action.definition(id, 0);
        crate::application::action_service::normalize_action(&mut action)?;
        actions.push(action);
    }
    let mut automations = vec![];
    for input in &bundle.automations {
        if input.enabled {
            return Err(ToolFailure::invalid(
                "automations.enabled",
                "bundles create disabled automations; enable them after the entire bundle succeeds",
            ));
        }
        let mut definition = input.definition(uuid::Uuid::new_v4().to_string(), 0);
        for step in &mut definition.steps {
            if let AutomationStep::QuickAction { action_id } = step {
                if let Some(key) = action_id.strip_prefix('$') {
                    *action_id = action_ids.get(key).cloned().ok_or_else(|| {
                        ToolFailure::invalid(
                            "steps.actionId",
                            format!("unknown bundle action key: {key}"),
                        )
                    })?;
                }
            }
        }
        arcrelay_automation::validate(&definition)?;
        if definition.name.is_empty() {
            return Err(ToolFailure::invalid("name", "enter an automation name"));
        }
        automations.push(definition);
    }
    Ok(BundlePlan {
        valid: true,
        issues: vec![],
        action_ids,
        actions,
        automations,
    })
}

impl ArcRelayMcpServer {
    async fn check_bundle(
        &self,
        bundle: &BundleInput,
        client: &super::access::McpClientView,
    ) -> Result<BundlePlan, ToolFailure> {
        let mut plan = resolve_bundle(bundle)?;
        if !client.permissions.manage {
            plan.issues.push(ToolFailure::denied("manage"));
        }
        for action in &plan.actions {
            plan.issues.extend(self.action_issues(action).await);
            if uses_script(action) && !client.permissions.scripts {
                plan.issues.push(ToolFailure::denied("scripts"));
            }
        }
        for definition in &plan.automations {
            for issue in self
                .host()?
                .state
                .automations
                .engine
                .preflight(definition)
                .await
            {
                let planned_reference = issue.code == "action.missing" && issue.step_index.is_some_and(|index| {
                    matches!(definition.steps.get(index), Some(AutomationStep::QuickAction { action_id }) if plan.actions.iter().any(|action| &action.id == action_id))
                });
                if !planned_reference {
                    plan.issues
                        .push(ToolFailure::new(&issue.code, issue.message, &issue.remedy));
                }
            }
            for step in &definition.steps {
                let scripted = match step {
                    AutomationStep::Shell { .. } => true,
                    AutomationStep::QuickAction { action_id } => plan
                        .actions
                        .iter()
                        .find(|a| &a.id == action_id)
                        .cloned()
                        .or_else(|| self.action(action_id).ok())
                        .is_some_and(|a| uses_script(&a)),
                    _ => false,
                };
                if scripted && !client.permissions.scripts {
                    plan.issues.push(ToolFailure::denied("scripts"));
                }
            }
        }
        plan.valid = plan.issues.is_empty();
        Ok(plan)
    }
}

#[tool_router(router = bundle_router, vis = "pub(super)")]
impl ArcRelayMcpServer {
    #[tool(
        description = "Preview creation of related quick actions and disabled automations without changing anything. Reference bundle action keys as $key in actionId. IDs shown are preview-only. Bundles do not activate triggers or bind shortcuts.",
        annotations(read_only_hint = true)
    )]
    async fn preview_configuration_bundle(
        &self,
        Parameters(request): Parameters<BundleInput>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<BundlePlan>, ToolFailure> {
        Ok(Json(
            self.check_bundle(&request, &self.caller(&context)?).await?,
        ))
    }
    #[tool(
        description = "Create a related bundle of manual quick actions and disabled automations, resolving $key action references. Revalidates the entire bundle before writing. On failure attempts compensating deletion of only the created revisions; reports any incomplete cleanup. This is not a cross-store atomic transaction. Enable automations separately after success.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn apply_configuration_bundle(
        &self,
        Parameters(request): Parameters<ApplyBundleRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<BundleOutput>, ToolFailure> {
        let request = &request;
        self.mutate(&context, "apply_configuration_bundle", &request.idempotency_key, request, |client, _| async move {
            require(&client, "manage")?;
            let plan = self.check_bundle(&request.bundle, &client).await?;
            if !plan.valid {
                let mut error = ToolFailure::invalid("bundle", "bundle validation failed; no configurations were written");
                error.details = Some(Box::new(serde_json::to_value(plan.issues)?));
                return Err(error);
            }
            let result = async {
                let mut output = BundleOutput { actions: vec![], automations: vec![] };
                for action in &plan.actions { output.actions.push(self.save_action(&client, action.clone(), true).await?); }
                for definition in &plan.automations { output.automations.push(self.save_automation(&client, definition.clone(), true).await?); }
                Ok::<_, ToolFailure>(output)
            }.await;
            match result {
                Ok(output) => Ok(output),
                Err(mut error) => {
                    let host = self.host()?;
                    let actor = format!("mcp:{}:{} rollback", client.id, client.name);
                    let mut rollback_errors = vec![];
                    // Read every planned ID, including writes whose result journaling failed.
                    for definition in plan.automations.iter().rev() {
                        match host.state.automations.engine.get(&definition.id).await {
                            Ok(_) => if let Err(e) = configuration::delete_automation_locked(&host.app, &host.state, &definition.id, Some(1), &actor).await { rollback_errors.push(format!("{}: {e}", definition.id)); },
                            Err(arcrelay_automation::AutomationError::NotFound) => {},
                            Err(e) => rollback_errors.push(format!("{}: {e}", definition.id)),
                        }
                    }
                    for action in plan.actions.iter().rev() {
                        if self.action(&action.id).is_ok() {
                            if let Err(e) = configuration::delete_action_locked(&host.app, &host.state, &action.id, Some(1), &actor).await { rollback_errors.push(format!("{}: {e}", action.id)); }
                        }
                    }
                    error.details = Some(Box::new(serde_json::json!({"rollbackComplete":rollback_errors.is_empty(),"rollbackErrors":rollback_errors,"plannedActionIds":plan.action_ids,"plannedAutomationIds":plan.automations.iter().map(|d| &d.id).collect::<Vec<_>>()})));
                    error.remedy = "inspect cleanup details and change history before creating another bundle".into();
                    Err(error)
                }
            }
        }).await
    }
    #[tool(
        description = "Get typed starter templates for common local automations. Fill requiredInputs from actual application/device discovery, validate, then create. Templates remain disabled until explicitly enabled.",
        annotations(read_only_hint = true)
    )]
    async fn list_automation_templates(&self) -> Result<Json<TemplatesOutput>, ToolFailure> {
        let timezone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".into());
        let notification = |title: &str, body: &str| AutomationStep::Notification {
            title: title.into(),
            body: body.into(),
            send_to_connected_devices: false,
        };
        let input = |name: &str, trigger, steps| AutomationInput {
            name: name.into(),
            enabled: false,
            trigger,
            conditions: vec![],
            steps,
            run_mode: RunMode::Automatic,
        };
        Ok(Json(TemplatesOutput { items: vec![
            Template { id: "weekday-start".into(), name: "工作日开工".into(), description: "Weekday morning starter; insert discovered app/path quick actions before the notification.".into(), required_inputs: vec![], automation: input("工作日开工", AutomationTrigger::Schedule { time: "09:00".into(), weekdays: vec![1,2,3,4,5], timezone, catch_up: false }, vec![notification("开始工作", "工作日自动化已运行")]) },
            Template { id: "meeting-privacy".into(), name: "会议隐私".into(), description: "Enable presentation privacy when a selected meeting app becomes foreground.".into(), required_inputs: vec!["trigger.apps (use list_installed_apps)".into()], automation: input("会议隐私", AutomationTrigger::Application { event: arcrelay_automation::ApplicationEvent::Foreground, apps: vec![] }, vec![AutomationStep::QuickAction { action_id: "builtin-automation-privacy-on".into() }]) },
            Template { id: "received-files".into(), name: "收到文件".into(), description: "Notify when ArcRelay finishes receiving files; optionally select a device and add processing actions.".into(), required_inputs: vec![], automation: input("收到文件", AutomationTrigger::Transfer { received: true, device_ids: vec![], file_kinds: vec![] }, vec![notification("收到文件", "来自 {{event.device.name}} 的 {{event.file.count}} 个文件已保存到 {{event.file.directory}}")]) },
        ] }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bundle() -> BundleInput {
        serde_json::from_value(serde_json::json!({"actions":[{"key":"folder","action":{"name":"Folder","actionType":{"type":"OpenPath","path":"/tmp"}}}],"automations":[{"name":"Start","trigger":{"type":"manual"},"steps":[{"type":"quickAction","actionId":"$folder"}],"runMode":"automatic"}]})).unwrap()
    }
    #[test]
    fn references_resolve_and_activation_is_never_implicit() {
        let mut input = bundle();
        let plan = resolve_bundle(&input).unwrap();
        assert!(
            matches!(&plan.automations[0].steps[0], AutomationStep::QuickAction { action_id } if action_id == &plan.actions[0].id)
        );
        assert!(!plan.automations[0].enabled);
        assert_eq!(plan.actions[0].revision, 0);
        input.automations[0].enabled = true;
        assert!(resolve_bundle(&input).is_err());
        input.automations[0].enabled = false;
        input.actions[0].action.global_shortcut = Some("Ctrl+K".into());
        assert!(resolve_bundle(&input).is_err());
    }
    #[test]
    fn unknown_and_duplicate_keys_fail_before_any_write() {
        let mut input = bundle();
        input.automations[0].steps = vec![AutomationStep::QuickAction {
            action_id: "$unknown".into(),
        }];
        assert!(resolve_bundle(&input).is_err());
        let mut input = bundle();
        input.actions.push(input.actions[0].clone());
        assert!(resolve_bundle(&input).is_err());
    }
}
