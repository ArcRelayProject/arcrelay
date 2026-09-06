use rmcp::{
    handler::server::tool::IntoCallToolResult,
    model::{CallToolResponse, CallToolResult},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolFailure {
    pub code: String,
    pub message: String,
    pub field: Option<String>,
    pub remedy: String,
    pub details: Option<Box<serde_json::Value>>,
}

pub type ToolResult<T> = Result<rmcp::Json<T>, ToolFailure>;

impl ToolFailure {
    pub fn new(code: &str, message: impl Into<String>, remedy: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            remedy: remedy.into(),
            field: None,
            details: None,
        }
    }
    pub fn invalid(field: &str, message: impl Into<String>) -> Self {
        let mut error = Self::new(
            "configuration.invalid",
            message,
            "correct the indicated field and validate again",
        );
        error.field = Some(field.into());
        error
    }
    pub fn denied(permission: &str) -> Self {
        let mut error = Self::new(
            "access.denied",
            format!("this client needs the {permission} permission"),
            "change this client's permissions in ArcRelay Settings → Agent access",
        );
        error.field = Some(permission.into());
        error
    }
}

impl From<String> for ToolFailure {
    fn from(message: String) -> Self {
        if message.contains("configuration outcome could not be recorded")
            || message.contains("configuration saved, but")
        {
            return Self::new(
                "operation.unresolved",
                message,
                "inspect the current configuration and change history before issuing a new operation; the write may already have completed",
            );
        }
        let code = if message.contains("conflict") || message.contains("modified in another") {
            "configuration.conflict"
        } else if message.contains("not found") || message.contains("does not exist") {
            "configuration.not_found"
        } else {
            "configuration.invalid"
        };
        Self::new(
            code,
            message,
            "read the current configuration, correct the request, and use a new idempotency key",
        )
    }
}
impl From<arcrelay_automation::AutomationError> for ToolFailure {
    fn from(error: arcrelay_automation::AutomationError) -> Self {
        Self::new(
            error.code(),
            error.to_string(),
            "read the current configuration and check its capabilities",
        )
    }
}
impl From<sqlx::Error> for ToolFailure {
    fn from(error: sqlx::Error) -> Self {
        Self::new(
            "storage.failed",
            error.to_string(),
            "check local storage; retry using the same idempotency key",
        )
    }
}
impl From<serde_json::Error> for ToolFailure {
    fn from(error: serde_json::Error) -> Self {
        Self::invalid("configuration", error.to_string())
    }
}
impl IntoCallToolResult for ToolFailure {
    fn into_call_tool_result(self) -> Result<CallToolResponse, rmcp::ErrorData> {
        Ok(CallToolResult::structured_error(serde_json::json!({"error": self})).into())
    }
}
