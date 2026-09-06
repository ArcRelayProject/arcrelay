use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use arcrelay_input::{
    DisplayId, DisplaySurface, KeyboardProfile, KeyboardProfileKind, WorkspaceLayout,
};
use serde::{Deserialize, Serialize};

const STORE_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub enum HorizontalScrollBehavior {
    #[default]
    NativeScroll,
    NavigateHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfiguration {
    pub version: u32,
    #[serde(default)]
    pub input_sharing_enabled: bool,
    #[serde(default)]
    pub topology_author: String,
    pub layout: Option<WorkspaceLayout>,
    pub keyboard_profiles: Vec<KeyboardProfile>,
    pub remembered_displays: BTreeMap<DisplayId, DisplaySurface>,
    #[serde(default)]
    pub excluded_displays: BTreeSet<DisplayId>,
    pub active_keyboard_profile: KeyboardProfileKind,
    pub right_option_raw_mode: bool,
    #[serde(default)]
    pub horizontal_scroll_behavior: HorizontalScrollBehavior,
    #[serde(default)]
    pub consumer_shortcuts: Vec<arcrelay_input::ConsumerShortcut>,
}

fn default_active_keyboard_profile() -> KeyboardProfileKind {
    KeyboardProfileKind::Productivity
}

fn default_right_option_raw_mode() -> bool {
    true
}

impl Default for WorkspaceConfiguration {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            input_sharing_enabled: false,
            topology_author: String::new(),
            layout: None,
            keyboard_profiles: Vec::new(),
            remembered_displays: BTreeMap::new(),
            excluded_displays: BTreeSet::new(),
            active_keyboard_profile: default_active_keyboard_profile(),
            right_option_raw_mode: default_right_option_raw_mode(),
            horizontal_scroll_behavior: HorizontalScrollBehavior::default(),
            consumer_shortcuts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceStore {
    path: Arc<PathBuf>,
    state: Arc<Mutex<WorkspaceConfiguration>>,
}

impl WorkspaceStore {
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let path = path.into();
        let state = if path.exists() {
            let state: WorkspaceConfiguration = serde_json::from_slice(&std::fs::read(&path)?)?;
            if state.version != STORE_VERSION {
                return Err(StoreError::UnsupportedVersion(state.version));
            }
            if let Some(layout) = &state.layout {
                layout.validate()?;
            }
            state
        } else {
            WorkspaceConfiguration::default()
        };
        Ok(Self {
            path: Arc::new(path),
            state: Arc::new(Mutex::new(state)),
        })
    }

    pub fn snapshot(&self) -> WorkspaceConfiguration {
        lock(&self.state).clone()
    }

    pub fn horizontal_scroll_behavior(&self) -> HorizontalScrollBehavior {
        lock(&self.state).horizontal_scroll_behavior
    }

    pub fn save(&self, next: WorkspaceConfiguration) -> Result<(), StoreError> {
        arcrelay_input::validate_consumer_shortcuts(&next.consumer_shortcuts).map_err(
            |message| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    message,
                ))
            },
        )?;
        if next.version != STORE_VERSION {
            return Err(StoreError::UnsupportedVersion(next.version));
        }
        if let Some(layout) = &next.layout {
            layout.validate()?;
        }
        let bytes = serde_json::to_vec_pretty(&next)?;
        atomic_write(&self.path, &bytes)?;
        *lock(&self.state) = next;
        Ok(())
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(temporary, path)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("workspace I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("workspace JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("workspace topology is invalid: {0}")]
    Topology(#[from] arcrelay_input::TopologyError),
    #[error("unsupported workspace store version {0}")]
    UnsupportedVersion(u32),
}

#[cfg(test)]
mod tests {
    use super::*;
    use arcrelay_input::{
        EdgeSegment, KeyChord, KeyboardMappingRule, KeyboardProfileKind, SemanticAction, HID_KEY_C,
        HID_LEFT_CONTROL,
    };

