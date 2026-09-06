use super::*;
use std::sync::{Arc, Mutex};
use tauri::menu::{CheckMenuItem, PredefinedMenuItem, Submenu};

use crate::settings::{AppSettingsPatch, LanguagePreference};

mod collaboration;
use collaboration::{CollaborationState, MAX_DEVICES};

struct TrayRuntime {
    collaboration: Mutex<CollaborationState>,
    #[cfg(target_os = "linux")]
    menu: Menu<tauri::Wry>,
}

#[derive(Default)]
struct PendingNavigation(Mutex<Option<String>>);

impl PendingNavigation {
    fn set(&self, destination: &str) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(destination.to_owned());
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

static PENDING_NAVIGATION: PendingNavigation = PendingNavigation(Mutex::new(None));

pub fn take_pending_tray_navigation() -> Option<String> {
    PENDING_NAVIGATION.take()
}

// Both native menus and the web UI read these keys from the same catalogs.
const TEXTS: &[&str] = &[
    "打开 ArcRelay",
    "打开剪贴板",
    "发送文件…",
    "打开接收文件夹",
    "剪贴板同步",
    "投屏隐私模式",
    "提示音",
    "提示音静音 1 小时",
    "恢复提示音",
    "已静音，直到 {time}",
    "设置…",
    "帮助与诊断",
    "打开日志文件夹",
    "导出诊断包…",
    "退出 ArcRelay",
    "选择要发送的文件",
    "操作未完成",
    "无法完成托盘操作，请重试。",
    "诊断包已导出至：{path}",
    "诊断包已导出",
    "ArcRelay · 已连接 {count} 台设备",
    "设备",
    "管理设备…",
    "暂无可用设备",
    "已连接",
    "附近",
    "离线",
    "浏览文件…",
    "断开连接",
    "传输未结束，暂不可断开",
    "附近传输尚未就绪",
    "暂无进行中的附近传输",
    "附近传输：进行中 {active} · 等待确认 {waiting} · 已暂停 {paused}",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum Label {
    Open,
    Clipboard,
    SendFiles,
    ReceiveDirectory,
    Sync,
    Privacy,
    Sounds,
    Mute,
    Unmute,
    MuteStatus,
    Settings,
    Help,
    Logs,
    Diagnostics,
    Quit,
    ChooseFiles,
    ErrorTitle,
    ErrorMessage,
    ExportedPath,
    ExportedTitle,
    ConnectionSummary,
    Devices,
    ManageDevices,
    NoDevices,
    Connected,
    Nearby,
    Offline,
    BrowseFiles,
    Disconnect,
    DisconnectBusy,
    TransferUnavailable,
    TransferIdle,
    TransferSummary,
}

fn text(language: LanguagePreference, index: Label) -> String {
    crate::localization::translate(language, TEXTS[index as usize], TEXTS[index as usize])
}

#[derive(Debug, PartialEq, Eq)]
struct MenuState {
    language: LanguagePreference,
    clipboard_enabled: bool,
    clipboard_shortcut: String,
    sync_enabled: bool,
    privacy_active: bool,
    mute_until: Option<i64>,
    collaboration: CollaborationState,
}

impl MenuState {
    fn read(app: &AppHandle) -> Self {
        let state = app.state::<DesktopState>();
        let settings = state.settings.snapshot();
        Self {
            language: settings.language.resolve(),
            clipboard_enabled: settings.clipboard_enabled,
            clipboard_shortcut: settings.clipboard_shortcut,
            sync_enabled: settings.clipboard_sync_enabled,
            privacy_active: state.privacy.snapshot().active,
            mute_until: crate::sound::temporary_mute_until(),
            collaboration: app
                .try_state::<TrayRuntime>()
                .map(|runtime| {
                    runtime
                        .collaboration
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clone()
                })
                .unwrap_or_default(),
        }
    }

    fn sound_action(&self) -> (&'static str, Label) {
        if self.mute_until.is_some() {
            ("sound-unmute", Label::Unmute)
        } else {
            ("sound-mute", Label::Mute)
        }
    }
}

fn build_menu(app: &AppHandle, state: &MenuState) -> tauri::Result<Menu<tauri::Wry>> {
    let label = |index| text(state.language, index);
    let item = |id, index| MenuItem::with_id(app, id, label(index), true, None::<&str>);
    let show = item("show", Label::Open)?;
    let status = MenuItem::with_id(
        app,
        "devices",
        label(Label::ConnectionSummary)
            .replace("{count}", &state.collaboration.connected_count.to_string()),
        true,
        None::<&str>,
    )?;
    let summary = &state.collaboration.transfers;
    let summary_label = if !summary.ready {
        label(Label::TransferUnavailable)
    } else if summary.active + summary.waiting + summary.paused == 0 {
        label(Label::TransferIdle)
    } else {
        label(Label::TransferSummary)
            .replace("{active}", &summary.active.to_string())
            .replace("{waiting}", &summary.waiting.to_string())
            .replace("{paused}", &summary.paused.to_string())
    };
    let transfers = MenuItem::with_id(app, "transfers", summary_label, true, None::<&str>)?;
    let devices = Submenu::new(app, label(Label::Devices), true)?;
    for device in state.collaboration.devices.iter().take(MAX_DEVICES) {
        let status_label = if device.connected {
            Label::Connected
        } else if device.nearby {
            Label::Nearby
        } else {
            Label::Offline
        };
        let mut name = collaboration::device_name(&device.name);
        if state
            .collaboration
            .devices
            .iter()
            .filter(|peer| peer.name == device.name)
            .count()
            > 1
        {
            let suffix: String = device
                .id
                .chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            name = format!("{name} ({})", collaboration::device_name(&suffix));
        }
        let submenu = Submenu::new(app, format!("{name} · {}", label(status_label)), true)?;
        if device.can_send {
            submenu.append(&MenuItem::with_id(
                app,
                format!("device-send:{}", device.id),
                label(Label::SendFiles),
                true,
                None::<&str>,
            )?)?;
        }
        if device.can_browse {
            submenu.append(&MenuItem::with_id(
                app,
                format!("device-files:{}", device.id),
                label(Label::BrowseFiles),
                true,
                None::<&str>,
            )?)?;
        }
        if device.connected {
            submenu.append(&MenuItem::with_id(
                app,
                format!("device-disconnect:{}", device.id),
                label(if device.transfer_pending {
                    Label::DisconnectBusy
                } else {
                    Label::Disconnect
                }),
                device.can_disconnect(),
                None::<&str>,
            )?)?;
        }
        submenu.append(&item("devices", Label::ManageDevices)?)?;
        devices.append(&submenu)?;
    }
    if state.collaboration.devices.is_empty() {
        devices.append(&MenuItem::with_id(
            app,
            "no-devices",
            label(Label::NoDevices),
            false,
            None::<&str>,
        )?)?;
    }
    devices.append(&PredefinedMenuItem::separator(app)?)?;
    devices.append(&item("devices", Label::ManageDevices)?)?;
    let clipboard = MenuItem::with_id(
        app,
        "clipboard",
        label(Label::Clipboard),
        state.clipboard_enabled,
        (!state.clipboard_shortcut.is_empty()).then_some(state.clipboard_shortcut.as_str()),
    )?;
    let send = item("send-files", Label::SendFiles)?;
    let receive = item("receive-directory", Label::ReceiveDirectory)?;
    let sync = CheckMenuItem::with_id(
        app,
        "clipboard-sync",
        label(Label::Sync),
        true,
        state.sync_enabled,
        None::<&str>,
    )?;
    let privacy = CheckMenuItem::with_id(
        app,
        "privacy",
        label(Label::Privacy),
        true,
        state.privacy_active,
        None::<&str>,
    )?;
    let sounds = Submenu::new(app, label(Label::Sounds), true)?;
    if let Some(until) = state
        .mute_until
        .and_then(chrono::DateTime::from_timestamp_millis)
    {
        let time = until
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string();
        sounds.append(&MenuItem::with_id(
            app,
            "mute-status",
            label(Label::MuteStatus).replace("{time}", &time),
            false,
            None::<&str>,
        )?)?;
    }
    let (sound_id, sound_label) = state.sound_action();
    sounds.append(&item(sound_id, sound_label)?)?;
    let settings = item("settings", Label::Settings)?;
    let help = Submenu::with_items(
        app,
        label(Label::Help),
        true,
        &[
            &item("logs", Label::Logs)?,
            &item("diagnostics", Label::Diagnostics)?,
        ],
    )?;
    let quit = item("quit", Label::Quit)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &transfers,
            &PredefinedMenuItem::separator(app)?,
            &show,
            &clipboard,
            &send,
            &receive,
            &PredefinedMenuItem::separator(app)?,
            &devices,
            &PredefinedMenuItem::separator(app)?,
            &sync,
        ],
    )?;
    // Linux currently has no native screen-protection backend.
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        menu.append(&privacy)?;
    }
    menu.append_items(&[
        &sounds,
        &PredefinedMenuItem::separator(app)?,
        &settings,
        &help,
        &PredefinedMenuItem::separator(app)?,
        &quit,
    ])?;
    Ok(menu)
}

