use crate::application::output_manager::OutputManager;
use crate::domain::quick_action::{ActionType, MediaOperation, QuickAction, SystemOperation};
use crate::screenshot::{ScreenshotService, StartOutcome};
use arcrelay_core::application::service::ArcRelayService;
use arcrelay_core::domain::input_control::InputEvent;
use arcrelay_core::domain::media_control::PlaybackAction;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tracing::info;

const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const READ_CHUNK_BYTES: usize = 8 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

/// Executes a quick action on the local system.
pub fn execute_action(
    action: &QuickAction,
    output: Option<Arc<OutputManager>>,
    runtime: &Arc<ArcRelayService>,
    screenshot: &Arc<ScreenshotService>,
) -> Result<String, String> {
    info!(
        action_id = %action.id,
        action_type = action.action_type_label(),
        "executing action"
    );
    match &action.action_type {
        ActionType::LaunchApp { app_name, app_path } => {
            launch_app(action, app_name, app_path.as_deref(), output)
        }
        ActionType::OpenPath { path } => open_path(action, path, output),
        ActionType::OpenUrl { url } => open_url(action, url, output),
        ActionType::ShellCommand {
            command,
            working_dir,
        } => run_shell(action, command, working_dir.as_deref(), output),
        ActionType::Hotkey { modifiers, key } => {
            simulate_hotkey(runtime, modifiers, key).map(|_| "快捷键已发送".to_string())
        }
        ActionType::AppleScript { script } => run_applescript(action, script, output),
        ActionType::System { operation } => execute_system(action, *operation, output, screenshot),
        ActionType::Media { operation } => execute_media(runtime, *operation),
        ActionType::SetSystemVolume { volume } => {
            let volume = (*volume).min(100);
            run_core(runtime.media_control.set_system_volume(volume))?;
            Ok(format!("系统音量已设置为 {volume}%"))
        }
        ActionType::SetSystemMuted { muted } => {
            run_core(runtime.media_control.set_system_muted(*muted))?;
            Ok(if *muted {
                "系统已静音".to_string()
            } else {
                "系统已取消静音".to_string()
            })
        }
        ActionType::SetMicrophone { active } => {
            run_core(runtime.media_control.set_microphone_active(*active))?;
            Ok(if *active {
                "麦克风已开启".to_string()
            } else {
                "麦克风已静音".to_string()
            })
        }
        ActionType::ToggleShellCommand { .. } => {
            Err("Toggle actions must be executed via ActionService".to_string())
        }
    }
}

fn launch_app(
    action: &QuickAction,
    app_name: &str,
    app_path: Option<&str>,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    let discovered_path = if app_path.is_none() && !app_name.trim().is_empty() {
        app_info::find_app_by_name(app_name.trim(), 0)
            .ok()
            .map(|app| app.path)
    } else {
        None
    };
    let path = app_path
        .filter(|path| !path.trim().is_empty())
        .map(expand_path)
        .transpose()?
        .or(discovered_path);

    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        if let Some(path) = &path {
            command.arg(path);
        } else {
            command.args(["-a", app_name.trim()]);
        }
        command
    };

    #[cfg(target_os = "windows")]
    let command = {
        if let Some(path) = &path {
            if path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            {
                Command::new(path)
            } else {
                let mut command = Command::new("explorer.exe");
                command.arg(path);
                command
            }
        } else {
            Command::new(app_name.trim())
        }
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        if let Some(path) = &path {
            Command::new(path)
        } else {
            Command::new(app_name.trim())
        }
    };

    let display_name = if app_name.trim().is_empty() {
        path.as_deref()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "application".to_string())
    } else {
        app_name.trim().to_string()
    };
    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("failed to launch {display_name}: {error}"))?;
    if status.success() {
        Ok(format!("已启动 {display_name}"))
    } else {
        Err(format!("failed to launch {display_name}: {stderr}"))
    }
}

