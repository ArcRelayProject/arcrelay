use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, ts_rs::TS)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct McpPermissions {
    pub notifications: bool,
    pub read: bool,
    pub manage: bool,
    pub enable: bool,
    pub execute: bool,
    pub scripts: bool,
}

impl McpPermissions {
    fn validate(&self) -> Result<(), String> {
        if !self.read && (self.manage || self.enable || self.execute || self.scripts) {
            return Err(
                "management, activation, execution and scripts require read permission".into(),
            );
        }
        Ok(())
    }
    pub fn allows(&self, permission: &str) -> bool {
        match permission {
            "notifications" => self.notifications,
            "read" => self.read,
            "manage" => self.manage,
            "enable" => self.enable,
            "execute" => self.execute,
            "scripts" => self.scripts,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct McpClientView {
    pub id: String,
    pub name: String,
    pub permissions: McpPermissions,
    pub revoked: bool,
    pub legacy: bool,
    pub created_at: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Client {
    view: McpClientView,
    token_hash: String,
    // Kept in an owner-only local file so the user can copy their configuration again.
    // Never included in list responses, logs, audit entries or MCP tools.
    #[serde(default)]
    token: Option<String>,
}

#[derive(Clone)]
pub struct Credential(pub String);

/// Credentials are checked on every HTTP request and again before tool dispatch.
/// Configuration credentials stay in an owner-only local file. The old token can only notify.
pub struct McpAccess {
    path: PathBuf,
    clients: Mutex<Vec<Client>>,
}

pub fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

pub fn new_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

impl McpAccess {
    pub fn new(directory: &Path, legacy_token: &str) -> Result<Self, String> {
        let directory = directory.join("mcp");
        std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
        }
        let path = directory.join("clients.json");
        let clients = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("invalid MCP client registry: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => vec![Client {
                view: McpClientView {
                    id: "legacy-notifications".into(),
                    name: "Legacy notifications".into(),
                    permissions: McpPermissions {
                        notifications: true,
                        ..Default::default()
                    },
                    revoked: false,
                    legacy: true,
                    created_at: chrono::Utc::now().to_rfc3339(),
                },
                token_hash: digest(legacy_token),
                token: None,
            }],
            Err(e) => return Err(e.to_string()),
        };
        let store = Self {
            path,
            clients: Mutex::new(clients),
        };
        store.persist(&store.clients.lock().unwrap_or_else(|e| e.into_inner()))?;
        Ok(store)
    }

    fn persist(&self, clients: &[Client]) -> Result<(), String> {
        crate::infrastructure::durable_file::replace_private(
            &self.path,
            &serde_json::to_vec(clients).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }

    pub fn list(&self) -> Vec<McpClientView> {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|c| c.view.clone())
            .collect()
    }

    pub fn resolve(&self, credential: &Credential) -> Result<McpClientView, String> {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|c| c.token_hash == credential.0 && !c.view.revoked)
            .map(|c| {
                let mut view = c.view.clone();
                // A legacy credential must never acquire management through migration.
                if view.legacy {
                    view.permissions = McpPermissions {
                        notifications: true,
                        ..Default::default()
                    };
                }
                view
            })
            .ok_or_else(|| "access.denied: missing, revoked or invalid client credential".into())
    }

    pub fn create(
        &self,
        name: String,
        permissions: McpPermissions,
    ) -> Result<(McpClientView, String), String> {
        permissions.validate()?;
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > 100 {
            return Err("enter a client name of 1–100 bytes".into());
        }
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        if clients.len() >= 128 {
            return Err("no more than 128 MCP clients can be saved".into());
        }
        let token = new_token();
        let view = McpClientView {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            permissions,
            revoked: false,
            legacy: false,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let mut next = clients.clone();
        next.push(Client {
            view: view.clone(),
            token_hash: digest(&token),
            token: Some(token.clone()),
        });
        self.persist(&next)?;
        *clients = next;
        Ok((view, token))
    }

    fn update(
        &self,
        id: &str,
        permissions: Option<McpPermissions>,
        revoke: bool,
    ) -> Result<(), String> {
        if let Some(permissions) = permissions {
            permissions.validate()?;
        }
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = clients.clone();
        let client = next
            .iter_mut()
            .find(|c| c.view.id == id)
            .ok_or("MCP client not found")?;
        let permissions = permissions.unwrap_or(client.view.permissions);
        if client.view.legacy
            && (permissions.read
                || permissions.manage
                || permissions.enable
                || permissions.execute
                || permissions.scripts)
        {
            return Err("create a separate client to grant management permissions".into());
        }
        client.view.permissions = permissions;
        client.view.revoked |= revoke;
        self.persist(&next)?;
        *clients = next;
        Ok(())
    }

    pub fn set_permissions(&self, id: &str, permissions: McpPermissions) -> Result<(), String> {
        self.update(id, Some(permissions), false)
    }

    pub fn revoke(&self, id: &str) -> Result<(), String> {
        self.update(id, None, true)
    }

    pub fn rotate(&self, id: &str) -> Result<(McpClientView, String), String> {
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = clients.clone();
        let client = next
            .iter_mut()
            .find(|c| c.view.id == id)
            .ok_or("MCP client not found")?;
        if client.view.legacy {
            return Err("create a separate client, then revoke the legacy credential".into());
        }
        let token = new_token();
        client.token_hash = digest(&token);
        client.token = Some(token.clone());
        client.view.revoked = false;
        let view = client.view.clone();
        self.persist(&next)?;
        *clients = next;
        Ok((view, token))
    }

    pub fn configuration_token(&self, id: &str) -> Result<String, String> {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|c| c.view.id == id && !c.view.revoked)
            .and_then(|c| c.token.clone())
            .ok_or_else(|| "client was revoked or its token must be rotated before copying".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_remain_scoped_and_private_without_a_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let store = McpAccess::new(directory.path(), "legacy-token").unwrap();
        let old = Credential(digest("legacy-token"));
        let legacy = store.resolve(&old).unwrap();
        assert!(legacy.permissions.notifications);
        assert!(!legacy.permissions.manage);
        assert!(store
            .set_permissions(
                &legacy.id,
                McpPermissions {
                    manage: true,
                    ..Default::default()
                }
            )
            .is_err());
        let (client, token) = store
            .create(
                "Test".into(),
                McpPermissions {
                    read: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let credential = Credential(digest(&token));
        assert!(store.resolve(&credential).unwrap().permissions.read);
        assert!(!serde_json::to_string(&store.list())
            .unwrap()
            .contains(&token));
        assert_eq!(store.configuration_token(&client.id).unwrap(), token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let (_, replacement) = store.rotate(&client.id).unwrap();
        assert!(store.resolve(&credential).is_err());
        let replacement = Credential(digest(replacement));
        assert!(store.resolve(&replacement).is_ok());
        store.revoke(&client.id).unwrap();
        assert!(store.resolve(&replacement).is_err());
        store
            .set_permissions(&client.id, client.permissions)
            .unwrap();
        assert!(
            store.resolve(&replacement).is_err(),
            "permission edits must never revive a revoked token"
        );
        store.revoke(&legacy.id).unwrap();
        drop(store);
        assert!(McpAccess::new(directory.path(), "legacy-token")
            .unwrap()
            .resolve(&old)
            .is_err());
    }
}
