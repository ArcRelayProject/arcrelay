//! Credential APIs are local WebView commands only, deliberately absent from MCP and sync.
mod device;
use arcrelay_core::infrastructure::login_vault::LoginVault;
use serde::Serialize;
use std::{path::PathBuf, sync::Mutex};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LoginContext {
    pub id: Option<String>,
    pub name: Option<String>,
    pub token: String,
}
pub struct RestoreTicket {
    pub token: String,
    pub path: PathBuf,
    pub password: zeroize::Zeroizing<String>,
    pub hash: Vec<u8>,
    pub vault_id: String,
    pub recovery: bool,
    pub revision: String,
    pub expires: std::time::Instant,
}
pub struct LoginService {
    pub vault: LoginVault,
    pub context: Mutex<LoginContext>,
    pub restore: Mutex<Option<RestoreTicket>>,
    device_path: PathBuf,
    clipboard: Mutex<
        Option<
            std::sync::Arc<
                arcrelay_core::application::clipboard_service::ClipboardApplicationService,
            >,
        >,
    >,
}
impl LoginService {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ArcRelay");
        Self {
            vault: LoginVault::new(dir.join("logins.arclogin")),
            context: Mutex::default(),
            restore: Mutex::default(),
            device_path: dir.join("login-device.json"),
            clipboard: Mutex::default(),
        }
    }
    pub fn attach_clipboard(
        &self,
        clipboard: std::sync::Arc<
            arcrelay_core::application::clipboard_service::ClipboardApplicationService,
        >,
    ) {
        if let Ok(mut saved) = self.clipboard.lock() {
            *saved = Some(clipboard);
        }
    }
    fn clear_private(&self) {
        let clipboard = self.clipboard.lock().ok().and_then(|saved| saved.clone());
        if let Some(clipboard) = clipboard {
            tauri::async_runtime::spawn(async move {
                let _ = clipboard.clear_owned_ephemeral_text().await;
            });
        }
    }
    pub fn close(&self) {
        if self.vault.close() {
            self.clear_private();
        }
    }
    pub fn lock(&self) {
        self.vault.lock();
        self.clear_private();
        if let Ok(mut p) = self.restore.lock() {
            *p = None;
        }
    }
    pub fn device_enabled(&self) -> bool {
        let id = self.vault.status().ok().and_then(|s| s.vault_id);
        id.is_some() && std::fs::read_to_string(&self.device_path).ok().as_ref() == id.as_ref()
    }
    pub fn enable_device(&self, enable: bool, window: isize) -> Result<(), String> {
        let (id, key) = self.vault.device_key()?;
        if enable {
            device::store(&id, &key, window)?;
            std::fs::write(&self.device_path, &id)
                .map_err(|_| "Cannot save quick unlock settings")?;
        } else {
            device::remove(&id)?;
            match std::fs::remove_file(&self.device_path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err("Cannot remove quick unlock settings".into())
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub fn unlock_device(&self, window: isize) -> Result<(), String> {
        if !self.device_enabled() {
            return Err("Enable system quick unlock using the master password first".into());
        }
        let id = self
            .vault
            .status()?
            .vault_id
            .ok_or("Login protection is not enabled")?;
        self.vault
            .unlock_with_device_key(&id, device::load(&id, window)?)
    }
    pub fn revoke_device(&self, old_id: &str) {
        let _ = device::remove(old_id);
        let _ = std::fs::remove_file(&self.device_path);
    }
}
pub fn device_available() -> bool {
    device::available()
}

pub fn context(app: &AppHandle) -> Result<LoginContext, String> {
    #[cfg(target_os = "macos")]
    {
        let recipient =
            crate::windowing::clipboard_paste_recipient(app).map_err(|e| e.to_string())?;
        Ok(recipient
            .map(|a| LoginContext {
                id: a.bundleIdentifier().map(|v| v.to_string()),
                name: a.localizedName().map(|v| v.to_string()),
                token: a
                    .launchDate()
                    .map(|d| format!("{}:{}", a.processIdentifier(), d.timeIntervalSince1970()))
                    .unwrap_or_default(),
            })
            .unwrap_or_default())
    }
    #[cfg(target_os = "windows")]
    {
        let _ = app;
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let target = crate::windowing::clipboard_windows::capture_paste_target()?;
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, target.process)
                .map_err(|_| "Cannot identify the current application")?;
            let mut buffer = vec![0u16; 32768];
            let mut n = buffer.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                windows_core::PWSTR(buffer.as_mut_ptr()),
                &mut n,
            );
            let _ = CloseHandle(process);
            result.map_err(|_| "Cannot identify the current application")?;
            let path = String::from_utf16_lossy(&buffer[..n as usize]);
            Ok(LoginContext {
                name: PathBuf::from(&path)
                    .file_stem()
                    .map(|v| v.to_string_lossy().into_owned()),
                id: Some(path.to_lowercase()),
                token: format!("{}:{}:{}", target.process, target.window, target.focus),
            })
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Ok(LoginContext::default())
    }
}

pub fn start_guard(state: crate::backend::DesktopState, app: AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
        use objc2_foundation::NSNotification;
        let login = state.login.clone();
        let app = app.clone();
        let block = block2::RcBlock::new(move |_: std::ptr::NonNull<NSNotification>| {
            login.lock();
            let _ = app.emit("login-vault-locked", ());
        });
        unsafe {
            let _ = NSWorkspace::sharedWorkspace()
                .notificationCenter()
                .addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceWillSleepNotification),
                    None,
                    None,
                    &block,
                );
        }
    }
    #[cfg(target_os = "windows")]
    device::observe_sleep(state.login.clone(), app.clone());
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(1));
        let mut unlocked = false;
        let mut last_tick = std::time::Instant::now();
        loop {
            ticker.tick().await;
            // A long clock gap also invalidates sessions after suspend on platforms without sleep callbacks.
            if last_tick.elapsed() > std::time::Duration::from_secs(3)
                || crate::application::automations::platform::locked()
                || crate::presence_access::clipboard_locked(&state)
            {
                state.login.lock();
            }
            last_tick = std::time::Instant::now();
            use tauri::Manager;
            if app
                .get_webview_window("clipboard")
                .is_some_and(|w| w.is_visible().unwrap_or(false))
            {
                if let Ok(next) = context(&app) {
                    if let Ok(mut saved) = state.login.context.lock() {
                        if next.token != saved.token || next.id != saved.id {
                            *saved = next.clone();
                            let _ = app.emit_to("clipboard", "login-context-changed", next);
                        }
                    }
                }
            }
            if let Ok(mut ticket) = state.login.restore.lock() {
                if ticket
                    .as_ref()
                    .is_some_and(|t| t.expires <= std::time::Instant::now())
                {
                    *ticket = None;
                }
            }
            let now = state
                .login
                .vault
                .status()
                .map(|s| s.unlocked)
                .unwrap_or(false);
            if unlocked && !now {
                state.login.lock();
                let _ = app.emit("login-vault-locked", ());
            }
            unlocked = now;
        }
    });
}

#[cfg(test)]
#[test]
fn login_host_constructor_needs_no_tokio_runtime() {
    let service = LoginService::new();
    assert!(service.restore.lock().unwrap().is_none());
    assert!(service.context.lock().unwrap().token.is_empty());
}