fn open_path(
    action: &QuickAction,
    path: &str,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    let path = expand_path(path)?;
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }

    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg(&path);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("explorer.exe");
        command.arg(&path);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(&path);
        command
    };

    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("failed to open path: {error}"))?;
    if status.success() {
        Ok(format!("已打开 {}", path.display()))
    } else {
        Err(format!("failed to open path: {stderr}"))
    }
}

fn open_url(
    action: &QuickAction,
    url: &str,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    validate_external_url(url)?;

    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };

    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("failed to open URL: {error}"))?;
    if status.success() {
        Ok(format!("已打开 {url}"))
    } else {
        Err(format!("failed to open URL: {stderr}"))
    }
}

fn run_shell(
    action: &QuickAction,
    command: &str,
    working_dir: Option<&str>,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    #[cfg(windows)]
    let mut cmd = {
        let mut command_process = Command::new("cmd");
        command_process.args(["/C", command]);
        command_process
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut command_process = Command::new("sh");
        command_process.args(["-c", command]);
        command_process
    };
    if let Some(dir) = working_dir {
        cmd.current_dir(expand_path(dir)?);
    }
    let (status, stdout, stderr) =
        run_bounded(cmd, action, output).map_err(|error| format!("Shell error: {error}"))?;
    if status.success() {
        Ok(if stdout.is_empty() {
            "OK".to_string()
        } else {
            stdout
        })
    } else {
        Err(format!("Exit {status}: {stderr}"))
    }
}

fn simulate_hotkey(
    runtime: &Arc<ArcRelayService>,
    modifiers: &[String],
    key: &str,
) -> Result<(), String> {
    let mut events = Vec::with_capacity(modifiers.len() * 2 + 2);
    let modifier_usages: Vec<u16> = modifiers
        .iter()
        .map(|modifier| modifier_to_hid(modifier))
        .collect::<Result<_, _>>()?;
    let key_usage = key_to_hid(key).ok_or_else(|| format!("unsupported shortcut key: {key}"))?;

    for usage in &modifier_usages {
        events.push(InputEvent::Key {
            hid_usage: *usage,
            down: true,
            repeat: false,
        });
    }
    events.push(InputEvent::Key {
        hid_usage: key_usage,
        down: true,
        repeat: false,
    });
    events.push(InputEvent::Key {
        hid_usage: key_usage,
        down: false,
        repeat: false,
    });
    for usage in modifier_usages.iter().rev() {
        events.push(InputEvent::Key {
            hid_usage: *usage,
            down: false,
            repeat: false,
        });
    }
    runtime
        .input_control
        .validate_events(&events)
        .map_err(|error| error.to_string())?;
    run_core(runtime.input_control.apply_events(&events))
}

fn modifier_to_hid(modifier: &str) -> Result<u16, String> {
    match modifier.trim().to_ascii_lowercase().as_str() {
        "control" | "ctrl" => Ok(0xE0),
        "shift" => Ok(0xE1),
        "option" | "alt" => Ok(0xE2),
        "command" | "cmd" | "meta" | "super" | "win" | "windows" => Ok(0xE3),
        value => Err(format!("unsupported modifier key: {value}")),
    }
}

