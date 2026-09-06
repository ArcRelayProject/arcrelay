//! Activation forwarding exists before durable services are initialized.
use std::sync::Mutex;

pub struct PendingActivations(Mutex<Option<Vec<Vec<String>>>>);
impl Default for PendingActivations {
    fn default() -> Self {
        Self(Mutex::new(Some(Vec::new())))
    }
}
impl PendingActivations {
    pub fn push(&self, arguments: Vec<String>) -> Option<Vec<String>> {
        let mut pending = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match pending.as_mut() {
            Some(queue) => {
                queue.push(arguments);
                None
            }
            None => Some(arguments),
        }
    }
    pub fn take(&self) -> Vec<Vec<String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pre_service_activation_forwarding_needs_no_runtime() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let queue = PendingActivations::default();
        queue.push(vec![
            "ArcRelay".into(),
            "--share".into(),
            "路径 with spaces".into(),
        ]);
        let pending = queue.take();
        assert_eq!(pending[0][2], "路径 with spaces");
        assert!(queue.take().is_empty());
        assert_eq!(
            queue.push(vec!["late activation".into()]),
            Some(vec!["late activation".into()])
        );
    }
}
