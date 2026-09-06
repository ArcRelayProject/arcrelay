use crate::application::output_manager::OutputManager;
use crate::application::preset_catalog::{self, ActionPresetView};
use crate::domain::action_icon;
use crate::domain::quick_action::{ActionType, QuickAction, SystemOperation};
use crate::infrastructure::executor;
use crate::infrastructure::persistence::JsonStore;
use crate::screenshot::ScreenshotService;
use arcrelay_core::application::service::ArcRelayService;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Application service for managing quick actions (CRUD + execution).
pub struct ActionService {
    store: JsonStore,
    actions: Vec<QuickAction>,
    /// Running toggle processes, keyed by action ID.
    toggle_processes: HashMap<String, Child>,
    /// Shared output manager for streaming process output.
    output: Arc<OutputManager>,
    /// Shared cross-platform system capabilities used by native actions.
    runtime: Arc<ArcRelayService>,
    screenshot: Arc<ScreenshotService>,
}

const MAX_IMPORT_TEXT_BYTES: usize = 2 * 1024 * 1024;
const MAX_IMPORTED_ACTIONS: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionTextBundle {
    #[serde(default = "bundle_format")]
    pub format: String,
    #[serde(default = "bundle_version")]
    pub version: u32,
    pub actions: Vec<QuickAction>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActionImportResult {
    pub imported_count: usize,
    pub names: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ActionImportPayload {
    Bundle(ActionTextBundle),
    List(Vec<QuickAction>),
    Single(Box<QuickAction>),
}

fn bundle_format() -> String {
    "arcrelay-actions".to_string()
}

const fn bundle_version() -> u32 {
    1
}

/// Resolve the immutable action definition under the catalog lock, then run
/// ordinary actions after releasing it. Toggle bookkeeping still uses the
/// lock briefly because it mutates the child-process registry.
pub fn execute_by_id(
    service: &Arc<Mutex<ActionService>>,
    action_id: &str,
) -> Result<String, String> {
    let action = service
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .find_cloned(action_id)
        .ok_or_else(|| format!("Action not found: {action_id}"))?;
    execute_definition(service, action)
}

/// Execute the definition that was shown in a native confirmation dialog.
pub fn execute_definition(
    service: &Arc<Mutex<ActionService>>,
    action: QuickAction,
) -> Result<String, String> {
    use crate::sound::{play, SoundEvent};
    play(SoundEvent::ActionStarted);
    let result = execute_resolved_definition(service, action, true);
    play(if result.is_ok() {
        SoundEvent::ActionSucceeded
    } else {
        SoundEvent::ActionFailed
    });
    result
}

/// Only the automation runner calls this with a persisted, confirmed run snapshot.
/// Catalog edits affect future runs, not steps in a run that already started.
pub fn execute_automation_snapshot(
    service: &Arc<Mutex<ActionService>>,
    action: QuickAction,
) -> Result<String, String> {
    execute_resolved_definition(service, action, false)
}

fn execute_resolved_definition(
    service: &Arc<Mutex<ActionService>>,
    action: QuickAction,
    verify_catalog: bool,
) -> Result<String, String> {
    let action_id = action.id.as_str();
    let (output, runtime, screenshot) = {
        let service = service.lock().unwrap_or_else(|error| error.into_inner());
        if verify_catalog && service.find(action_id) != Some(&action) {
            return Err("action was modified or deleted; run it again".to_string());
        }
        (
            service.output_manager(),
            service.runtime(),
            service.screenshot_service(),
        )
    };
    if action.is_toggle() {
        let ActionType::ToggleShellCommand {
            ref start_command,
            ref stop_command,
            ref working_dir,
        } = action.action_type
        else {
            unreachable!();
        };
        let running_child = {
            let mut service = service.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(child) = service.toggle_processes.remove(action_id) {
                Some(child)
            } else {
                return service.start_toggle(action_id, start_command, working_dir.as_deref());
            }
        };

        let stop_result = stop_command.as_ref().map(|command| {
            let mut stop_action = action.clone();
            stop_action.action_type = ActionType::ShellCommand {
                command: command.clone(),
                working_dir: working_dir.clone(),
            };
            executor::execute_action(&stop_action, Some(output), &runtime, &screenshot)
        });
        let mut child = running_child.expect("running child was removed above");
        let pid = child.id();
        kill_process_group(&child);
        let _ = child.wait();
        info!(%action_id, pid, "toggle stopped");
        match stop_result {
            Some(Err(error)) => Ok(format!("Stopped (stop command: {error})")),
            _ => Ok(format!("Stopped (pid {pid})")),
        }
    } else {
        executor::execute_action(&action, Some(output), &runtime, &screenshot)
    }
}

impl ActionService {
    pub fn new(
        store: JsonStore,
        output: Arc<OutputManager>,
        runtime: Arc<ArcRelayService>,
        screenshot: Arc<ScreenshotService>,
    ) -> Result<Self, String> {
        let mut actions = if store.actions_exist() {
            store.load_actions()?
        } else {
            let actions = preset_catalog::default_actions();
            store.save_actions(&actions)?;
            actions
        };
        actions.retain_mut(|action| match action_icon::normalize_action_icon(action) {
            Ok(_) => true,
            Err(error) => {
                tracing::warn!(
                    action_id = %action.id,
                    %error,
                    "ignoring action with invalid SVG icon"
                );
                false
            }
        });
        Ok(Self {
            store,
            actions,
            toggle_processes: HashMap::new(),
            output,
            runtime,
            screenshot,
        })
    }

    pub fn actions(&self) -> &[QuickAction] {
        &self.actions
    }

    /// Check whether a toggle action is currently running.
    pub fn is_toggle_running(&self, action_id: &str) -> bool {
        self.toggle_processes.contains_key(action_id)
    }

    pub fn add(&mut self, mut action: QuickAction) -> Result<(), String> {
        if self.find(&action.id).is_some() || action.revision != 0 {
            return Err(
                "action.conflict: the action already exists or has a stale revision".into(),
            );
        }
        action.revision = 1;
        let mut next = self.actions.clone();
        next.push(action);
        self.replace_actions(next)
    }

    pub fn preset_views(&self) -> Vec<ActionPresetView> {
        preset_catalog::catalog()
            .into_iter()
            .map(|preset| {
                let installed = self
                    .actions
                    .iter()
                    .any(|action| action.source_preset_id.as_deref() == Some(preset.id));
                preset.view(installed)
            })
            .collect()
    }

    pub fn install_preset(&mut self, preset_id: &str) -> Result<QuickAction, String> {
        if let Some(action) = self
            .actions
            .iter()
            .find(|action| action.source_preset_id.as_deref() == Some(preset_id))
        {
            return Ok(action.clone());
        }
        let preset = preset_catalog::find(preset_id)
            .ok_or_else(|| format!("Preset not found: {preset_id}"))?;
        let action = preset.build_action(self.actions.len() as u32);
        self.add(action.clone())?;
        self.find_cloned(&action.id)
            .ok_or_else(|| "saved preset not found".into())
    }

    pub fn export_text(&self) -> Result<String, String> {
        serde_json::to_string_pretty(&ActionTextBundle {
            format: bundle_format(),
            version: bundle_version(),
            actions: self.actions.clone(),
        })
        .map_err(|error| format!("Failed to export actions: {error}"))
    }

    pub fn import_text(&mut self, text: &str) -> Result<ActionImportResult, String> {
        if text.len() > MAX_IMPORT_TEXT_BYTES {
            return Err("import text cannot exceed 2 MiB".to_string());
        }
        let payload: ActionImportPayload = serde_json::from_str(text.trim())
            .map_err(|error| format!("action text is not valid JSON: {error}"))?;
        let mut actions = match payload {
            ActionImportPayload::Bundle(bundle) => {
                if !bundle.format.is_empty() && bundle.format != bundle_format() {
                    return Err(format!("unsupported action text format: {}", bundle.format));
                }
                if bundle.version > bundle_version() {
                    return Err(format!(
                        "action text version {} is newer than the supported version",
                        bundle.version
                    ));
                }
                bundle.actions
            }
            ActionImportPayload::List(actions) => actions,
            ActionImportPayload::Single(action) => vec![*action],
        };
        if actions.is_empty() {
            return Err("action text contains no importable actions".to_string());
        }
        if actions.len() > MAX_IMPORTED_ACTIONS {
            return Err(format!(
                "at most {MAX_IMPORTED_ACTIONS} actions can be imported at once"
            ));
        }

        let base_order = self.actions.len() as u32;
        for (index, action) in actions.iter_mut().enumerate() {
            reset_import_identity(action);
            action.sort_order = base_order.saturating_add(index as u32);
            normalize_action(action)?;
        }
        let names = actions.iter().map(|action| action.name.clone()).collect();
        let imported_count = actions.len();
        let mut next = self.actions.clone();
        next.extend(actions);
        self.replace_actions(next)?;
        Ok(ActionImportResult {
            imported_count,
            names,
        })
    }

    pub fn update(&mut self, mut action: QuickAction) -> Result<(), String> {
        let mut next = self.actions.clone();
        let position = next
            .iter()
            .position(|a| a.id == action.id)
            .ok_or("action does not exist")?;
        if next[position].revision != action.revision {
            return Err("action.conflict: the action was modified; reopen it before saving".into());
        }
        action.revision = action
            .revision
            .checked_add(1)
            .ok_or("action revision overflow")?;
        next[position] = action;
        self.replace_actions(next)
    }

    pub fn remove(&mut self, action_id: &str) -> Result<(), String> {
        let next = self
            .actions
            .iter()
            .filter(|a| a.id != action_id)
            .cloned()
            .collect();
        self.replace_actions(next)?;
        self.stop_toggle(action_id);
        Ok(())
    }

    pub fn find(&self, action_id: &str) -> Option<&QuickAction> {
        self.actions.iter().find(|a| a.id == action_id)
    }

    pub fn find_cloned(&self, action_id: &str) -> Option<QuickAction> {
        self.find(action_id).cloned()
    }

    pub fn output_manager(&self) -> Arc<OutputManager> {
        Arc::clone(&self.output)
    }

    pub fn runtime(&self) -> Arc<ArcRelayService> {
        Arc::clone(&self.runtime)
    }

    pub fn screenshot_service(&self) -> Arc<ScreenshotService> {
        Arc::clone(&self.screenshot)
    }

    fn start_toggle(
        &mut self,
        action_id: &str,
        command: &str,
        working_dir: Option<&str>,
    ) -> Result<String, String> {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", command]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            use std::os::windows::process::CommandExt;
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", command]);
            // CREATE_NEW_PROCESS_GROUP keeps tree termination working, while
            // CREATE_NO_WINDOW prevents a background toggle from flashing a
            // console window.
            c.creation_flags(0x0800_0200);
            c
        };
        if let Some(dir) = working_dir {
            cmd.current_dir(executor::expand_path(dir)?);
        }
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        // Make the child a new process group leader so we can kill the whole tree
        #[cfg(unix)]
        unsafe {
            cmd.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                info!(%action_id, pid, "toggle started");

                // Spawn reader threads to capture stdout/stderr into OutputManager
                let out_mgr = Arc::clone(&self.output);
                let aid = action_id.to_string();
                if let Some(stdout) = child.stdout.take() {
                    let mgr = Arc::clone(&out_mgr);
                    let id = aid.clone();
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stdout);
                        for line in reader.lines() {
                            match line {
                                Ok(l) => mgr.append(&id, l),
                                Err(_) => break,
                            }
                        }
                    });
                }
                if let Some(stderr) = child.stderr.take() {
                    let mgr = Arc::clone(&out_mgr);
                    let id = aid.clone();
                    std::thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            match line {
                                Ok(l) => mgr.append(&id, format!("[stderr] {l}")),
                                Err(_) => break,
                            }
                        }
                    });
                }

                self.toggle_processes.insert(action_id.to_string(), child);
                Ok(format!("Started (pid {pid})"))
            }
            Err(e) => Err(format!("Failed to start: {e}")),
        }
    }

    fn stop_toggle(&mut self, action_id: &str) {
        if let Some(mut child) = self.toggle_processes.remove(action_id) {
            kill_process_group(&child);
            let _ = child.wait();
        }
    }

    /// Reap finished toggle processes (call periodically or before listing).
    pub fn reap_finished_toggles(&mut self) {
        let finished: Vec<String> = self
            .toggle_processes
            .iter_mut()
            .filter_map(|(id, child)| {
                match child.try_wait() {
                    Ok(Some(_)) => Some(id.clone()), // process exited
                    _ => None,
                }
            })
            .collect();
        for id in finished {
            info!(action_id = %id, "toggle process exited on its own");
            self.toggle_processes.remove(&id);
        }
    }

    fn replace_actions(&mut self, next: Vec<QuickAction>) -> Result<(), String> {
        self.store.save_actions(&next)?;
        self.actions = next;
        Ok(())
    }
}