fn key_to_hid(key: &str) -> Option<u16> {
    let key = key.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_lowercase() {
            return Some(0x04 + u16::from(byte - b'a'));
        }
        if (b'1'..=b'9').contains(&byte) {
            return Some(0x1E + u16::from(byte - b'1'));
        }
        if byte == b'0' {
            return Some(0x27);
        }
    }
    Some(match key.as_str() {
        "enter" | "return" => 0x28,
        "escape" | "esc" => 0x29,
        "backspace" => 0x2A,
        "tab" => 0x2B,
        "space" => 0x2C,
        "minus" => 0x2D,
        "equal" => 0x2E,
        "left_bracket" => 0x2F,
        "right_bracket" => 0x30,
        "backslash" => 0x31,
        "semicolon" => 0x33,
        "quote" => 0x34,
        "grave" | "backtick" => 0x35,
        "comma" => 0x36,
        "period" | "dot" => 0x37,
        "slash" => 0x38,
        "caps_lock" => 0x39,
        "f1" => 0x3A,
        "f2" => 0x3B,
        "f3" => 0x3C,
        "f4" => 0x3D,
        "f5" => 0x3E,
        "f6" => 0x3F,
        "f7" => 0x40,
        "f8" => 0x41,
        "f9" => 0x42,
        "f10" => 0x43,
        "f11" => 0x44,
        "f12" => 0x45,
        "insert" => 0x49,
        "home" => 0x4A,
        "page_up" | "pageup" => 0x4B,
        "delete" => 0x4C,
        "end" => 0x4D,
        "page_down" | "pagedown" => 0x4E,
        "arrow_right" | "right" => 0x4F,
        "arrow_left" | "left" => 0x50,
        "arrow_down" | "down" => 0x51,
        "arrow_up" | "up" => 0x52,
        _ => return None,
    })
}

fn run_applescript(
    action: &QuickAction,
    script: &str,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("osascript");
        command.args(["-e", script]);
        let (status, stdout, stderr) = run_bounded(command, action, output)
            .map_err(|error| format!("AppleScript error: {error}"))?;
        if status.success() {
            let stdout = stdout.trim().to_string();
            Ok(if stdout.is_empty() {
                "OK".to_string()
            } else {
                stdout
            })
        } else {
            Err(format!("AppleScript failed: {stderr}"))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (action, script, output);
        Err("AppleScript is only supported on macOS".to_string())
    }
}

fn execute_media(
    runtime: &Arc<ArcRelayService>,
    operation: MediaOperation,
) -> Result<String, String> {
    let playback_action = match operation {
        MediaOperation::TogglePlayPause => {
            let is_playing = run_core(runtime.media_control.playback_info())?
                .is_some_and(|info| info.is_playing);
            if is_playing {
                PlaybackAction::Pause
            } else {
                PlaybackAction::Play
            }
        }
        MediaOperation::Play => PlaybackAction::Play,
        MediaOperation::Pause => PlaybackAction::Pause,
        MediaOperation::Next => PlaybackAction::Next,
        MediaOperation::Previous => PlaybackAction::Previous,
        MediaOperation::SeekForward => PlaybackAction::SeekForward,
        MediaOperation::SeekBackward => PlaybackAction::SeekBackward,
    };
    run_core(runtime.media_control.playback_action(playback_action))?;
    Ok("媒体控制已执行".to_string())
}

fn execute_system(
    action: &QuickAction,
    operation: SystemOperation,
    output: Option<Arc<OutputManager>>,
    screenshot: &Arc<ScreenshotService>,
) -> Result<String, String> {
    match operation {
        SystemOperation::ScreenshotFull => capture_fullscreen(action, output, screenshot),
        SystemOperation::ScreenshotRegion => capture_region(action, output, screenshot),
        _ => execute_power_operation(action, operation, output),
    }
}