    #[test]
    fn empty_store_is_stable() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        let store = WorkspaceStore::load(&path).unwrap();
        store.save(store.snapshot()).unwrap();
        assert_eq!(
            WorkspaceStore::load(path).unwrap().snapshot().version,
            STORE_VERSION
        );
    }

    #[test]
    fn custom_keyboard_rules_round_trip_as_json_arrays() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        let store = WorkspaceStore::load(&path).unwrap();
        let mut configuration = store.snapshot();
        let mut profile = KeyboardProfile::built_in(KeyboardProfileKind::Productivity);
        profile.semantic_overrides.push(KeyboardMappingRule {
            source: KeyChord::new([HID_LEFT_CONTROL], HID_KEY_C),
            action: SemanticAction::Copy,
        });
        configuration.keyboard_profiles.push(profile);
        store.save(configuration).unwrap();
        let loaded = WorkspaceStore::load(path).unwrap().snapshot();
        assert_eq!(loaded.keyboard_profiles[0].semantic_overrides.len(), 1);
    }

    #[test]
    fn consumer_shortcuts_persist_and_invalid_changes_leave_saved_state_intact() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        let store = WorkspaceStore::load(&path).unwrap();
        let mut configuration = store.snapshot();
        configuration
            .consumer_shortcuts
            .push(arcrelay_input::ConsumerShortcut {
                key: 0x4b,
                modifiers: 3,
                action: arcrelay_input::ConsumerKey::BrightnessUp,
            });
        store.save(configuration.clone()).unwrap();
        assert_eq!(
            WorkspaceStore::load(&path)
                .unwrap()
                .snapshot()
                .consumer_shortcuts,
            configuration.consumer_shortcuts
        );
        configuration
            .consumer_shortcuts
            .push(configuration.consumer_shortcuts[0].clone());
        assert!(store.save(configuration).is_err());
        assert_eq!(store.snapshot().consumer_shortcuts.len(), 1);
        assert_eq!(
            WorkspaceStore::load(&path)
                .unwrap()
                .snapshot()
                .consumer_shortcuts
                .len(),
            1
        );
    }

    #[test]
    fn unsupported_store_version_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        let configuration = WorkspaceConfiguration {
            version: 1,
            ..WorkspaceConfiguration::default()
        };
        std::fs::write(&path, serde_json::to_vec(&configuration).unwrap()).unwrap();

        assert!(matches!(
            WorkspaceStore::load(path),
            Err(StoreError::UnsupportedVersion(1))
        ));
    }

    #[test]
    fn version_two_store_without_continuous_input_fields_is_migrated() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("workspace.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "version": 2,
                "layout": null,
                "keyboardProfiles": [],
                "rememberedDisplays": {},
                "activeKeyboardProfile": "Productivity",
                "rightOptionRawMode": true
            }))
            .unwrap(),
        )
        .unwrap();

        let configuration = WorkspaceStore::load(path).unwrap().snapshot();
        assert!(!configuration.input_sharing_enabled);
        assert!(configuration.topology_author.is_empty());
        assert!(configuration.consumer_shortcuts.is_empty());
        assert_eq!(
            configuration.horizontal_scroll_behavior,
            HorizontalScrollBehavior::NativeScroll
        );
    }

    #[test]
    fn portal_segments_write_camel_case_and_read_legacy_snake_case() {
        let value = serde_json::to_value(EdgeSegment {
            start_um: 12,
            end_um: 34,
        })
        .unwrap();
        assert_eq!(value, serde_json::json!({ "startUm": 12, "endUm": 34 }));
        let restored: EdgeSegment = serde_json::from_value(value).unwrap();
        assert_eq!(restored.start_um, 12);
        assert_eq!(restored.end_um, 34);

        let legacy: EdgeSegment = serde_json::from_value(serde_json::json!({
            "start_um": 56,
            "end_um": 78
        }))
        .unwrap();
        assert_eq!(legacy.start_um, 56);
        assert_eq!(legacy.end_um, 78);
    }
}
