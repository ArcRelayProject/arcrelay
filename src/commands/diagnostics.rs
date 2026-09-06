use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::arc_input::ArcInputRuntime;
use crate::observability::{self, DiagnosticBundleInfo, LogStatus};

#[derive(Debug, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct FrontendLogEntry {
    pub level: String,
    pub event: String,
    pub detail: String,
    pub stack: Option<String>,
}

#[arcrelay_desktop_ipc::command]
pub fn get_log_status() -> LogStatus {
    observability::status()
}

#[arcrelay_desktop_ipc::command]
pub fn set_detailed_logging(enabled: bool) -> Result<LogStatus, String> {
    observability::set_detailed_logging(enabled)
}

#[arcrelay_desktop_ipc::command]
pub fn open_log_directory() -> Result<(), String> {
    let directory = observability::log_directory();
    std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    reveal_path(&directory, false)?;
    tracing::info!(
        event = "diagnostics.log_directory_opened",
        "log directory opened"
    );
    Ok(())
}

#[arcrelay_desktop_ipc::command]
pub async fn export_diagnostic_bundle(
    runtime: State<'_, Arc<ArcInputRuntime>>,
) -> Result<DiagnosticBundleInfo, String> {
    let snapshot = runtime.snapshot();
    let bundle = tauri::async_runtime::spawn_blocking(move || {
        observability::export_diagnostic_bundle(&snapshot)
    })
    .await
    .map_err(|error| error.to_string())??;
    let _ = reveal_path(Path::new(&bundle.path), true);
    Ok(bundle)
}

#[arcrelay_desktop_ipc::command]
pub fn frontend_log(entry: FrontendLogEntry) {
    let event = sanitize_event_name(&entry.event);
    let detail = observability::sanitize_text(&entry.detail, 4_096);
    let stack = observability::sanitize_text(entry.stack.as_deref().unwrap_or(""), 32_768);
    match entry.level.to_ascii_lowercase().as_str() {
        "error" => tracing::error!(
            event = %event,
            component = "frontend",
            detail = %detail,
            stack = %stack,
            "frontend event"
        ),
        "warn" | "warning" => tracing::warn!(
            event = %event,
            component = "frontend",
            detail = %detail,
            stack = %stack,
            "frontend event"
        ),
        "debug" => tracing::debug!(
            event = %event,
            component = "frontend",
            detail = %detail,
            "frontend event"
        ),
        _ => tracing::info!(
            event = %event,
            component = "frontend",
            detail = %detail,
            "frontend event"
        ),
    }
}

fn sanitize_event_name(value: &str) -> String {
    let event = value
        .to_ascii_lowercase()
        .chars()
        .filter(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
        .take(96)
        .collect::<String>();
    if event.is_empty() {
        "frontend.event".into()
    } else {
        event
    }
}

fn reveal_path(path: &Path, select_file: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/open");
        if select_file {
            command.arg("-R");
        }
        command.arg(path);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("explorer.exe");
        if select_file {
            command.arg(format!("/select,{}", path.display()));
        } else {
            command.arg(path);
        }
        command
    };

    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(if select_file {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        command
    };

    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_event_names_are_bounded_and_machine_readable() {
        assert_eq!(
            sanitize_event_name("Input.Handoff Failed!"),
            "input.handofffailed"
        );
        assert_eq!(
            sanitize_event_name("input.handoff.failed"),
            "input.handoff.failed"
        );
        assert_eq!(sanitize_event_name("!!!"), "frontend.event");
    }
}

#[arcrelay_desktop_ipc::command]
pub async fn get_runtime_modules(
    state: State<'_, crate::backend::DesktopState>,
) -> Result<Vec<crate::application::runtime_modules::ModuleStatus>, String> {
    Ok(state.modules.statuses())
}

#[cfg(test)]
include!(concat!(env!("OUT_DIR"), "/src_commands_diagnostics_ipc.rs"));
