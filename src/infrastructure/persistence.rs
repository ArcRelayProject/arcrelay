use crate::domain::quick_action::QuickAction;
use std::path::PathBuf;

/// The caller runs catalog mutations on a blocking worker. Success means the
/// atomic replacement and directory sync have completed, not merely queued.
pub struct JsonStore {
    config_dir: PathBuf,
}
impl JsonStore {
    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }
    fn actions_path(&self) -> PathBuf {
        self.config_dir.join("quick_actions.json")
    }
    pub fn actions_exist(&self) -> bool {
        self.actions_path().is_file()
    }
    pub fn load_actions(&self) -> Result<Vec<QuickAction>, String> {
        let path = self.actions_path();
        let bytes = std::fs::read(&path).map_err(|e| format!("failed to read actions: {e}"))?;
        match serde_json::from_slice(&bytes) {
            Ok(actions) => Ok(actions),
            Err(error) => {
                // Preserve corrupt input before recovering a verified backup.
                let backup = path.with_extension("json.bak");
                let recovered = std::fs::read(&backup)
                    .ok()
                    .and_then(|data| serde_json::from_slice(&data).ok());
                if let Some(actions) = recovered {
                    let corrupt =
                        path.with_extension(format!("json.corrupt.{}", uuid::Uuid::new_v4()));
                    crate::infrastructure::durable_file::replace(&corrupt, &bytes)
                        .map_err(|e| e.to_string())?;
                    Ok(actions)
                } else {
                    Err(format!(
                        "action file is corrupted; original retained at {}: {error}",
                        path.display()
                    ))
                }
            }
        }
    }
    pub fn save_actions(&self, actions: &[QuickAction]) -> Result<(), String> {
        let path = self.actions_path();
        let bytes = serde_json::to_vec(actions).map_err(|e| e.to_string())?;
        if let Ok(previous) = std::fs::read(&path) {
            if serde_json::from_slice::<Vec<QuickAction>>(&previous).is_ok() {
                crate::infrastructure::durable_file::replace(
                    &path.with_extension("json.bak"),
                    &previous,
                )
                .map_err(|e| e.to_string())?;
            }
        }
        crate::infrastructure::durable_file::replace(&path, &bytes)
            .map_err(|e| format!("failed to save actions: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn corrupt_catalog_is_not_treated_as_empty() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("quick_actions.json"), b"broken").unwrap();
        assert!(JsonStore::new(directory.path().into())
            .load_actions()
            .is_err());
        assert_eq!(
            std::fs::read(directory.path().join("quick_actions.json")).unwrap(),
            b"broken"
        );
    }
    #[test]
    fn failed_write_is_reported_and_backup_recovers_corruption() {
        let directory = tempfile::tempdir().unwrap();
        let store = JsonStore::new(directory.path().into());
        store.save_actions(&[]).unwrap();
        store.save_actions(&[]).unwrap();
        std::fs::write(store.actions_path(), b"broken").unwrap();
        assert!(store.load_actions().unwrap().is_empty());
        let blocked = directory.path().join("blocked");
        std::fs::write(&blocked, b"file").unwrap();
        assert!(JsonStore::new(blocked).save_actions(&[]).is_err());
    }
}