pub fn normalize_action(action: &mut QuickAction) -> Result<(), String> {
    action.global_shortcut = crate::action_shortcuts::normalize(action.global_shortcut.as_deref())?;
    action.name = action.name.trim().to_string();
    action.group = action.group.trim().to_string();
    action.color = action.color.trim().to_string();
    if action.name.is_empty() {
        return Err("action name cannot be empty".to_string());
    }
    if action.group.is_empty() {
        action.group = "常用".to_string();
    }
    if action.id.is_empty() {
        action.id = uuid::Uuid::new_v4().to_string();
    }
    if action.color.is_empty() {
        action.color = "#6366F1".to_string();
    }
    action_icon::normalize_action_icon(action)?;
    if matches!(
        action.action_type,
        ActionType::System {
            operation: SystemOperation::Shutdown | SystemOperation::Restart
        }
    ) {
        action.confirm_before_run = true;
    }

    match &mut action.action_type {
        ActionType::LaunchApp { app_name, app_path } => {
            *app_name = app_name.trim().to_string();
            if let Some(path) = app_path {
                *path = path.trim().to_string();
                if path.is_empty() {
                    *app_path = None;
                }
            }
            if app_name.is_empty() && app_path.is_none() {
                return Err(
                    "select an installed application or enter an application path".to_string(),
                );
            }
        }
        ActionType::OpenPath { path } if path.trim().is_empty() => {
            return Err("enter a file or directory path".to_string());
        }
        ActionType::OpenUrl { url } if url.trim().is_empty() => {
            return Err("enter a URL".to_string());
        }
        ActionType::ShellCommand { command, .. } if command.trim().is_empty() => {
            return Err("enter a shell command".to_string());
        }
        ActionType::Hotkey { key, .. } if key.trim().is_empty() => {
            return Err("enter a shortcut".to_string());
        }
        ActionType::AppleScript { script } if script.trim().is_empty() => {
            return Err("enter an AppleScript".to_string());
        }
        ActionType::SetSystemVolume { volume } if *volume > 100 => {
            return Err("system volume must be between 0 and 100".to_string());
        }
        ActionType::ToggleShellCommand { start_command, .. } if start_command.trim().is_empty() => {
            return Err("enter a launch command".to_string());
        }
        _ => {}
    }
    Ok(())
}

