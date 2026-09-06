use super::DesktopAutomations;
use arcrelay_automation::ApplicationIdentity;
use std::collections::HashMap;

pub fn application_support() -> bool {
    cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    ))
}
pub fn foreground_support() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
        || (cfg!(target_os = "linux")
            && std::env::var_os("DISPLAY").is_some()
            && std::env::var("XDG_SESSION_TYPE").unwrap_or_default() != "wayland")
}
pub fn session_support(kind: &str) -> bool {
    cfg!(target_os = "macos")
        || (cfg!(target_os = "windows") && matches!(kind, "system.locked" | "system.unlocked"))
        || (cfg!(target_os = "linux")
            && std::env::var_os("XDG_SESSION_ID").is_some()
            && std::path::Path::new("/usr/bin/loginctl").is_file()
            && matches!(kind, "system.locked" | "system.unlocked"))
}

#[cfg(target_os = "macos")]
pub fn input_allowed() -> bool {
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}
#[cfg(target_os = "macos")]
pub fn capture_allowed() -> bool {
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
    }
    unsafe { CGPreflightScreenCaptureAccess() }
}
#[cfg(not(target_os = "macos"))]
pub fn input_allowed() -> bool {
    cfg!(windows) || std::env::var_os("DISPLAY").is_some()
}
#[cfg(not(target_os = "macos"))]
pub fn capture_allowed() -> bool {
    input_allowed()
}