fn execute_power_operation(
    action: &QuickAction,
    operation: SystemOperation,
    output: Option<Arc<OutputManager>>,
) -> Result<String, String> {
    #[cfg(target_os = "macos")]
    let command = match operation {
        SystemOperation::LockScreen => {
            let mut command = Command::new(
                "/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession",
            );
            command.arg("-suspend");
            command
        }
        SystemOperation::Sleep => {
            let mut command = Command::new("pmset");
            command.arg("sleepnow");
            command
        }
        SystemOperation::DisplaySleep => {
            let mut command = Command::new("pmset");
            command.arg("displaysleepnow");
            command
        }
        SystemOperation::Shutdown => {
            let mut command = Command::new("osascript");
            command.args(["-e", "tell application \"System Events\" to shut down"]);
            command
        }
        SystemOperation::Restart => {
            let mut command = Command::new("osascript");
            command.args(["-e", "tell application \"System Events\" to restart"]);
            command
        }
        _ => return Err("Unsupported system operation".to_string()),
    };

    #[cfg(target_os = "windows")]
    let command = match operation {
        SystemOperation::LockScreen => {
            let mut command = Command::new("rundll32.exe");
            command.arg("user32.dll,LockWorkStation");
            command
        }
        SystemOperation::Sleep => {
            let mut command = Command::new("rundll32.exe");
            command.args(["powrprof.dll,SetSuspendState", "0,1,0"]);
            command
        }
        SystemOperation::DisplaySleep => {
            let script = concat!(
                "$sig='[DllImport(\"user32.dll\")] public static extern IntPtr ",
                "SendMessage(IntPtr hWnd,uint Msg,IntPtr wParam,IntPtr lParam);';",
                "$native=Add-Type -MemberDefinition $sig -Name Native -Namespace ArcRelay -PassThru;",
                "$native::SendMessage([IntPtr]0xffff,0x0112,[IntPtr]0xF170,[IntPtr]2) | Out-Null"
            );
            let mut command = Command::new("powershell.exe");
            command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            command
        }
        SystemOperation::Shutdown => {
            let mut command = Command::new("shutdown.exe");
            command.args(["/s", "/t", "0"]);
            command
        }
        SystemOperation::Restart => {
            let mut command = Command::new("shutdown.exe");
            command.args(["/r", "/t", "0"]);
            command
        }
        _ => return Err("Unsupported system operation".to_string()),
    };

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = match operation {
        SystemOperation::LockScreen => {
            let mut command = Command::new("loginctl");
            command.arg("lock-session");
            command
        }
        SystemOperation::Sleep => {
            let mut command = Command::new("systemctl");
            command.arg("suspend");
            command
        }
        SystemOperation::DisplaySleep => {
            let mut command = Command::new("xset");
            command.args(["dpms", "force", "off"]);
            command
        }
        SystemOperation::Shutdown => {
            let mut command = Command::new("systemctl");
            command.arg("poweroff");
            command
        }
        SystemOperation::Restart => {
            let mut command = Command::new("systemctl");
            command.arg("reboot");
            command
        }
        _ => return Err("Unsupported system operation".to_string()),
    };

    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("system operation failed: {error}"))?;
    if status.success() {
        Ok("系统操作已执行".to_string())
    } else {
        Err(format!("system operation failed: {stderr}"))
    }
}

fn capture_fullscreen(
    action: &QuickAction,
    output: Option<Arc<OutputManager>>,
    screenshot: &Arc<ScreenshotService>,
) -> Result<String, String> {
    let path = screenshot_output_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    if screenshot.is_enabled() {
        match screenshot.capture_fullscreen(&path) {
            Ok(()) => return Ok(format!("截图已保存到 {}", path.display())),
            Err(error) if error == "SNIPTRA_ALREADY_RUNNING" => {
                return Ok("Sniptra 截图界面已经打开".to_string());
            }
            Err(error) => {
                tracing::warn!(%error, "Sniptra full screenshot failed; falling back to system capture");
            }
        }
    }

    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("screencapture");
        command.args(["-x", path.to_string_lossy().as_ref()]);
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let escaped = path.to_string_lossy().replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
             $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; \
             $i=New-Object System.Drawing.Bitmap $b.Width,$b.Height; \
             $g=[System.Drawing.Graphics]::FromImage($i); \
             $g.CopyFromScreen($b.Left,$b.Top,0,0,$b.Size); \
             $i.Save('{escaped}',[System.Drawing.Imaging.ImageFormat]::Png); \
             $g.Dispose(); $i.Dispose();"
        );
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("gnome-screenshot");
        command.args(["-f", path.to_string_lossy().as_ref()]);
        command
    };

    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("screenshot failed: {error}"))?;
    if status.success() {
        Ok(format!("截图已保存到 {}", path.display()))
    } else {
        Err(format!("screenshot failed: {stderr}"))
    }
}

