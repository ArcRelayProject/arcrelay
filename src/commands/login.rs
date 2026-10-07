use super::*;
use crate::login::{LoginContext, RestoreTicket};
use arcrelay_core::{
    domain::login::*,
    infrastructure::login_vault::{LoginRestorePreview, LoginVaultStatus},
};
use serde::Deserialize;
use zeroize::{Zeroize, Zeroizing};

#[derive(Deserialize, ts_rs::TS)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LoginRequest {
    Status,
    Context,
    OpenSettings,
    CopyDraft {
        value: String,
    },
    PickApp,
    Initialize {
        password: String,
    },
    Unlock {
        password: String,
    },
    UnlockDevice,
    Lock,
    List {
        search: String,
        tag: Option<String>,
    },
    Save {
        draft: LoginDraft,
    },
    Remove {
        id: String,
    },
    Otp {
        id: String,
    },
    Reveal {
        id: String,
    },
    Use {
        id: String,
        field: LoginField,
        paste: bool,
        token: String,
    },
    Settings {
        settings: LoginSettings,
    },
    Device {
        enabled: bool,
    },
    ChangePassword {
        previous: String,
        password: String,
    },
    Export {
        password: String,
    },
    PreviewRestore {
        password: String,
        recovery: bool,
    },
    Restore {
        token: String,
        replace: bool,
    },
    Recover {
        token: String,
        password: String,
        confirmed: bool,
    },
}
#[derive(Default, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub status: Option<LoginVaultStatus>,
    pub context: Option<LoginContext>,
    pub entries: Option<Vec<LoginSummary>>,
    pub tags: Option<Vec<String>>,
    pub otp: Option<LoginOtp>,
    pub value: Option<String>,
    pub preview: Option<LoginRestorePreview>,
    pub token: Option<String>,
    pub count: Option<u32>,
    pub file: Option<String>,
    pub application: Option<LoginAppRule>,
    pub device_available: bool,
    pub device_enabled: bool,
}
fn owner(window: &tauri::WebviewWindow) -> Result<(), String> {
    if !matches!(window.label(), "main" | "clipboard") {
        return Err("Login information is accessible only from the local login interface".into());
    }
    let url = window
        .url()
        .map_err(|_| "Cannot verify the login interface")?;
    let local = matches!(url.scheme(), "tauri" | "asset")
        || url.host_str() == Some("tauri.localhost")
        || (cfg!(debug_assertions) && matches!(url.host_str(), Some("localhost" | "127.0.0.1")));
    if !local {
        return Err("Login information is accessible only from the local login interface".into());
    }
    Ok(())
}
fn backup_hash(path: &Path) -> Result<Vec<u8>, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|_| "Cannot read backup file")?;
    if file
        .metadata()
        .map_err(|_| "Cannot read backup file")?
        .len()
        > 16 * 1024 * 1024
    {
        return Err("Backup file is too large".into());
    }
    let mut bytes = Vec::new();
    file.take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read backup file")?;
    Ok(Sha256::digest(&bytes).to_vec())
}
#[arcrelay_desktop_ipc::command]
pub async fn login_request(
    window: tauri::WebviewWindow,
    state: State<'_, DesktopState>,
    app: AppHandle,
    request: LoginRequest,
) -> Result<LoginResponse, String> {
    owner(&window)?;
    #[cfg(target_os = "windows")]
    let native_window = window
        .hwnd()
        .map_err(|_| "Cannot access the login window")?
        .0 as isize;
    #[cfg(not(target_os = "windows"))]
    let native_window = 0;
    crate::presence_access::require_clipboard_access(&state)?;
    let login = state.login.clone();
    let mut response = LoginResponse::default();
    match request {
        LoginRequest::PickApp => {
            #[cfg(target_os = "macos")]
            let selected = rfd::AsyncFileDialog::new()
                .set_title("选择应用（.app）")
                .set_directory("/Applications")
                .add_filter("macOS 应用", &["app"])
                .pick_file()
                .await;
            #[cfg(not(target_os = "macos"))]
            let selected = rfd::AsyncFileDialog::new()
                .set_title("选择应用")
                .add_filter("应用", &["exe", "desktop"])
                .pick_file()
                .await;
            if let Some(file) = selected {
                let path = file.path().to_owned();
                let name = path
                    .file_stem()
                    .map(|v| v.to_string_lossy().into_owned())
                    .ok_or("Invalid application name")?;
                #[cfg(target_os = "macos")]
                let id = objc2_foundation::NSBundle::bundleWithPath(
                    &objc2_foundation::NSString::from_str(&path.to_string_lossy()),
                )
                .and_then(|b| b.bundleIdentifier())
                .map(|v| v.to_string())
                .ok_or("Select a valid macOS application")?;
                #[cfg(not(target_os = "macos"))]
                let id = std::fs::canonicalize(&path)
                    .map_err(|_| "Cannot identify the selected application")?
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_lowercase();
                response.application = Some(LoginAppRule {
                    id,
                    name,
                    enabled: true,
                    priority: 80,
                });
            }
        }
        LoginRequest::OpenSettings => {
            crate::windowing::hide_clipboard_window(&app)
                .map_err(|_| "Cannot close the clipboard window")?;
            crate::windowing::open_login_settings(&app)?;
        }
        LoginRequest::Context => {
            let context = crate::login::context(&app)?;
            *login
                .context
                .lock()
                .map_err(|_| "Application identification is unavailable")? = context.clone();
            response.context = Some(context);
        }
        LoginRequest::CopyDraft { value } => {
            let value = Zeroizing::new(value);
            let _action = clipboard::begin_clipboard_action()?;
            let session = login.vault.status()?;
            if !session.unlocked || value.is_empty() || value.len() > 16384 {
                return Err("Unlock first, then copy a valid field".into());
            }
            let receipt = state
                .clipboard
                .set_ephemeral_text(&value)
                .await
                .map_err(|e| e.to_string())?;
            let current = login.vault.status()?;
            if !current.unlocked || current.session_version != session.session_version {
                let _ = state.clipboard.clear_ephemeral_text(&receipt).await;
                return Err("Login protection session has changed; unlock again".into());
            }
            let clipboard = state.clipboard.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(u64::from(
                    session.settings.clear_seconds,
                )))
                .await;
                let _ = clipboard.clear_ephemeral_text(&receipt).await;
            });
        }
        LoginRequest::Use {
            id,
            field,
            paste,
            token,
        } => {
            let session = login.vault.status()?;
            if !session.unlocked {
                return Err("Unlock login information first".into());
            }
            let _action = clipboard::begin_clipboard_action()?;
            let expected = login
                .context
                .lock()
                .map_err(|_| "Application identification is unavailable")?
                .clone();
            let target = clipboard::capture_paste_target(&app)?;
            if paste
                && (token.is_empty()
                    || token != expected.token
                    || (crate::login::context(&app)?.token != token
                        || crate::login::context(&app)?.id != expected.id))
            {
                return Err(
                    "Current application has changed; reopen the clipboard before inserting".into(),
                );
            }
            let restore = if paste {
                clipboard::require_input_permission(&state, &app).await?;
                #[cfg(target_os = "macos")]
                let prepared_target = target.clone();
                #[cfg(not(target_os = "macos"))]
                let prepared_target = target;
                clipboard::prepare_window_and_wait_for_paste(&app, prepared_target).await?
            } else {
                false
            };
            let result = async {
                if paste {
                    clipboard::ensure_paste_target_unchanged(&app, &target)?;
                }
                let vault = login.clone();
                let entry_id = id.clone();
                let chosen = field;
                let value =
                    tokio::task::spawn_blocking(move || vault.vault.field(&entry_id, chosen))
                        .await
                        .map_err(|_| "Login information read failed")??;
                let seconds = if matches!(field, LoginField::Totp) {
                    let vault = login.clone();
                    let entry_id = id.clone();
                    let otp = tokio::task::spawn_blocking(move || vault.vault.otp(&entry_id))
                        .await
                        .map_err(|_| "TOTP read failed")??;
                    ((otp.expires_at_ms - chrono::Utc::now().timestamp_millis()).max(0) as u64)
                        .div_ceil(1000)
                } else {
                    u64::from(login.vault.status()?.settings.clear_seconds)
                };
                // Recheck after reading and before the native write and key event.
                if paste {
                    clipboard::ensure_paste_target_unchanged(&app, &target)?;
                }
                let current = login.vault.status()?;
                if !current.unlocked || current.session_version != session.session_version {
                    return Err("Login protection session has changed; unlock again".into());
                }
                let receipt = state
                    .clipboard
                    .set_ephemeral_text(&value)
                    .await
                    .map_err(|error| error.to_string())?;
                drop(value);
                let current = login.vault.status()?;
                if !current.unlocked || current.session_version != session.session_version {
                    let _ = state.clipboard.clear_ephemeral_text(&receipt).await;
                    return Err("Login protection session has changed; unlock again".into());
                }
                let clipboard = state.clipboard.clone();
                let cleanup = receipt.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
                    let _ = clipboard.clear_ephemeral_text(&cleanup).await;
                });
                if paste {
                    let pasted = clipboard::ensure_paste_target_unchanged(&app, &target);
                    if let Err(e) = pasted {
                        let _ = state.clipboard.clear_ephemeral_text(&receipt).await;
                        return Err(e);
                    }
                    state
                        .clipboard
                        .paste_prepared(ClipboardContentKind::Text)
                        .await
                        .map_err(|_| "Credential insertion failed; paste manually")?;
                }
                let vault = login.clone();
                tokio::task::spawn_blocking(move || vault.vault.touch(&id))
                    .await
                    .map_err(|_| "Cannot save login usage metadata")??;
                Ok::<(), String>(())
            }
            .await;
            if paste {
                let _ = crate::windowing::restore_clipboard_window_after_paste(&app, restore);
            }
            result?;
        }
        LoginRequest::Export { password } => {
            let password = Zeroizing::new(password);
            let file = rfd::AsyncFileDialog::new()
                .set_title("导出加密登录备份")
                .add_filter("ArcRelay 登录备份", &["arclogin"])
                .set_file_name("ArcRelay-logins.arclogin")
                .save_file()
                .await;
            if let Some(file) = file {
                let path = file.path().to_owned();
                let name = path.display().to_string();
                let vault = login.clone();
                tokio::task::spawn_blocking(move || vault.vault.export_backup(&path, &password))
                    .await
                    .map_err(|_| "Export failed")??;
                response.file = Some(name);
            }
        }
        LoginRequest::PreviewRestore { password, recovery } => {
            let password = Zeroizing::new(password);
            let file = rfd::AsyncFileDialog::new()
                .set_title("选择加密登录备份")
                .add_filter("ArcRelay 登录备份", &["arclogin"])
                .pick_file()
                .await;
            if let Some(file) = file {
                let path = file.path().to_owned();
                let vault = login.clone();
                let (preview, token) = tokio::task::spawn_blocking(move || {
                    let hash = backup_hash(&path)?;
                    let preview = if recovery {
                        vault.vault.preview_recovery(&path, &password)?
                    } else {
                        vault.vault.preview_backup(&path, &password)?
                    };
                    if hash != backup_hash(&path)? {
                        return Err("Backup file has changed; select it again".into());
                    }
                    let token = uuid::Uuid::new_v4().to_string();
                    let ticket = RestoreTicket {
                        token: token.clone(),
                        path,
                        password,
                        hash,
                        vault_id: vault.vault.status()?.vault_id.unwrap_or_default(),
                        recovery,
                        revision: vault.vault.storage_revision()?,
                        expires: std::time::Instant::now() + std::time::Duration::from_secs(180),
                    };
                    *vault
                        .restore
                        .lock()
                        .map_err(|_| "Restore service is unavailable")? = Some(ticket);
                    Ok::<_, String>((preview, token))
                })
                .await
                .map_err(|_| "Backup read failed")??;
                response.preview = Some(preview);
                response.token = Some(token);
            }
        }
        request => {
            response = tokio::task::spawn_blocking(move || {
                let mut r = LoginResponse::default();
                match request {
                    LoginRequest::Status => {}
                    LoginRequest::Initialize { password } => {
                        login.vault.initialize(&Zeroizing::new(password))?
                    }
                    LoginRequest::Unlock { password } => {
                        login.vault.unlock(&Zeroizing::new(password))?
                    }
                    LoginRequest::UnlockDevice => login.unlock_device(native_window)?,
                    LoginRequest::Lock => {
                        login.lock();
                        let _ = app.emit("login-vault-locked", ());
                    }
                    LoginRequest::List { search, tag } => {
                        let context = login
                            .context
                            .lock()
                            .map_err(|_| "Application identification is unavailable")?
                            .clone();
                        r.entries = Some(login.vault.list(
                            context.id.as_deref(),
                            &search,
                            tag.as_deref(),
                        )?);
                        r.tags = Some(login.vault.tags()?);
                    }
                    LoginRequest::Save { draft } => {
                        r.value = Some(login.vault.upsert(draft)?);
                    }
                    LoginRequest::Remove { id } => login.vault.remove(&id)?,
                    LoginRequest::Otp { id } => r.otp = Some(login.vault.otp(&id)?),
                    LoginRequest::Reveal { id } => {
                        r.value = Some(login.vault.field(&id, LoginField::Password)?.to_string())
                    }
                    LoginRequest::Settings { settings } => login.vault.settings(settings)?,
                    LoginRequest::Device { enabled } => {
                        login.enable_device(enabled, native_window)?
                    }
                    LoginRequest::ChangePassword { previous, password } => {
                        let id = login
                            .vault
                            .status()?
                            .vault_id
                            .ok_or("Login protection is not enabled")?;
                        login.vault.change_password(
                            &Zeroizing::new(previous),
                            &Zeroizing::new(password),
                        )?;
                        login.revoke_device(&id);
                    }
                    LoginRequest::Restore { token, replace } => {
                        let ticket = login
                            .restore
                            .lock()
                            .map_err(|_| "Restore service is unavailable")?
                            .take()
                            .ok_or("Restore preview has expired; select the backup again")?;
                        if ticket.recovery
                            || ticket.token != token
                            || ticket.expires < std::time::Instant::now()
                            || Some(ticket.vault_id) != login.vault.status()?.vault_id
                            || ticket.revision != login.vault.storage_revision()?
                            || ticket.hash != backup_hash(&ticket.path)?
                        {
                            return Err(
                                "Backup file or login protection has changed; preview again".into(),
                            );
                        }
                        r.count = Some(login.vault.restore_backup(
                            &ticket.path,
                            &ticket.password,
                            replace,
                        )?);
                    }
                    LoginRequest::Recover {
                        token,
                        password,
                        confirmed,
                    } => {
                        let password = Zeroizing::new(password);
                        let ticket = login
                            .restore
                            .lock()
                            .map_err(|_| "Restore service is unavailable")?
                            .take()
                            .ok_or("Restore preview has expired; select the backup again")?;
                        let previous = login.vault.status()?.vault_id.unwrap_or_default();
                        if !confirmed
                            || !ticket.recovery
                            || ticket.token != token
                            || ticket.expires < std::time::Instant::now()
                            || ticket.vault_id != previous
                            || ticket.revision != login.vault.storage_revision()?
                            || ticket.hash != backup_hash(&ticket.path)?
                        {
                            return Err(
                                "Invalid restore confirmation or backup preview; select again"
                                    .into(),
                            );
                        }
                        r.count = Some(login.vault.recover_backup(
                            &ticket.path,
                            &ticket.password,
                            &password,
                            &ticket.revision,
                        )?);
                        login.revoke_device(&previous);
                    }
                    _ => return Err("Invalid login operation".into()),
                }
                r.status = Some(login.vault.status()?);
                r.device_available = crate::login::device_available();
                r.device_enabled = login.device_enabled();
                Ok::<_, String>(r)
            })
            .await
            .map_err(|_| "Login protection operation failed")??;
            scrub_locked(&mut response);
            return Ok(response);
        }
    }
    response.status = Some(state.login.vault.status()?);
    response.device_available = crate::login::device_available();
    response.device_enabled = state.login.device_enabled();
    scrub_locked(&mut response);
    Ok(response)
}
#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_login_ipc.rs"));

fn scrub_locked(response: &mut LoginResponse) {
    if response.status.as_ref().is_some_and(|s| !s.unlocked) {
        if let Some(value) = response.value.as_mut() {
            value.zeroize();
        }
        response.value = None;
        response.entries = None;
        response.tags = None;
        response.otp = None;
    }
}
