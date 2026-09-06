use std::path::PathBuf;

use arcrelay_peer::ServiceInstanceId;

#[derive(Debug, Clone)]
pub struct ProductPaths {
    pub root: PathBuf,
    pub data: PathBuf,
    pub logs: PathBuf,
    pub workspace_store: PathBuf,
}

impl ProductPaths {
    pub fn default_for_user() -> Result<Self, IdentityError> {
        let base = dirs::data_local_dir().ok_or(IdentityError::MissingDataDirectory)?;
        let app_root = base.join("ArcRelay");
        let mut paths = Self::from_root(app_root.join("arc-input"));
        paths.logs = app_root.join("logs").join("desktop");
        Ok(paths)
    }

    pub fn from_root(root: PathBuf) -> Self {
        let data = root.join("data");
        let logs = root.join("logs");
        Self {
            workspace_store: data.join("workspace.json"),
            root,
            data,
            logs,
        }
    }

    pub fn create(&self) -> Result<(), IdentityError> {
        // Arc Input keeps only feature configuration here. Device key material
        // belongs exclusively to arcrelay-network's root identity directory.
        for path in [&self.root, &self.data, &self.logs] {
            std::fs::create_dir_all(path)?;
            harden_directory(path)?;
        }
        Ok(())
    }
}

/// Feature-local view of the unified root device id.
pub struct ProductIdentity {
    pub service_instance_id: ServiceInstanceId,
}

impl ProductIdentity {
    pub fn from_device_id(device_id: impl Into<String>) -> Result<Self, IdentityError> {
        Ok(Self {
            service_instance_id: ServiceInstanceId::parse(device_id.into())?,
        })
    }
}

fn harden_directory(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("Arc Input data directory is unavailable")]
    MissingDataDirectory,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    PeerId(#[from] arcrelay_peer::PeerIdError),
}
