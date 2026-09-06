use super::{
    error::ToolFailure,
    service::{require, uses_script},
    types::*,
    ArcRelayMcpServer,
};
use crate::{application::configuration, domain::quick_action::QuickAction};
use arcrelay_automation::{
    AutomationDefinition, AutomationEvent, AutomationStep, AutomationTrigger, RunMode,
};
use rmcp::{
    handler::server::wrapper::Parameters, service::RequestContext, tool, tool_router, Json,
    RoleServer,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TestRequest {
    idempotency_key: String,
    automation: AutomationInput,
    sample_activity_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RestoreRequest {
    idempotency_key: String,
    change_id: String,
    /// Selects the saved state before or after this historical change.
    version: RestoreVersion,
    /// Current entity revision, or zero only when the entity no longer exists.
    expected_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum RestoreVersion {
    Before,
    After,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct RestoredOutput {
    action: Option<QuickAction>,
    automation: Option<AutomationDefinition>,
}

fn check_revision(actual: u64, expected: u64) -> Result<(), ToolFailure> {
    if actual == expected {
        Ok(())
    } else {
        Err(ToolFailure::new(
            "configuration.conflict",
            "the configuration changed since it was read",
            "get the latest definition and revision, then submit a new request",
        ))
    }
}

impl ArcRelayMcpServer {
    async fn run_definition(
        &self,
        client: &super::access::McpClientView,
        definition: AutomationDefinition,
        operation_id: String,
        sample: Option<&str>,
    ) -> Result<ActivityOutput, ToolFailure> {
        self.authorize_automation(client, &definition, true).await?;
        arcrelay_automation::validate(&definition)?;
        let engine = &self.host()?.state.automations.engine;
        let issues = engine.preflight(&definition).await;
        if let Some(issue) = issues.first() {
            return Err(ToolFailure::new(&issue.code, &issue.message, &issue.remedy));
        }
        let mut event = if let Some(id) = sample {
            let activity = engine.activity(id).await?;
            if activity.event.kind != definition.trigger.capability() {
                return Err(ToolFailure::invalid(
                    "sampleActivityId",
                    "the sample must contain a real event matching this trigger type",
                ));
            }
            activity.event
        } else {
            AutomationEvent::new("mcp.manual")
        };
        for step in &definition.steps {
            match step {
                AutomationStep::Shell { script, .. } => {
                    arcrelay_automation::render_text(script, &event)?;
                }
                AutomationStep::Notification { title, body, .. } => {
                    arcrelay_automation::render_text(title, &event)?;
                    arcrelay_automation::render_text(body, &event)?;
                }
                _ => {}
            }
        }
        event.id = format!("mcp:{operation_id}");
        event.origin_automation_id = None;
        event
            .variables
            .insert("source.clientId".into(), client.id.clone());
        event
            .variables
            .insert("source.clientName".into(), client.name.clone());
        let id = engine.test(definition, Some(event)).await?;
        let activity = engine.activity(&id).await?;
        Ok(ActivityOutput {
            activity_id: id,
            status: activity.status.key().into(),
            reason: activity.reason,
        })
    }
}

#[tool_router(router = mutation_router, vis = "pub(super)")]
impl ArcRelayMcpServer {
    #[tool(
        description = "Create a reusable quick action. Does not execute it. A globalShortcut registers a trigger and requires enable permission; scripts require scripts permission. Retry with the same idempotencyKey and exact arguments.",
        annotations(destructive_hint = false, idempotent_hint = true)
    )]
    async fn create_quick_action(
        &self,
        Parameters(request): Parameters<CreateActionRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<QuickAction>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "create_quick_action",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                self.save_action(
                    &client,
                    request
                        .action
                        .definition(uuid::Uuid::new_v4().to_string(), 0),
                    true,
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Replace a quick action using its current revision. Rechecks active automation references, scripts and shortcut permissions. Changes affect future runs; existing execution snapshots remain immutable.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn update_quick_action(
        &self,
        Parameters(request): Parameters<UpdateActionRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<QuickAction>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "update_quick_action",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                self.save_action(
                    &client,
                    request
                        .action
                        .definition(request.id.clone(), request.expected_revision),
                    false,
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Delete a quick action only if its revision matches and no automation still references it. Remove references first. Deleting a running toggle also stops its process.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn delete_quick_action(
        &self,
        Parameters(request): Parameters<DeleteRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<MutationOutput<bool>>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "delete_quick_action",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                let host = self.host()?;
                let action = self.action(&request.id)?;
                check_revision(action.revision, request.expected_revision)?;
                if action.global_shortcut.is_some() {
                    require(&client, "enable")?;
                }
                if host
                    .state
                    .action_service
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .is_toggle_running(&request.id)
                {
                    require(&client, "execute")?;
                }
                let references = configuration::action_references(&host.state, &request.id).await?;
                if !references.is_empty() {
                    let mut error = ToolFailure::new(
                        "action.in_use",
                        "automations still reference this action",
                        "update or delete the referencing automations first",
                    );
                    error.details = Some(Box::new(serde_json::to_value(
                        references
                            .into_iter()
                            .map(AutomationSummary::from)
                            .collect::<Vec<_>>(),
                    )?));
                    return Err(error);
                }
                configuration::delete_action_locked(
                    &host.app,
                    &host.state,
                    &request.id,
                    Some(request.expected_revision),
                    &format!("mcp:{}:{}", client.id, client.name),
                )
                .await?;
                Ok(MutationOutput { value: true })
            },
        )
        .await
    }
    #[tool(
        description = "Create an automation from typed triggers, AND conditions and sequential steps. Defaults to disabled. enabled=true requires enable permission and passes the same preflight as desktop saving. Creating does not perform a test run.",
        annotations(destructive_hint = false, idempotent_hint = true)
    )]
    async fn create_automation(
        &self,
        Parameters(request): Parameters<CreateAutomationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<AutomationDefinition>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "create_automation",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                self.save_automation(
                    &client,
                    request
                        .automation
                        .definition(uuid::Uuid::new_v4().to_string(), 0),
                    true,
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Replace an automation with an explicit expectedRevision. Editing an enabled automation needs enable permission; referenced executable content is checked on every save.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn update_automation(
        &self,
        Parameters(request): Parameters<UpdateAutomationRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<AutomationDefinition>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "update_automation",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                self.save_automation(
                    &client,
                    request
                        .automation
                        .definition(request.id.clone(), request.expected_revision),
                    false,
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Delete an automation using its current revision. Deleting an enabled automation also requires enable permission. Already-started activities retain their snapshots; cancel them explicitly if needed.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn delete_automation(
        &self,
        Parameters(request): Parameters<DeleteRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<MutationOutput<bool>>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "delete_automation",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                let host = self.host()?;
                let definition = host.state.automations.engine.get(&request.id).await?;
                if definition.enabled {
                    require(&client, "enable")?;
                }
                configuration::delete_automation_locked(
                    &host.app,
                    &host.state,
                    &request.id,
                    Some(request.expected_revision),
                    &format!("mcp:{}:{}", client.id, client.name),
                )
                .await?;
                Ok(MutationOutput { value: true })
            },
        )
        .await
    }
    #[tool(
        description = "Enable or disable an automation at its current revision. Enabling revalidates platform capabilities, referenced actions, scripts and system shortcut availability. Disabling does not cancel an existing activity.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn set_automation_enabled(
        &self,
        Parameters(request): Parameters<EnableRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<AutomationDefinition>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "set_automation_enabled",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                let host = self.host()?;
                let mut definition = host.state.automations.engine.get(&request.id).await?;
                check_revision(definition.revision, request.expected_revision)?;
                definition.enabled = request.enabled;
                if definition.enabled {
                    self.authorize_automation(&client, &definition, false)
                        .await?;
                }
                Ok(configuration::save_automation_locked(
                    &host.app,
                    &host.state,
                    definition,
                    &format!("mcp:{}:{}", client.id, client.name),
                )
                .await?)
            },
        )
        .await
    }
    #[tool(
        description = "Execute a saved quick action now, using an immutable activity snapshot and its current revision. This has real system effects. Repeated identical keys return the same activity, including for toggle actions. Required confirmations remain local to the desktop.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn run_quick_action(
        &self,
        Parameters(request): Parameters<RunRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ActivityOutput>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "run_quick_action",
            &request.idempotency_key,
            &request,
            |client, operation_id| async move {
                let action = self.action(&request.id)?;
                check_revision(action.revision, request.expected_revision)?;
                if uses_script(&action) {
                    require(&client, "scripts")?;
                }
                let definition = AutomationInput {
                    name: action.name,
                    enabled: false,
                    trigger: AutomationTrigger::Manual,
                    conditions: vec![],
                    steps: vec![AutomationStep::QuickAction {
                        action_id: action.id.clone(),
                    }],
                    run_mode: RunMode::Automatic,
                }
                .definition(format!("mcp-action-{}", action.id), 0);
                self.run_definition(&client, definition, operation_id, None)
                    .await
            },
        )
        .await
    }
    #[tool(
        description = "Run a saved automation's steps now, regardless of enabled state, trigger and conditions. This is a manual execution with real side effects, not a trigger simulation. For event variables, choose a real matching activity sample. Returns an activity ID; never confirms on the user's behalf.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn run_automation(
        &self,
        Parameters(request): Parameters<RunRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ActivityOutput>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "run_automation",
            &request.idempotency_key,
            &request,
            |client, operation_id| async move {
                let definition = self
                    .host()?
                    .state
                    .automations
                    .engine
                    .get(&request.id)
                    .await?;
                check_revision(definition.revision, request.expected_revision)?;
                self.run_definition(
                    &client,
                    definition,
                    operation_id,
                    request.sample_activity_id.as_deref(),
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Execute an unsaved automation's steps with REAL side effects. Does not save or enable it, and ignores trigger/conditions. Use validate_automation for a side-effect-free check. Local confirmations are preserved, and event variables need a real matching activity sample.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn test_automation(
        &self,
        Parameters(request): Parameters<TestRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ActivityOutput>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "test_automation",
            &request.idempotency_key,
            &request,
            |client, operation_id| async move {
                let mut definition = request
                    .automation
                    .definition(format!("mcp-test-{operation_id}"), 0);
                definition.enabled = false;
                self.run_definition(
                    &client,
                    definition,
                    operation_id,
                    request.sample_activity_id.as_deref(),
                )
                .await
            },
        )
        .await
    }
    #[tool(
        description = "Cancel an activity or dismiss its pending confirmation. Stops cancellable shell work and prevents later steps; completed system side effects cannot be undone.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn cancel_activity(
        &self,
        Parameters(request): Parameters<CancelRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<ActivityOutput>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "cancel_activity",
            &request.idempotency_key,
            &request,
            |_, _| async move {
                let engine = &self.host()?.state.automations.engine;
                engine.cancel(&request.id).await?;
                let activity = engine.activity(&request.id).await?;
                Ok(ActivityOutput {
                    activity_id: activity.id,
                    status: activity.status.key().into(),
                    reason: activity.reason,
                })
            },
        )
        .await
    }
    #[tool(
        description = "Restore a before/after definition from change history as a new revision. Deleted entities receive a new ID to prevent stale revisions from matching. Restored automations are disabled and restored actions have no global shortcut; enable/bind separately. Rechecks scripts, dependencies and current revision. It never replays execution or silently overwrites a concurrent edit.",
        annotations(destructive_hint = true, idempotent_hint = true)
    )]
    async fn restore_configuration(
        &self,
        Parameters(request): Parameters<RestoreRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<Json<RestoredOutput>, ToolFailure> {
        let request = &request;
        self.mutate(
            &context,
            "restore_configuration",
            &request.idempotency_key,
            &request,
            |client, _| async move {
                let change = self
                    .host()?
                    .state
                    .mcp_journal
                    .change(&request.change_id)
                    .await?;
                if change.status != "applied" {
                    return Err(ToolFailure::invalid(
                        "changeId",
                        "choose a successfully applied change",
                    ));
                }
                let value = match request.version {
                    RestoreVersion::Before => change.before,
                    RestoreVersion::After => change.after,
                }
                .ok_or_else(|| {
                    ToolFailure::invalid(
                        "version",
                        "this side of the change has no definition to restore",
                    )
                })?;
                if change.entity == "quickAction" {
                    let mut action: QuickAction = serde_json::from_value(value)?;
                    let current = self
                        .host()?
                        .state
                        .action_service
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .find_cloned(&action.id);
                    if let Some(current) = &current {
                        check_revision(current.revision, request.expected_revision)?;
                    } else {
                        check_revision(0, request.expected_revision)?;
                    }
                    if current.is_none() {
                        action.id = uuid::Uuid::new_v4().to_string();
                    }
                    action.revision = request.expected_revision;
                    action.global_shortcut = None;
                    Ok(RestoredOutput {
                        action: Some(self.save_action(&client, action, current.is_none()).await?),
                        automation: None,
                    })
                } else if change.entity == "automation" {
                    let mut definition: AutomationDefinition = serde_json::from_value(value)?;
                    let creating = match self
                        .host()?
                        .state
                        .automations
                        .engine
                        .get(&definition.id)
                        .await
                    {
                        Ok(current) => {
                            check_revision(current.revision, request.expected_revision)?;
                            false
                        }
                        Err(arcrelay_automation::AutomationError::NotFound) => {
                            check_revision(0, request.expected_revision)?;
                            true
                        }
                        Err(error) => return Err(error.into()),
                    };
                    if creating {
                        definition.id = uuid::Uuid::new_v4().to_string();
                    }
                    definition.revision = request.expected_revision;
                    definition.enabled = false;
                    Ok(RestoredOutput {
                        action: None,
                        automation: Some(
                            self.save_automation(&client, definition, creating).await?,
                        ),
                    })
                } else {
                    Err(ToolFailure::invalid(
                        "changeId",
                        "unsupported configuration entity",
                    ))
                }
            },
        )
        .await
    }
}