pub fn setup_tray(app: &mut App) -> tauri::Result<()> {
    let state = MenuState::read(app.handle());
    let menu = build_menu(app.handle(), &state)?;
    app.manage(TrayRuntime {
        collaboration: Mutex::new(CollaborationState::default()),
        #[cfg(target_os = "linux")]
        menu: menu.clone(),
    });
    let mut builder = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let app = app.clone();
            let id = event.id.as_ref().to_owned();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = handle_action(&app, &id).await {
                    tracing::warn!(%error, action = %id, "Tray action failed");
                    let language = app.state::<DesktopState>().settings.snapshot().language;
                    rfd::AsyncMessageDialog::new()
                        .set_title(text(language, Label::ErrorTitle))
                        .set_description(text(language, Label::ErrorMessage))
                        .set_level(rfd::MessageLevel::Error)
                        .show()
                        .await;
                }
                if let Err(error) = refresh_tray_menu(&app) {
                    tracing::warn!(%error, "Could not refresh tray menu");
                }
            });
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) && !super::tray_transfer::reopen(tray.app_handle())
            {
                request_main_window(tray.app_handle().clone());
            }
        });
    #[cfg(not(target_os = "linux"))]
    {
        builder = builder.tooltip("ArcRelay").show_menu_on_left_click(false);
    }
    #[cfg(target_os = "macos")]
    {
        builder = builder
            .icon(Image::from_bytes(include_bytes!(
                "../../icons/tray-icon.png"
            ))?)
            .icon_as_template(true);
    }
    #[cfg(not(target_os = "macos"))]
    {
        builder = builder.icon(Image::from_bytes(include_bytes!("../../icons/icon.png"))?);
    }
    builder.build(app)?;
    #[cfg(target_os = "macos")]
    if let Err(error) = super::tray_transfer::setup(app.handle()) {
        tracing::warn!(%error, "Could not enable menu bar file drop");
    }

    // Run in Tauri's long-lived runtime; constructors above need no Tokio context.
    // Observe backend changes even when the main webview has never been created.
    let app = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let mut previous = state;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if app.tray_by_id("main-tray").is_none()
                || app.state::<DesktopState>().quitting.load(Ordering::Acquire)
            {
                break;
            }
            let collaboration = CollaborationState::read(&app.state::<DesktopState>()).await;
            *app.state::<TrayRuntime>()
                .collaboration
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = collaboration;
            let next = MenuState::read(&app);
            if next != previous {
                match refresh_tray_menu(&app) {
                    Ok(()) => previous = next,
                    Err(error) => tracing::warn!(%error, "Could not refresh tray state"),
                }
            }
        }
    });
    Ok(())
}