fn reset_import_identity(action: &mut QuickAction) {
    action.revision = 0;
    action.id = uuid::Uuid::new_v4().to_string();
    action.source_preset_id = None;
    // A copied action must not claim an existing local shortcut on import.
    action.global_shortcut = None;
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::normalize_action;
    use crate::domain::quick_action::{ActionType, QuickAction};

    #[test]
    fn imported_actions_do_not_reuse_local_shortcuts() {
        let mut action = QuickAction::new(
            "Copy".into(),
            "Test".into(),
            ActionType::OpenUrl {
                url: "https://example.com".into(),
            },
        );
        action.global_shortcut = Some("Ctrl+Alt+L".into());
        let old_id = action.id.clone();
        super::reset_import_identity(&mut action);
        assert_ne!(action.id, old_id);
        assert!(action.global_shortcut.is_none());
    }

    #[test]
    fn action_text_with_builtin_icon_gets_canonical_svg() {
        let mut action: QuickAction = serde_json::from_str(
            r#"{
                "name": "打开下载目录",
                "icon_id": "folder",
                "action_type": { "type": "OpenPath", "path": "~/Downloads" }
            }"#,
        )
        .expect("minimal action should deserialize");

        normalize_action(&mut action).expect("minimal action should normalize");

        assert!(!action.id.is_empty());
        assert_eq!(action.group, "常用");
        assert_eq!(action.icon_id, "folder");
        assert!(action.icon_svg.starts_with("<svg"));
        assert_eq!(action.color, "#6366F1");
        assert!(matches!(action.action_type, ActionType::OpenPath { .. }));
    }
}

/// Kill an entire process group.
#[cfg(unix)]
fn kill_process_group(child: &Child) {
    let pid = child.id() as libc::pid_t;
    // Negative pid means "send to process group"
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    // Give processes a moment to shut down gracefully
    // Note: this runs in a spawn_blocking context, so std::thread::sleep is acceptable.
    std::thread::sleep(std::time::Duration::from_millis(200));
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

/// Kill an entire process tree on Windows using `taskkill /F /T`.
#[cfg(windows)]
fn kill_process_group(child: &Child) {
    let pid = child.id();
    // /T kills the whole process tree, /F forces termination
    let mut command = std::process::Command::new("taskkill");
    command.args(["/F", "/T", "/PID", &pid.to_string()]);
    let _ = crate::infrastructure::executor::output_with_timeout(
        command,
        std::time::Duration::from_secs(10),
    );
}