fn capture_region(
    action: &QuickAction,
    output: Option<Arc<OutputManager>>,
    screenshot: &Arc<ScreenshotService>,
) -> Result<String, String> {
    if screenshot.is_enabled() {
        match screenshot.capture_region() {
            Ok(StartOutcome::Started(pid)) => {
                return Ok(format!("Sniptra 区域截图已打开（pid {pid}）"));
            }
            Ok(StartOutcome::AlreadyRunning) => {
                return Ok("Sniptra 截图界面已经打开".to_string());
            }
            Err(error) => {
                tracing::warn!(%error, "Sniptra region screenshot failed; falling back to system capture");
            }
        }
    }

    #[cfg(target_os = "macos")]
    let command = {
        let mut command = Command::new("screencapture");
        command.arg("-i");
        command
    };
    #[cfg(target_os = "windows")]
    let command = {
        let mut command = Command::new("explorer.exe");
        command.arg("ms-screenclip:");
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let command = {
        let mut command = Command::new("gnome-screenshot");
        command.arg("-a");
        command
    };
    let (status, _, stderr) = run_bounded(command, action, output)
        .map_err(|error| format!("region screenshot failed: {error}"))?;
    if status.success() {
        Ok("区域截图工具已打开".to_string())
    } else {
        Err(format!("region screenshot failed: {stderr}"))
    }
}

fn screenshot_output_path() -> PathBuf {
    let base = dirs::picture_dir()
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(std::env::temp_dir);
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    base.join(format!("ArcRelay-{seconds}.png"))
}

pub(crate) fn validate_external_url(url: &str) -> Result<(), String> {
    if url.chars().any(char::is_control) {
        return Err("URL contains invalid control characters".to_string());
    }
    let (scheme, _) = url
        .split_once(':')
        .ok_or_else(|| format!("URL is missing a scheme: {url}"))?;
    if scheme.is_empty()
        || !scheme.chars().enumerate().all(|(index, value)| {
            if index == 0 {
                value.is_ascii_alphabetic()
            } else {
                value.is_ascii_alphanumeric() || matches!(value, '+' | '-' | '.')
            }
        })
    {
        return Err(format!("invalid URL scheme: {scheme}"));
    }
    Ok(())
}

pub(crate) fn expand_path(path: &str) -> Result<PathBuf, String> {
    let expanded = shellexpand::full(path.trim())
        .map_err(|error| format!("failed to expand path variables: {error}"))?;
    Ok(PathBuf::from(expanded.as_ref()))
}

fn run_core<F, T>(future: F) -> Result<T, String>
where
    F: Future<Output = arcrelay_core::error::Result<T>>,
{
    action_runtime()
        .block_on(future)
        .map_err(|error| error.to_string())
}

fn action_runtime() -> &'static tokio::runtime::Runtime {
    static ACTION_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    ACTION_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("arcrelay-action")
            .build()
            .expect("failed to create ArcRelay action runtime")
    })
}

fn run_bounded(
    mut command: Command,
    action: &QuickAction,
    output: Option<Arc<OutputManager>>,
) -> Result<(std::process::ExitStatus, String, String), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Quick actions run in the background and stream their output into
        // ArcRelay's UI, so a separate console window is never needed.
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    action_runtime().block_on(run_bounded_async(command, action.id.clone(), output))
}

async fn run_bounded_async(
    command: Command,
    action_id: String,
    output: Option<Arc<OutputManager>>,
) -> Result<(std::process::ExitStatus, String, String), String> {
    let mut command = tokio::process::Command::from(command);
    command.kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "stdout unavailable".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "stderr unavailable".to_string())?;
    let stdout_id = action_id.clone();
    let stderr_id = action_id;
    let stdout_output = output.clone();
    let execution = async {
        let (status, stdout, stderr) = tokio::try_join!(
            async { child.wait().await.map_err(|error| error.to_string()) },
            read_bounded(stdout, stdout_output, stdout_id, ""),
            read_bounded(stderr, output, stderr_id, "[stderr] "),
        )?;
        Ok::<_, String>((
            status,
            String::from_utf8_lossy(&stdout).into_owned(),
            String::from_utf8_lossy(&stderr).into_owned(),
        ))
    };
    match tokio::time::timeout(COMMAND_TIMEOUT, execution).await {
        Ok(result) => result,
        Err(_) => {
            terminate_child(&mut child).await;
            Err(format!(
                "command exceeded {} seconds and was terminated",
                COMMAND_TIMEOUT.as_secs()
            ))
        }
    }
}

