use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const MAX_NOTIFICATIONS: usize = 500;
const MAX_TITLE_BYTES: usize = 160;
const MAX_BODY_BYTES: usize = 8 * 1024;
const MAX_SOURCE_BYTES: usize = 120;
const MAX_REFERENCE_BYTES: usize = 512;
const LOCK_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const LOCK_RETRY_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    #[default]
    Info,
    TaskCompleted,
    ActionRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostNotification {
    pub id: String,
    pub title: String,
    pub body: String,
    pub source: String,
    pub kind: NotificationKind,
    pub reference: Option<String>,
    pub created_at_ms: i64,
    pub read_at_ms: Option<i64>,
    pub read_by_device_id: Option<String>,
    pub read_by_device_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewNotification {
    pub title: String,
    pub body: String,
    pub source: String,
    pub kind: NotificationKind,
    pub reference: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NotificationStore {
    path: PathBuf,
    lock_path: PathBuf,
    cache: Arc<Mutex<Option<Vec<HostNotification>>>>,
    unread_count_cache: Arc<Mutex<Option<usize>>>,
    changes: Arc<broadcast::Sender<()>>,
}

impl NotificationStore {
    pub fn from_config_dir(config_dir: impl AsRef<Path>) -> Self {
        let config_dir = config_dir.as_ref();
        let (changes, _) = broadcast::channel(64);
        Self {
            path: config_dir.join("notifications.json"),
            lock_path: config_dir.join("notifications.lock"),
            cache: Arc::new(Mutex::new(None)),
            unread_count_cache: Arc::new(Mutex::new(None)),
            changes: Arc::new(changes),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changes.subscribe()
    }

    pub fn list(&self, include_read: bool, limit: usize) -> Result<Vec<HostNotification>, String> {
        self.with_lock(|| {
            let mut notifications = self.read_unlocked()?;
            notifications.sort_by_key(|item| std::cmp::Reverse(item.created_at_ms));
            if !include_read {
                notifications.retain(|item| item.read_at_ms.is_none());
            }
            notifications.truncate(limit.clamp(1, MAX_NOTIFICATIONS));
            Ok(notifications)
        })
    }

    pub fn unread_count(&self) -> Result<usize, String> {
        if let Some(count) = *self
            .unread_count_cache
            .lock()
            .map_err(|error| error.to_string())?
        {
            return Ok(count);
        }
        self.with_lock(|| {
            let notifications = self.read_unlocked()?;
            let count = notifications
                .iter()
                .filter(|notification| notification.read_at_ms.is_none())
                .count();
            *self
                .unread_count_cache
                .lock()
                .map_err(|error| error.to_string())? = Some(count);
            Ok(count)
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<HostNotification>, String> {
        let id = validate_identifier(id)?;
        self.with_lock(|| {
            Ok(self
                .read_unlocked()?
                .into_iter()
                .find(|notification| notification.id == id))
        })
    }

    pub fn create(&self, value: NewNotification) -> Result<HostNotification, String> {
        let title = validate_text("notification title", value.title, MAX_TITLE_BYTES, false)?;
        let body = validate_text("notification body", value.body, MAX_BODY_BYTES, true)?;
        let source = validate_text("notification source", value.source, MAX_SOURCE_BYTES, false)?;
        let reference = value
            .reference
            .map(|reference| {
                validate_text(
                    "notification reference",
                    reference,
                    MAX_REFERENCE_BYTES,
                    true,
                )
            })
            .transpose()?
            .filter(|reference| !reference.is_empty());
        let notification = self.with_lock(|| {
            let mut notifications = self.read_unlocked()?;
            let notification = HostNotification {
                id: uuid::Uuid::new_v4().to_string(),
                title,
                body,
                source,
                kind: value.kind,
                reference,
                created_at_ms: now_ms(),
                read_at_ms: None,
                read_by_device_id: None,
                read_by_device_name: None,
            };
            notifications.push(notification.clone());
            trim_notifications(&mut notifications);
            self.write_unlocked(&notifications)?;
            Ok(notification)
        })?;
        let _ = self.changes.send(());
        Ok(notification)
    }

    pub fn mark_read(
        &self,
        id: &str,
        device_id: Option<&str>,
        device_name: Option<&str>,
    ) -> Result<HostNotification, String> {
        let id = validate_identifier(id)?;
        let device_id = device_id
            .map(|value| validate_text("device ID", value.to_string(), 256, false))
            .transpose()?;
        let device_name = device_name
            .map(|value| validate_text("device name", value.to_string(), 128, false))
            .transpose()?;
        let (notification, changed) = self.with_lock(|| {
            let mut notifications = self.read_unlocked()?;
            let notification = notifications
                .iter_mut()
                .find(|notification| notification.id == id)
                .ok_or_else(|| "notification not found".to_string())?;
            if notification.read_at_ms.is_none() {
                notification.read_at_ms = Some(now_ms());
                notification.read_by_device_id = device_id;
                notification.read_by_device_name = device_name;
                let updated = notification.clone();
                self.write_unlocked(&notifications)?;
                Ok((updated, true))
            } else {
                Ok((notification.clone(), false))
            }
        })?;
        if changed {
            let _ = self.changes.send(());
        }
        Ok(notification)
    }

    pub fn delete(&self, id: &str) -> Result<bool, String> {
        let id = validate_identifier(id)?;
        let deleted = self.with_lock(|| {
            let mut notifications = self.read_unlocked()?;
            let previous_len = notifications.len();
            notifications.retain(|notification| notification.id != id);
            if notifications.len() == previous_len {
                return Ok(false);
            }
            self.write_unlocked(&notifications)?;
            Ok(true)
        })?;
        if deleted {
            let _ = self.changes.send(());
        }
        Ok(deleted)
    }

    fn with_lock<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.lock_path)
            .map_err(|error| error.to_string())?;
        let deadline = Instant::now() + LOCK_WAIT_TIMEOUT;
        loop {
            match lock.try_lock_exclusive() {
                Ok(()) => break,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "notification store lock wait exceeded {} seconds",
                            LOCK_WAIT_TIMEOUT.as_secs()
                        ));
                    }
                    std::thread::sleep(LOCK_RETRY_INTERVAL);
                }
                Err(error) => return Err(error.to_string()),
            }
        }
        let result = operation();
        let _ = lock.unlock();
        result
    }

    fn read_unlocked(&self) -> Result<Vec<HostNotification>, String> {
        if let Some(notifications) = self
            .cache
            .lock()
            .map_err(|error| error.to_string())?
            .as_ref()
        {
            return Ok(notifications.clone());
        }
        let notifications: Vec<HostNotification> = match std::fs::read(&self.path) {
            Ok(bytes) if bytes.is_empty() => Vec::new(),
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| format!("notification store is corrupted: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.to_string()),
        };
        *self
            .unread_count_cache
            .lock()
            .map_err(|error| error.to_string())? = Some(
            notifications
                .iter()
                .filter(|notification| notification.read_at_ms.is_none())
                .count(),
        );
        *self.cache.lock().map_err(|error| error.to_string())? = Some(notifications.clone());
        Ok(notifications)
    }

    fn write_unlocked(&self, notifications: &[HostNotification]) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(notifications).map_err(|error| error.to_string())?;
        crate::infrastructure::durable_file::replace(&self.path, &bytes)
            .map_err(|error| error.to_string())?;
        *self
            .unread_count_cache
            .lock()
            .map_err(|error| error.to_string())? = Some(
            notifications
                .iter()
                .filter(|notification| notification.read_at_ms.is_none())
                .count(),
        );
        *self.cache.lock().map_err(|error| error.to_string())? = Some(notifications.to_vec());
        Ok(())
    }
}