#[cfg(target_os = "macos")]
pub fn applications(
    _targets: &[ApplicationIdentity],
) -> (HashMap<String, ApplicationIdentity>, Option<String>) {
    use objc2_app_kit::{NSApplicationActivationPolicy, NSWorkspace};
    let workspace = NSWorkspace::sharedWorkspace();
    let foreground = workspace
        .frontmostApplication()
        .and_then(|a| a.bundleIdentifier())
        .map(|s| s.to_string());
    let mut apps = HashMap::new();
    for app in workspace.runningApplications() {
        if app.activationPolicy() != NSApplicationActivationPolicy::Regular {
            continue;
        }
        let Some(id) = app.bundleIdentifier().map(|s| s.to_string()) else {
            continue;
        };
        let Some(path) = app
            .bundleURL()
            .and_then(|u| u.path())
            .map(|s| s.to_string())
        else {
            continue;
        };
        let name = app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_else(|| id.clone());
        apps.insert(id.clone(), ApplicationIdentity { id, name, path });
    }
    (apps, foreground)
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn applications(
    _targets: &[ApplicationIdentity],
) -> (HashMap<String, ApplicationIdentity>, Option<String>) {
    (HashMap::new(), None)
}

#[cfg(target_os = "windows")]
pub fn applications(
    targets: &[ApplicationIdentity],
) -> (HashMap<String, ApplicationIdentity>, Option<String>) {
    use std::sync::{Mutex, OnceLock};
    use sysinfo::{ProcessesToUpdate, System};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    static PROCESSES: OnceLock<Mutex<System>> = OnceLock::new();
    let mut system = PROCESSES
        .get_or_init(|| Mutex::new(System::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    system.refresh_processes(ProcessesToUpdate::All, true);
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
    }
    let mut apps = HashMap::new();
    let mut foreground = None;
    for (process_id, process) in system.processes() {
        let Some(path) = process.exe() else {
            continue;
        };
        let normalized = normalize_executable(&path.to_string_lossy());
        for target in targets
            .iter()
            .filter(|a| normalize_executable(&a.path) == normalized)
        {
            apps.insert(target.id.clone(), target.clone());
            if process_id.as_u32() == pid {
                foreground = Some(target.id.clone());
            }
        }
    }
    (apps, foreground)
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub fn normalize_executable(path: &str) -> String {
    let path = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.into())
        .to_string_lossy()
        .into_owned();
    if cfg!(windows) {
        path.trim_start_matches("\\\\?\\")
            .replace('\\', "/")
            .to_lowercase()
    } else {
        path
    }
}

#[cfg(target_os = "linux")]
pub fn applications(
    targets: &[ApplicationIdentity],
) -> (HashMap<String, ApplicationIdentity>, Option<String>) {
    let mut apps = HashMap::new();
    let mut foreground = None;
    let foreground_pid = linux_foreground_pid();
    for entry in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(path) = std::fs::read_link(entry.path().join("exe")) else {
            continue;
        };
        let path = path.to_string_lossy();
        for target in targets
            .iter()
            .filter(|a| normalize_executable(&a.path) == path)
        {
            apps.insert(target.id.clone(), target.clone());
            if Some(pid) == foreground_pid {
                foreground = Some(target.id.clone());
            }
        }
    }
    (apps, foreground)
}
#[cfg(target_os = "linux")]
fn linux_foreground_pid() -> Option<u32> {
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ConnectionExt},
    };
    if !foreground_support() {
        return None;
    }
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let active = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let property = conn
        .get_property(false, root, active, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let window = property.value32()?.next()?;
    let pid = conn
        .intern_atom(false, b"_NET_WM_PID")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let property = conn
        .get_property(false, window, pid, AtomEnum::CARDINAL, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    let value = property.value32()?.next();
    value
}

#[cfg(target_os = "linux")]
pub fn installed_linux_apps() -> std::result::Result<Vec<crate::commands::InstalledAppView>, String>
{
    use std::path::{Path, PathBuf};
    let mut roots = vec![];
    if let Some(home) = dirs::data_local_dir() {
        roots.push(home.join("applications"));
    }
    roots.extend(
        std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
            .split(':')
            .map(|p| Path::new(p).join("applications")),
    );
    let mut apps = vec![];
    let mut seen = std::collections::HashSet::new();
    for root in roots {
        for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if source.len() > 65536 {
                continue;
            }
            let mut section = false;
            let mut fields = HashMap::new();
            for line in source.lines() {
                if line.starts_with('[') {
                    section = line == "[Desktop Entry]";
                    continue;
                }
                if section {
                    if let Some((key, value)) = line.split_once('=') {
                        fields.insert(key, value);
                    }
                }
            }
            if fields.get("Type") != Some(&"Application")
                || fields.get("Hidden") == Some(&"true")
                || fields.get("NoDisplay") == Some(&"true")
            {
                continue;
            }
            let Some(name) = fields.get("Name") else {
                continue;
            };
            let Some(exec) = fields.get("Exec") else {
                continue;
            };
            let Ok(words) = shell_words::split(exec) else {
                continue;
            };
            let Some(program) = words.first() else {
                continue;
            };
            if matches!(program.as_str(), "env" | "flatpak" | "snap" | "gtk-launch") {
                continue;
            }
            let executable = if Path::new(program).is_absolute() {
                PathBuf::from(program)
            } else {
                std::env::var_os("PATH")
                    .and_then(|v| {
                        std::env::split_paths(&v)
                            .map(|p| p.join(program))
                            .find(|p| p.is_file())
                    })
                    .unwrap_or_default()
            };
            if !executable.is_file() {
                continue;
            }
            apps.push(crate::commands::InstalledAppView {
                name: (*name).into(),
                path: executable.to_string_lossy().into_owned(),
                identifier: Some(id),
                version: None,
                icon_data_url: None,
            });
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(apps)
}

#[cfg(target_os = "macos")]
pub fn locked() -> bool {
    use core_foundation::{
        base::{CFType, TCFType},
        boolean::CFBoolean,
        dictionary::{CFDictionaryGetValue, CFDictionaryRef},
        string::CFString,
    };
    extern "C" {
        fn CGSessionCopyCurrentDictionary() -> CFDictionaryRef;
    }
    unsafe {
        let dict = CGSessionCopyCurrentDictionary();
        if dict.is_null() {
            return true;
        }
        let owner = CFType::wrap_under_create_rule(dict.cast());
        let key = CFString::new("CGSSessionScreenIsLocked");
        let value = CFDictionaryGetValue(dict, key.as_CFTypeRef());
        let result = !value.is_null() && CFBoolean::wrap_under_get_rule(value.cast()).into();
        drop(owner);
        result
    }
}
#[cfg(target_os = "windows")]
pub fn locked() -> bool {
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_CONTROL_FLAGS,
        DESKTOP_READOBJECTS, UOI_NAME,
    };
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(desktop) => {
                let mut name = [0u16; 256];
                let result = GetUserObjectInformationW(
                    windows::Win32::Foundation::HANDLE(desktop.0),
                    UOI_NAME,
                    Some(name.as_mut_ptr().cast()),
                    (name.len() * 2) as u32,
                    None,
                );
                let _ = CloseDesktop(desktop);
                result.is_err()
                    || String::from_utf16_lossy(
                        &name[..name.iter().position(|v| *v == 0).unwrap_or(name.len())],
                    ) != "Default"
            }
            Err(_) => true,
        }
    }
}
#[cfg(target_os = "linux")]
static LINUX_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
#[cfg(target_os = "linux")]
pub fn locked() -> bool {
    LINUX_LOCKED.load(std::sync::atomic::Ordering::Relaxed)
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn locked() -> bool {
    // Without a supported session provider, fail closed for confirmation and interactive actions.
    true
}

#[cfg(target_os = "macos")]
pub fn start_session_events(owner: DesktopAutomations) {
    use objc2_app_kit::{
        NSWorkspace, NSWorkspaceDidWakeNotification, NSWorkspaceWillSleepNotification,
    };
    use objc2_foundation::NSNotification;
    use std::ptr::NonNull;
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    for (name, kind) in unsafe {
        [
            (NSWorkspaceWillSleepNotification, "system.sleeping"),
            (NSWorkspaceDidWakeNotification, "system.resumed"),
        ]
    } {
        let owner = owner.clone();
        let block = block2::RcBlock::new(move |_: NonNull<NSNotification>| {
            let owner = owner.clone();
            tauri::async_runtime::spawn(async move {
                owner.system_event(kind).await;
            });
        });
        // The notification center retains the process-lifetime observer. Never capture a UI object.
        let _observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block)
        };
    }
    tokio::spawn(async move {
        let mut previous = locked();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let now = locked();
            if now != previous {
                owner
                    .system_event(if now {
                        "system.locked"
                    } else {
                        "system.unlocked"
                    })
                    .await;
                previous = now;
            }
        }
    });
}
#[cfg(target_os = "windows")]
pub fn start_session_events(owner: DesktopAutomations) {
    if !session_support("system.locked") {
        return;
    }
    tokio::spawn(async move {
        let mut previous = locked();
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let now = locked();
            if now != previous {
                owner
                    .system_event(if now {
                        "system.locked"
                    } else {
                        "system.unlocked"
                    })
                    .await;
                previous = now;
            }
        }
    });
}

