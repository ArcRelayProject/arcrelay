//! Optional feature initialization runs on the long-lived host runtime. Failed
//! initialization preserves the on-disk state and can be retried on next use.
use arcrelay_print::application::{PrintRuntime, PrintRuntimeAdapters, PrintRuntimeConfig};
use arcrelay_transfer::{TransferConfig, TransferManager};
use serde::Serialize;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{watch, Mutex, OnceCell};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ModuleState {
    Idle,
    Starting,
    Ready,
    Unavailable,
    Stopping,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ModuleStatus {
    pub name: String,
    pub state: ModuleState,
    pub error: Option<String>,
}
pub struct RuntimeModules {
    resources: Arc<arcrelay_content::ContentResources>,
    stopping: AtomicBool,
    lifecycle: Mutex<()>,
    transfer_config: TransferConfig,
    print_config: PrintRuntimeConfig,
    transfer: OnceCell<Arc<TransferManager>>,
    print: OnceCell<Arc<PrintRuntime>>,
    statuses: watch::Sender<Vec<ModuleStatus>>,
    pub print_has_saved_state: bool,
}
impl RuntimeModules {
    pub fn content_resources(&self) -> Arc<arcrelay_content::ContentResources> {
        self.resources.clone()
    }
    pub fn new(
        resources: Arc<arcrelay_content::ContentResources>,
        transfer_config: TransferConfig,
        print_config: PrintRuntimeConfig,
        print_has_saved_state: bool,
    ) -> Self {
        Self {
            resources,
            stopping: AtomicBool::new(false),
            lifecycle: Mutex::new(()),
            transfer_config,
            print_config,
            print_has_saved_state,
            transfer: OnceCell::new(),
            print: OnceCell::new(),
            statuses: watch::channel(
                ["transfer", "print"]
                    .into_iter()
                    .map(|name| ModuleStatus {
                        name: name.into(),
                        state: ModuleState::Idle,
                        error: None,
                    })
                    .collect(),
            )
            .0,
        }
    }
    pub fn status(&self, name: &str, state: ModuleState, error: Option<String>) {
        self.statuses.send_if_modified(|statuses| {
            if let Some(status) = statuses.iter_mut().find(|s| s.name == name) {
                let next = ModuleStatus {
                    name: name.into(),
                    state,
                    error,
                };
                if *status != next {
                    *status = next;
                    return true;
                }
            }
            false
        });
    }

    pub async fn shutdown(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        let _lifecycle = self.lifecycle.lock().await;
        for name in ["transfer", "print"] {
            self.status(name, ModuleState::Stopping, None);
        }
        if let Some(transfer) = self.transfer.get() {
            transfer.shutdown().await;
        }
        self.resources.shutdown();
        for name in ["transfer", "print"] {
            self.status(name, ModuleState::Stopped, None);
        }
    }

    pub fn statuses(&self) -> Vec<ModuleStatus> {
        self.statuses.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<Vec<ModuleStatus>> {
        self.statuses.subscribe()
    }
    pub fn initialized_transfer(&self) -> Option<Arc<TransferManager>> {
        self.transfer.get().cloned()
    }
    pub fn initialized_print(&self) -> Option<Arc<PrintRuntime>> {
        self.print.get().cloned()
    }
    pub async fn transfer(
        &self,
        name: String,
        discoverable: bool,
    ) -> Result<Arc<TransferManager>, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.stopping.load(Ordering::Acquire) {
            return Err("application is stopping".into());
        }
        self.transfer
            .get_or_try_init(|| async {
                self.status("transfer", ModuleState::Starting, None);
                let mut config = self.transfer_config.clone();
                config.device_name = name;
                match TransferManager::with_resources(config, self.resources.clone()).await {
                    Ok(manager) => {
                        manager.set_discoverable(discoverable).await;
                        Ok(manager)
                    }
                    Err(error) => {
                        let error = error.to_string();
                        self.status("transfer", ModuleState::Unavailable, Some(error.clone()));
                        Err(error)
                    }
                }
            })
            .await
            .cloned()
    }
    pub async fn print(&self) -> Result<Arc<PrintRuntime>, String> {
        let _lifecycle = self.lifecycle.lock().await;
        if self.stopping.load(Ordering::Acquire) {
            return Err("application is stopping".into());
        }
        let result = self
            .print
            .get_or_try_init(|| async {
                self.status("print", ModuleState::Starting, None);
                let backend = Arc::new(
                    arcrelay_print::infrastructure::platform::SystemPrintBackend::with_resources(
                        self.resources.clone(),
                    ),
                );
                let adapters = PrintRuntimeAdapters::with_system_clock(
                    backend.clone(),
                    backend.clone(),
                    backend,
                );
                match PrintRuntime::open(self.print_config.clone(), adapters).await {
                    Ok(runtime) => {
                        if let Err(error) = runtime.recover().await {
                            self.status("print", ModuleState::Unavailable, Some(error.to_string()));
                            return Err(error.to_string());
                        }
                        Ok(Arc::new(runtime))
                    }
                    Err(error) => {
                        let error = error.to_string();
                        self.status("print", ModuleState::Unavailable, Some(error.clone()));
                        Err(error)
                    }
                }
            })
            .await
            .cloned();
        if result.is_ok() {
            self.status("print", ModuleState::Ready, None);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lazy_module_construction_does_not_create_runtime_resources() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let modules = RuntimeModules::new(
            Arc::new(arcrelay_content::ContentResources::default()),
            TransferConfig::new(
                "unused-config".into(),
                "unused-receive".into(),
                "test".into(),
            ),
            PrintRuntimeConfig {
                database_url: "sqlite::memory:".into(),
                spool_directory: "unused-spool".into(),
            },
            false,
        );
        assert!(modules.initialized_print().is_none());
        assert!(modules.initialized_transfer().is_none());
        assert!(modules
            .statuses()
            .iter()
            .all(|module| module.state == ModuleState::Idle));
    }
}