fn trim_notifications(notifications: &mut Vec<HostNotification>) {
    if notifications.len() <= MAX_NOTIFICATIONS {
        return;
    }
    notifications.sort_by_key(|item| (item.read_at_ms.is_none(), item.created_at_ms));
    notifications.drain(0..notifications.len().saturating_sub(MAX_NOTIFICATIONS));
}

fn validate_identifier(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err("invalid notification id".to_string());
    }
    Ok(value.to_string())
}

fn validate_text(
    field: &str,
    value: String,
    maximum_bytes: usize,
    allow_empty: bool,
) -> Result<String, String> {
    let value = value.trim().to_string();
    if !allow_empty && value.is_empty() {
        return Err(format!("{field} cannot be empty"));
    }
    if value.len() > maximum_bytes {
        return Err(format!("{field} is too long"));
    }
    if value.chars().any(|character| character == '\0') {
        return Err(format!("{field} contains invalid characters"));
    }
    Ok(value)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_store() -> (PathBuf, NotificationStore) {
        let directory = std::env::temp_dir().join(format!(
            "arcrelay-notification-store-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = NotificationStore::from_config_dir(&directory);
        (directory, store)
    }

    #[test]
    fn lock_wait_is_bounded() {
        let (directory, store) = temporary_store();
        std::fs::create_dir_all(&directory).unwrap();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&store.lock_path)
            .unwrap();
        lock.lock_exclusive().unwrap();

        let started = Instant::now();
        let error = store.list(true, 10).expect_err("held lock must time out");

        assert!(error.contains("lock wait exceeded"));
        assert!(started.elapsed() >= LOCK_WAIT_TIMEOUT);
        assert!(started.elapsed() < LOCK_WAIT_TIMEOUT + Duration::from_secs(1));
        lock.unlock().unwrap();
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn first_read_wins_across_devices() {
        let (directory, store) = temporary_store();
        let mut changes = store.subscribe();
        let created = store
            .create(NewNotification {
                title: "需要确认".into(),
                body: "请查看任务".into(),
                source: "Codex".into(),
                kind: NotificationKind::ActionRequired,
                reference: None,
            })
            .unwrap();
        assert!(changes.try_recv().is_ok());
        let first = store
            .mark_read(&created.id, Some("iphone"), Some("iPhone"))
            .unwrap();
        let second = store
            .mark_read(&created.id, Some("ipad"), Some("iPad"))
            .unwrap();
        assert!(changes.try_recv().is_ok());
        assert!(changes.try_recv().is_err());
        assert_eq!(first.read_at_ms, second.read_at_ms);
        assert_eq!(second.read_by_device_id.as_deref(), Some("iphone"));
        assert!(store.list(false, 20).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn unread_count_tracks_creates_reads_and_deletes() {
        let (directory, store) = temporary_store();
        let first = store
            .create(NewNotification {
                title: "第一条".into(),
                body: String::new(),
                source: "test".into(),
                kind: NotificationKind::Info,
                reference: None,
            })
            .unwrap();
        let second = store
            .create(NewNotification {
                title: "第二条".into(),
                body: String::new(),
                source: "test".into(),
                kind: NotificationKind::Info,
                reference: None,
            })
            .unwrap();

        assert_eq!(store.unread_count().unwrap(), 2);
        store.mark_read(&first.id, None, None).unwrap();
        assert_eq!(store.unread_count().unwrap(), 1);
        store.delete(&second.id).unwrap();
        assert_eq!(store.unread_count().unwrap(), 0);

        let _ = std::fs::remove_dir_all(directory);
    }
}