pub(super) fn navigate(app: &AppHandle, destination: &str) -> Result<(), String> {
    PENDING_NAVIGATION.set(destination);
    ensure_main_window(app).map_err(|e| e.to_string())?;
    app.emit_to("main", "tray-navigation-pending", ())
        .map_err(|e| e.to_string())
}

async fn handle_action(app: &AppHandle, id: &str) -> Result<(), String> {
    for (prefix, capability) in [
        ("device-send:", "send"),
        ("device-files:", "files"),
        ("device-disconnect:", "disconnect"),
    ] {
        if let Some(peer_id) = id.strip_prefix(prefix) {
            let current = CollaborationState::read(&app.state::<DesktopState>()).await;
            let device = current
                .devices
                .iter()
                .find(|device| device.id == peer_id)
                .ok_or("device is no longer available")?;
            match capability {
                "send" if device.can_send => stage_files(app, Some(peer_id.to_owned())).await?,
                "files" if device.can_browse => navigate(app, &format!("files:{peer_id}"))?,
                "disconnect" if device.can_disconnect() => crate::commands::disconnect_device(
                    app.state::<DesktopState>(),
                    peer_id.to_owned(),
                )
                .await
                .map_err(|e| e.message)?,
                _ => return Err("device capability is no longer available".into()),
            }
            return Ok(());
        }
    }
    match id {
        "show" => ensure_main_window(app).map_err(|e| e.to_string())?,
        "clipboard" => show_clipboard_window(app).map_err(|e| e.to_string())?,
        "privacy" => {
            app.state::<DesktopState>().privacy.toggle_local();
        }
        "clipboard-sync" => {
            let state = app.state::<DesktopState>();
            let enabled = !state.settings.snapshot().clipboard_sync_enabled;
            crate::application::settings_service::update(
                app,
                &state,
                AppSettingsPatch {
                    clipboard_sync_enabled: Some(enabled),
                    ..Default::default()
                },
            )
            .await?;
        }
        "sound-mute" | "sound-unmute" => {
            crate::commands::set_sound_temporary_mute(app.clone(), id == "sound-mute")
                .map_err(|e| e.message)?;
        }
        "settings" => navigate(app, "settings")?,
        "devices" => navigate(app, "devices")?,
        "transfers" => navigate(app, "transfers")?,
        "send-files" => stage_files(app, None).await?,
        "receive-directory" => {
            crate::commands::open_transfer_receive_directory(app.state::<DesktopState>())
                .await
                .map_err(|e| e.message)?
        }
        "logs" => {
            tauri::async_runtime::spawn_blocking(crate::commands::open_log_directory)
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| e.message)?;
        }
        "diagnostics" => {
            let bundle = crate::commands::export_diagnostic_bundle(
                app.state::<Arc<crate::arc_input::ArcInputRuntime>>(),
            )
            .await
            .map_err(|e| e.message)?;
            let language = app.state::<DesktopState>().settings.snapshot().language;
            rfd::AsyncMessageDialog::new()
                .set_title(text(language, Label::ExportedTitle))
                .set_description(
                    text(language, Label::ExportedPath).replace("{path}", &bundle.path),
                )
                .show()
                .await;
        }
        "quit" => {
            app.state::<DesktopState>()
                .inner()
                .clone()
                .shutdown_and_exit(app.clone())
                .await;
        }
        _ => {}
    }
    Ok(())
}

