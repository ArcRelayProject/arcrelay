use crate::domain::quick_action::{ActionType, QuickAction};
use arcrelay_automation::{
    AutomationCondition, AutomationDefinition, AutomationStep, AutomationTrigger, RunMode,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActionInput {
    pub name: String,
    #[serde(default = "default_icon")]
    pub icon_id: String,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub sort_order: u32,
    #[serde(default)]
    pub confirm_before_run: bool,
    /// Registers a local keyboard trigger when saved. Omit for a manual action.
    pub global_shortcut: Option<String>,
    pub action_type: ActionType,
}
fn default_icon() -> String {
    "zap".into()
}
impl ActionInput {
    pub fn definition(&self, id: String, revision: u64) -> QuickAction {
        QuickAction {
            id,
            revision,
            name: self.name.clone(),
            icon_id: self.icon_id.clone(),
            icon_svg: String::new(),
            color: self.color.clone(),
            group: self.group.clone(),
            sort_order: self.sort_order,
            source_preset_id: None,
            confirm_before_run: self.confirm_before_run,
            global_shortcut: self.global_shortcut.clone(),
            action_type: self.action_type.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationInput {
    pub name: String,
    /// Defaults to disabled. Enabling requires a separate client permission and preflight.
    #[serde(default)]
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    #[serde(default)]
    pub conditions: Vec<AutomationCondition>,
    pub steps: Vec<AutomationStep>,
    pub run_mode: RunMode,
}
impl AutomationInput {
    pub fn definition(&self, id: String, revision: u64) -> AutomationDefinition {
        AutomationDefinition {
            id,
            revision,
            name: self.name.trim().into(),
            enabled: self.enabled,
            trigger: self.trigger.clone(),
            conditions: self.conditions.clone(),
            steps: self.steps.clone(),
            run_mode: self.run_mode,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            legacy_id: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListRequest {
    pub query: Option<String>,
    /// Zero-based offset; results are capped at 100 entries.
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}
impl ListRequest {
    pub fn matches(&self, name: &str) -> bool {
        self.query
            .as_ref()
            .is_none_or(|q| name.to_lowercase().contains(&q.to_lowercase()))
    }
    pub fn limit(&self) -> usize {
        self.limit.unwrap_or(30).clamp(1, 100)
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IdRequest {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateActionRequest {
    pub idempotency_key: String,
    pub action: ActionInput,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateActionRequest {
    pub idempotency_key: String,
    pub id: String,
    pub expected_revision: u64,
    pub action: ActionInput,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateAutomationRequest {
    pub idempotency_key: String,
    pub automation: AutomationInput,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateAutomationRequest {
    pub idempotency_key: String,
    pub id: String,
    pub expected_revision: u64,
    pub automation: AutomationInput,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeleteRequest {
    pub idempotency_key: String,
    pub id: String,
    pub expected_revision: u64,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnableRequest {
    pub idempotency_key: String,
    pub id: String,
    pub expected_revision: u64,
    pub enabled: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunRequest {
    pub idempotency_key: String,
    pub id: String,
    pub expected_revision: u64,
    /// Optional existing activity whose real event variables are used for a manual run.
    pub sample_activity_id: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelRequest {
    pub idempotency_key: String,
    pub id: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivityRequest {
    pub automation_id: Option<String>,
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScheduleRequest {
    pub trigger: AutomationTrigger,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateActionRequest {
    pub action: ActionInput,
    pub existing_id: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidateAutomationRequest {
    pub automation: AutomationInput,
    pub existing_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionSummary {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub group: String,
    pub action_type: String,
    pub global_shortcut: Option<String>,
    pub requires_confirmation: bool,
}
impl From<QuickAction> for ActionSummary {
    fn from(a: QuickAction) -> Self {
        Self {
            requires_confirmation: a.requires_confirmation(),
            action_type: a.action_type_label().into(),
            id: a.id,
            name: a.name,
            revision: a.revision,
            group: a.group,
            global_shortcut: a.global_shortcut,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSummary {
    pub id: String,
    pub name: String,
    pub revision: u64,
    pub enabled: bool,
    pub summary: String,
    pub next_scheduled_at: Option<String>,
}
impl From<AutomationDefinition> for AutomationSummary {
    fn from(d: AutomationDefinition) -> Self {
        Self {
            summary: d.summary(),
            next_scheduled_at: arcrelay_automation::next_schedule(&d.trigger, chrono::Utc::now())
                .map(|at| at.to_rfc3339()),
            id: d.id,
            name: d.name,
            revision: d.revision,
            enabled: d.enabled,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_offset: Option<usize>,
}
pub fn page<T>(items: impl IntoIterator<Item = T>, request: &ListRequest) -> Page<T> {
    let mut items: Vec<_> = items
        .into_iter()
        .skip(request.offset)
        .take(request.limit() + 1)
        .collect();
    let next_offset =
        (items.len() > request.limit()).then_some(request.offset.saturating_add(request.limit()));
    items.truncate(request.limit());
    Page { items, next_offset }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ValidationOutput {
    pub valid: bool,
    pub issues: Vec<super::error::ToolFailure>,
    pub required_permissions: Vec<String>,
    pub summary: String,
    pub next_scheduled_at: Option<String>,
    pub available_variables: Vec<String>,
    pub affected_automations: Vec<AutomationSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MutationOutput<T> {
    pub value: T,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityOutput {
    pub activity_id: String,
    pub status: String,
    pub reason: Option<String>,
}
