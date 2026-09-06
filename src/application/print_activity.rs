use std::{collections::HashMap, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Default)]
pub struct PrintActivityObservers {
    tasks: Mutex<HashMap<String, tauri::async_runtime::JoinHandle<()>>>,
}
impl PrintActivityObservers {
    pub fn stop(&self, label: &str) {
        if let Some(task) = self
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(label)
        {
            task.abort();
        }
    }
    pub fn start(&self, app: AppHandle, label: String) {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if tasks.contains_key(&label) {
            return;
        }
        let window_label = label.clone();
        let task = tauri::async_runtime::spawn(async move {
            let mut previous = serde_json::Value::Null;
            let mut reported_error = String::new();
            loop {
                let Some(window) = app.get_webview_window(&window_label) else {
                    break;
                };
                if window.is_visible().unwrap_or(false) {
                    match crate::commands::get_print_job_activity(
                        app.state::<crate::backend::DesktopState>(),
                    )
                    .await
                    {
                        Ok(activity) => {
                            reported_error.clear();
                            if let Ok(value) = serde_json::to_value(&activity) {
                                let mut comparable = value.clone();
                                comparable.as_object_mut().unwrap().remove("revision");
                                if comparable != previous {
                                    previous = comparable;
                                    let _ = window.emit("print-job-activity", value);
                                }
                            }
                        }
                        Err(error) => {
                            if reported_error != error.to_string() {
                                reported_error = error.to_string();
                                let _ = window.emit("print-job-activity-error", error);
                            }
                        }
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        });
        tasks.insert(label, task);
    }
}