async fn stage_files(app: &AppHandle, target_peer_id: Option<String>) -> Result<(), String> {
    let language = app.state::<DesktopState>().settings.snapshot().language;
    if let Some(files) = rfd::AsyncFileDialog::new()
        .set_title(text(language, Label::ChooseFiles))
        .pick_files()
        .await
    {
        if files.is_empty() {
            return Ok(());
        }
        if let Some(peer_id) = &target_peer_id {
            let current = CollaborationState::read(&app.state::<DesktopState>()).await;
            if !current
                .devices
                .iter()
                .any(|device| &device.id == peer_id && device.can_send)
            {
                return Err("recipient is no longer available".into());
            }
        }
        let service = app
            .state::<Arc<crate::system_share::SystemShareService>>()
            .inner()
            .clone();
        tauri::async_runtime::spawn_blocking(move || {
            service.enqueue_paths(
                crate::system_share::SystemShareSource::Tray,
                target_peer_id,
                files
                    .into_iter()
                    .map(|file| file.path().to_path_buf())
                    .collect(),
            )
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
        ensure_main_window(app).map_err(|e| e.to_string())?;
        crate::system_share::announce_pending(
            app,
            &app.state::<Arc<crate::system_share::SystemShareService>>(),
        );
    }
    Ok(())
}

pub fn refresh_tray_menu(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        dispatch_appkit(app, "refresh tray menu", |app| {
            refresh_tray_menu_on_main(&app)
        })
    }
    #[cfg(not(target_os = "macos"))]
    refresh_tray_menu_on_main(app)
}

fn refresh_tray_menu_on_main(app: &AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    debug_assert!(objc2::MainThreadMarker::new().is_some());
    if let Some(tray) = app.tray_by_id("main-tray") {
        let next = build_menu(app, &MenuState::read(app))?;
        #[cfg(target_os = "linux")]
        {
            // AppIndicator retains the original GTK menu: replace its contents,
            // never its identity. Build the complete replacement before mutation.
            let runtime = app.state::<TrayRuntime>();
            replace_menu_contents(&runtime.menu, next)?;
            let _ = tray;
        }
        #[cfg(not(target_os = "linux"))]
        tray.set_menu(Some(next))?;
    }
    Ok(())
}

// Compile this path on every host even though only AppIndicator requires it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn replace_menu_contents(current: &Menu<tauri::Wry>, next: Menu<tauri::Wry>) -> tauri::Result<()> {
    let previous = current.items()?;
    let replacements = next.items()?;
    for item in &replacements {
        next.remove(item)?;
    }
    let result = (|| {
        for item in &previous {
            current.remove(item)?;
        }
        for item in &replacements {
            current.append(item)?;
        }
        Ok(())
    })();
    if result.is_err() {
        // Preserve access to Open/Quit if a native menu mutation fails.
        if let Ok(items) = current.items() {
            for item in &items {
                let _ = current.remove(item);
            }
        }
        for item in &previous {
            let _ = current.append(item);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_navigation_survives_window_startup_without_runtime() {
        let pending = PendingNavigation::default();
        assert_eq!(pending.take(), None);
        pending.set("settings");
        assert_eq!(pending.take().as_deref(), Some("settings"));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn mute_action_tracks_backend_expiry_without_runtime() {
        let mut state = MenuState {
            language: LanguagePreference::EnUs,
            clipboard_enabled: true,
            clipboard_shortcut: String::new(),
            sync_enabled: true,
            privacy_active: false,
            mute_until: None,
            collaboration: CollaborationState::default(),
        };
        assert_eq!(state.sound_action(), ("sound-mute", Label::Mute));
        state.mute_until = Some(1234);
        assert_eq!(state.sound_action(), ("sound-unmute", Label::Unmute));
        state.mute_until = None;
        assert_eq!(state.sound_action(), ("sound-mute", Label::Mute));
    }

    #[test]
    fn all_tray_texts_exist_in_every_frontend_catalog() {
        for (language, json) in [
            (
                LanguagePreference::EnUs,
                include_str!("../../frontend/src/locales/en.json"),
            ),
            (
                LanguagePreference::JaJp,
                include_str!("../../frontend/src/locales/ja.json"),
            ),
            (
                LanguagePreference::KoKr,
                include_str!("../../frontend/src/locales/ko.json"),
            ),
            (
                LanguagePreference::DeDe,
                include_str!("../../frontend/src/locales/de.json"),
            ),
            (
                LanguagePreference::FrFr,
                include_str!("../../frontend/src/locales/fr.json"),
            ),
            (
                LanguagePreference::EsEs,
                include_str!("../../frontend/src/locales/es.json"),
            ),
            (
                LanguagePreference::PtBr,
                include_str!("../../frontend/src/locales/pt.json"),
            ),
        ] {
            let catalog: std::collections::HashMap<String, String> =
                serde_json::from_str(json).unwrap();
            for key in TEXTS {
                assert!(
                    catalog.get(*key).is_some_and(|value| !value.is_empty()),
                    "missing {key}"
                );
                assert_eq!(
                    crate::localization::translate(language, key, "missing translation"),
                    catalog[*key],
                    "{language:?}: {key}"
                );
            }
        }
    }
}
