use arcrelay_core::domain::text_slices::{
    build_text_slice_model, selected_slice_text, TextSliceError, TextSliceModel,
};
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

/// Short-lived view models owned by the host. Selection IPC carries IDs only;
/// closing the clipboard drops models and future stale requests fail explicitly.
#[derive(Default)]
pub struct TextSelectionService {
    generation: AtomicU64,
    models: Mutex<VecDeque<(u64, Arc<TextSliceModel>)>>,
}

#[derive(Debug, thiserror::Error)]
pub enum TextSelectionError {
    #[error("text preview expired; reopen it")]
    PreviewExpired,
    #[error("text changed; reopen the preview")]
    TextChanged,
    #[error(transparent)]
    InvalidSelection(#[from] TextSliceError),
}

impl crate::ipc::IntoIpcError for TextSelectionError {
    fn into_ipc_error(self) -> crate::ipc::IpcError {
        use crate::ipc::{IpcError, IpcErrorCode, IpcRecoveryAction};

        let code = match self {
            Self::PreviewExpired
            | Self::TextChanged
            | Self::InvalidSelection(TextSliceError::Stale) => IpcErrorCode::TextSelectionExpired,
            Self::InvalidSelection(_) => IpcErrorCode::InvalidArgument,
        };
        let recovery = if code == IpcErrorCode::TextSelectionExpired {
            IpcRecoveryAction::Refresh
        } else {
            IpcRecoveryAction::ChangeRequest
        };
        IpcError::new(code, self.to_string(), recovery)
    }
}
impl TextSelectionService {
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    pub fn model(&self, id: u64, text: &str, generation: u64) -> Arc<TextSliceModel> {
        {
            let models = self
                .models
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some((_, model)) = models.iter().find(|(key, m)| *key == id && m.text == text) {
                return model.clone();
            }
        }
        let model = Arc::new(build_text_slice_model(text));
        let mut models = self
            .models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.generation.load(Ordering::Acquire) != generation {
            return model;
        }
        models.retain(|(key, _)| *key != id);
        models.push_back((id, model.clone()));
        while models.len() > 4
            || (models.len() > 1
                && models.iter().map(|(_, m)| m.text.len() * 7).sum::<usize>() > 32 * 1024 * 1024)
        {
            models.pop_front();
        }
        model
    }
    pub fn join(
        &self,
        id: u64,
        version: &str,
        text: &str,
        ids: &[String],
    ) -> Result<String, TextSelectionError> {
        let model = self
            .models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(key, m)| *key == id && m.version == version)
            .map(|(_, m)| m.clone())
            .ok_or(TextSelectionError::PreviewExpired)?;
        if model.text != text {
            return Err(TextSelectionError::TextChanged);
        }
        Ok(selected_slice_text(&model, ids)?)
    }
    pub fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.models
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_load_started_before_hide_cannot_repopulate_selection_models() {
        let service = TextSelectionService::default();
        let old_generation = service.generation();
        service.clear();
        let old = service.model(1, "hello world", old_generation);
        let ids = vec![old.levels.document[0].id.clone()];
        assert!(service.join(1, &old.version, "hello world", &ids).is_err());
        let current = service.model(1, "hello world", service.generation());
        assert_eq!(
            service
                .join(1, &current.version, "hello world", &ids)
                .unwrap(),
            "hello world"
        );
    }
}