#[cfg(target_os = "linux")]
pub fn start_session_events(owner: DesktopAutomations) {
    if !session_support("system.locked") {
        return;
    }
    let Some(session) = std::env::var_os("XDG_SESSION_ID") else {
        return;
    };
    tokio::spawn(async move {
        let mut previous = None;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let mut command = tokio::process::Command::new("/usr/bin/loginctl");
            command
                .args(["show-session", "--property=LockedHint", "--value", "--"])
                .arg(&session)
                .kill_on_drop(true);
            let output =
                tokio::time::timeout(std::time::Duration::from_secs(2), command.output()).await;
            let state = match output {
                Ok(Ok(output)) if output.status.success() => {
                    match String::from_utf8_lossy(&output.stdout).trim() {
                        "yes" => Some(true),
                        "no" => Some(false),
                        _ => None,
                    }
                }
                _ => None,
            };
            LINUX_LOCKED.store(state.unwrap_or(true), std::sync::atomic::Ordering::Relaxed);
            if let Some(now) = state {
                if previous.is_some_and(|old| old != now) {
                    owner
                        .system_event(if now {
                            "system.locked"
                        } else {
                            "system.unlocked"
                        })
                        .await;
                }
                previous = Some(now);
            }
        }
    });
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn start_session_events(_: DesktopAutomations) {}
