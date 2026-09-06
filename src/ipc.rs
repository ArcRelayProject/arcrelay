use serde::Serialize;

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct IpcError {
    pub code: IpcErrorCode,
    pub message: String,
    pub retryable: bool,
    pub recovery_action: IpcRecoveryAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum IpcErrorCode {
    OperationFailed,
    InvalidArgument,
    NotFound,
    Conflict,
    PermissionDenied,
    ResourceExhausted,
    FailedPrecondition,
    Unavailable,
    Cancelled,
    Internal,
    DirectorySnapshotExpired,
    TextSelectionExpired,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum IpcRecoveryAction {
    None,
    RetryWithBackoff,
    Refresh,
    ChangeRequest,
}

impl IpcError {
    pub fn new(
        code: IpcErrorCode,
        message: impl Into<String>,
        recovery_action: IpcRecoveryAction,
    ) -> Self {
        let mut message = message.into();
        let error_id = if code == IpcErrorCode::Internal {
            let error_id = uuid::Uuid::new_v4().simple().to_string();
            tracing::error!(
                event = "ipc.command.failed",
                error_id,
                error_code = "ipc.internal",
                error = %message,
                "IPC command failed internally"
            );
            message = "operation failed internally".into();
            Some(error_id)
        } else {
            None
        };
        Self {
            code,
            message,
            retryable: recovery_action == IpcRecoveryAction::RetryWithBackoff,
            recovery_action,
            error_id,
        }
    }
}

pub trait IntoIpcError {
    fn into_ipc_error(self) -> IpcError;
}

impl IntoIpcError for String {
    fn into_ipc_error(self) -> IpcError {
        IpcError::new(IpcErrorCode::OperationFailed, self, IpcRecoveryAction::None)
    }
}

impl IntoIpcError for IpcError {
    fn into_ipc_error(self) -> IpcError {
        self
    }
}

impl IntoIpcError for arcrelay_files::FileError {
    fn into_ipc_error(self) -> IpcError {
        use arcrelay_files::FileError;
        let (code, recovery) = match &self {
            FileError::Invalid(_) => (
                IpcErrorCode::InvalidArgument,
                IpcRecoveryAction::ChangeRequest,
            ),
            FileError::NotFound(_) => (IpcErrorCode::NotFound, IpcRecoveryAction::None),
            FileError::DirectorySnapshotExpired(_) => (
                IpcErrorCode::DirectorySnapshotExpired,
                IpcRecoveryAction::Refresh,
            ),
            FileError::Conflict(_) => (IpcErrorCode::Conflict, IpcRecoveryAction::Refresh),
            FileError::PermissionDenied(_) => {
                (IpcErrorCode::PermissionDenied, IpcRecoveryAction::None)
            }
            FileError::DirectoryQueryTooBroad(_) | FileError::FileTooLarge(_) => (
                IpcErrorCode::ResourceExhausted,
                IpcRecoveryAction::ChangeRequest,
            ),
            FileError::InsufficientStorage(_) => {
                (IpcErrorCode::ResourceExhausted, IpcRecoveryAction::None)
            }
            FileError::Unavailable(_) | FileError::Io { .. } => (
                IpcErrorCode::Unavailable,
                IpcRecoveryAction::RetryWithBackoff,
            ),
            FileError::Serialization(_)
            | FileError::PasswordHash(_)
            | FileError::Task(_)
            | FileError::Image(_) => (IpcErrorCode::Internal, IpcRecoveryAction::None),
        };
        IpcError::new(code, self.to_string(), recovery)
    }
}

impl IntoIpcError for arcrelay_protocol::remote_files::RemoteFileError {
    fn into_ipc_error(self) -> IpcError {
        use arcrelay_protocol::remote_files::RemoteFileErrorCode;
        let (code, recovery) = match self.code {
            RemoteFileErrorCode::InvalidArgument => (
                IpcErrorCode::InvalidArgument,
                IpcRecoveryAction::ChangeRequest,
            ),
            RemoteFileErrorCode::PermissionDenied => {
                (IpcErrorCode::PermissionDenied, IpcRecoveryAction::None)
            }
            RemoteFileErrorCode::NotFound => (IpcErrorCode::NotFound, IpcRecoveryAction::None),
            RemoteFileErrorCode::Conflict => (IpcErrorCode::Conflict, IpcRecoveryAction::Refresh),
            RemoteFileErrorCode::ResourceExhausted => (
                IpcErrorCode::ResourceExhausted,
                IpcRecoveryAction::ChangeRequest,
            ),
            RemoteFileErrorCode::FailedPrecondition => (
                IpcErrorCode::FailedPrecondition,
                IpcRecoveryAction::ChangeRequest,
            ),
            RemoteFileErrorCode::Unavailable => (
                IpcErrorCode::Unavailable,
                IpcRecoveryAction::RetryWithBackoff,
            ),
            RemoteFileErrorCode::Cancelled => (IpcErrorCode::Cancelled, IpcRecoveryAction::None),
            RemoteFileErrorCode::Internal => (IpcErrorCode::Internal, IpcRecoveryAction::None),
        };
        IpcError::new(code, self.message, recovery)
    }
}

impl IntoIpcError for arcrelay_core::Error {
    fn into_ipc_error(self) -> IpcError {
        let (code, recovery) = match self.kind() {
            arcrelay_core::ErrorKind::Unsupported => {
                (IpcErrorCode::Unsupported, IpcRecoveryAction::None)
            }
            arcrelay_core::ErrorKind::NotFound => (IpcErrorCode::NotFound, IpcRecoveryAction::None),
            arcrelay_core::ErrorKind::Unavailable => (
                IpcErrorCode::Unavailable,
                IpcRecoveryAction::RetryWithBackoff,
            ),
            arcrelay_core::ErrorKind::OperationFailed => {
                (IpcErrorCode::OperationFailed, IpcRecoveryAction::None)
            }
            arcrelay_core::ErrorKind::Internal => (IpcErrorCode::Internal, IpcRecoveryAction::None),
        };
        IpcError::new(code, self.to_string(), recovery)
    }
}

impl IntoIpcError for arcrelay_transfer::TransferError {
    fn into_ipc_error(self) -> IpcError {
        use arcrelay_transfer::TransferError;
        let (code, recovery) = match &self {
            TransferError::Cancelled => (IpcErrorCode::Cancelled, IpcRecoveryAction::None),
            TransferError::Invalid(_) => (
                IpcErrorCode::InvalidArgument,
                IpcRecoveryAction::ChangeRequest,
            ),
            TransferError::PeerNotFound(_) | TransferError::TransferNotFound(_) => {
                (IpcErrorCode::NotFound, IpcRecoveryAction::None)
            }
            TransferError::InvalidState(_) | TransferError::Integrity(_) => (
                IpcErrorCode::FailedPrecondition,
                IpcRecoveryAction::ChangeRequest,
            ),
            TransferError::Io(_) | TransferError::Network(_) => (
                IpcErrorCode::Unavailable,
                IpcRecoveryAction::RetryWithBackoff,
            ),
            TransferError::Serialization(_) => (IpcErrorCode::Internal, IpcRecoveryAction::None),
        };
        IpcError::new(code, self.to_string(), recovery)
    }
}

impl IntoIpcError for arcrelay_print::PrintError {
    fn into_ipc_error(self) -> IpcError {
        use arcrelay_print::PrintError;
        let (code, recovery) = match &self {
            PrintError::Invalid(_) => (
                IpcErrorCode::InvalidArgument,
                IpcRecoveryAction::ChangeRequest,
            ),
            PrintError::InvalidState(_) => (
                IpcErrorCode::FailedPrecondition,
                IpcRecoveryAction::ChangeRequest,
            ),
            PrintError::NotFound(_) => (IpcErrorCode::NotFound, IpcRecoveryAction::None),
            PrintError::Conflict(_) => (IpcErrorCode::Conflict, IpcRecoveryAction::Refresh),
            PrintError::Backend(_) | PrintError::Io(_) => (
                IpcErrorCode::Unavailable,
                IpcRecoveryAction::RetryWithBackoff,
            ),
            PrintError::Persistence(_) | PrintError::Serialization(_) => {
                (IpcErrorCode::Internal, IpcRecoveryAction::None)
            }
        };
        IpcError::new(code, self.to_string(), recovery)
    }
}

impl IntoIpcError for arcrelay_automation::AutomationError {
    fn into_ipc_error(self) -> IpcError {
        use arcrelay_automation::AutomationError;
        let code = match &self {
            AutomationError::Invalid(_) => IpcErrorCode::InvalidArgument,
            AutomationError::Conflict => IpcErrorCode::Conflict,
            AutomationError::NotFound => IpcErrorCode::NotFound,
            AutomationError::Database(_) | AutomationError::Json(_) => IpcErrorCode::Internal,
        };
        IpcError::new(code, self.to_string(), IpcRecoveryAction::None)
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(formatter)
    }
}

impl std::error::Error for IpcError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_domain_errors_without_parsing_messages() {
        let expired =
            arcrelay_files::FileError::DirectorySnapshotExpired("cursor".into()).into_ipc_error();
        assert_eq!(expired.code, IpcErrorCode::DirectorySnapshotExpired);
        assert!(!expired.retryable);
        assert_eq!(expired.recovery_action, IpcRecoveryAction::Refresh);

        let conflict =
            arcrelay_transfer::TransferError::Integrity("digest mismatch".into()).into_ipc_error();
        assert_eq!(conflict.code, IpcErrorCode::FailedPrecondition);
        assert!(!conflict.retryable);

        let cancelled = arcrelay_transfer::TransferError::Cancelled.into_ipc_error();
        assert_eq!(cancelled.code, IpcErrorCode::Cancelled);
        assert!(!cancelled.retryable);
    }

    #[test]
    fn serializes_the_stable_ipc_shape() {
        let value = serde_json::to_value(IpcError::new(
            IpcErrorCode::PermissionDenied,
            "permission denied",
            IpcRecoveryAction::None,
        ))
        .unwrap();
        assert_eq!(value["code"], "permissionDenied");
        assert_eq!(value["message"], "permission denied");
        assert_eq!(value["retryable"], false);
        assert_eq!(value["recoveryAction"], "none");
        assert!(value.get("errorId").is_none());
    }

    #[test]
    fn hides_internal_details_and_adds_a_correlation_id() {
        let error = arcrelay_core::Error::Other("database password leaked".into()).into_ipc_error();
        assert_eq!(error.code, IpcErrorCode::Internal);
        assert_eq!(error.message, "operation failed internally");
        assert!(error.error_id.is_some());
        assert!(!error.message.contains("password"));
    }
}