async fn terminate_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id().map(|value| value as libc::pid_t) {
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

pub(crate) fn output_with_timeout(
    command: Command,
    timeout: Duration,
) -> std::io::Result<std::process::Output> {
    arcrelay_core::infrastructure::bounded_command::output(command, timeout)
}

async fn read_bounded<R: AsyncRead + Unpin>(
    mut reader: R,
    output: Option<Arc<OutputManager>>,
    action_id: String,
    prefix: &'static str,
) -> Result<Vec<u8>, String> {
    let mut captured = Vec::with_capacity(READ_CHUNK_BYTES);
    let mut pending = Vec::with_capacity(READ_CHUNK_BYTES);
    let mut chunk = [0u8; READ_CHUNK_BYTES];
    loop {
        let read = reader
            .read(&mut chunk)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        if captured.len() < MAX_CAPTURE_BYTES {
            let remaining = MAX_CAPTURE_BYTES - captured.len();
            captured.extend_from_slice(&chunk[..read.min(remaining)]);
        }
        if let Some(manager) = output.as_ref() {
            pending.extend_from_slice(&chunk[..read]);
            while let Some(position) = pending.iter().position(|byte| *byte == b'\n') {
                let line = pending.drain(..=position).collect::<Vec<_>>();
                manager.append(
                    &action_id,
                    format!("{prefix}{}", String::from_utf8_lossy(&line).trim_end()),
                );
            }
            if pending.len() >= 16 * 1024 {
                manager.append(
                    &action_id,
                    format!("{prefix}{}", String::from_utf8_lossy(&pending)),
                );
                pending.clear();
            }
        }
    }
    if let Some(manager) = output {
        if !pending.is_empty() {
            manager.append(
                &action_id,
                format!("{prefix}{}", String::from_utf8_lossy(&pending)),
            );
        }
    }
    Ok(captured)
}

#[cfg(test)]
mod tests {
    use super::{key_to_hid, run_bounded, validate_external_url};
    use crate::domain::quick_action::{ActionType, QuickAction};
    use std::process::Command;

    #[test]
    fn maps_named_and_character_keys() {
        assert_eq!(key_to_hid("A"), Some(0x04));
        assert_eq!(key_to_hid("arrow-left"), Some(0x50));
        assert_eq!(key_to_hid("F12"), Some(0x45));
        assert_eq!(key_to_hid("unsupported"), None);
    }

    #[test]
    fn accepts_deep_links_without_shell_execution() {
        assert!(validate_external_url("https://example.com").is_ok());
        assert!(validate_external_url("obsidian://open?vault=Work").is_ok());
        assert!(validate_external_url("missing-scheme").is_err());
    }

    #[test]
    fn bounded_commands_capture_both_output_streams() {
        let action = QuickAction::new(
            "test".into(),
            "test".into(),
            ActionType::ShellCommand {
                command: String::new(),
                working_dir: None,
            },
        );
        #[cfg(windows)]
        let command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "echo stdout & echo stderr 1>&2"]);
            command
        };
        #[cfg(not(windows))]
        let command = {
            let mut command = Command::new("sh");
            command.args(["-c", "printf stdout; printf stderr >&2"]);
            command
        };

        let (status, stdout, stderr) = run_bounded(command, &action, None).unwrap();
        assert!(status.success());
        assert!(stdout.contains("stdout"));
        assert!(stderr.contains("stderr"));
    }
}
