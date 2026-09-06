use arcrelay_protocol::server::OutputLine;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use tokio::sync::broadcast;

const MAX_LINES_PER_ACTION: usize = 500;
const MAX_LINE_BYTES: usize = 16 * 1024;
const MAX_BYTES_PER_ACTION: usize = 512 * 1024;
const MAX_BUFFERED_ACTIONS: usize = 64;
const MAX_BUFFERED_BYTES: usize = 8 * 1024 * 1024;
const BROADCAST_CAPACITY: usize = 256;
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutputSnapshot {
    pub action_id: String,
    pub revision: u64,
    pub lines: Vec<String>,
}

/// Manages ring-buffered output for action processes, with a broadcast
/// channel so the server can push lines to subscribed mobile clients.
pub struct OutputManager {
    state: Mutex<OutputState>,
    tx: broadcast::Sender<OutputLine>,
}

#[derive(Default)]
struct OutputState {
    buffers: HashMap<String, OutputBuffer>,
    order: VecDeque<String>,
    total_bytes: usize,
    revision: u64,
}

struct OutputBuffer {
    revision: u64,
    lines: VecDeque<String>,
    bytes: usize,
}

impl OutputManager {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            state: Mutex::new(OutputState::default()),
            tx,
        }
    }

    /// Append a line of output for the given action.
    /// Also broadcasts to any subscribed receivers.
    pub fn append(&self, action_id: &str, text: String) {
        let text = truncate_utf8(text, MAX_LINE_BYTES);
        let action_id = action_id.to_string();
        {
            let mut state = self.state.lock().unwrap();
            state.revision += 1;
            let revision = state.revision;
            let mut removed_bytes = 0;
            let buf = state
                .buffers
                .entry(action_id.to_string())
                .or_insert_with(|| OutputBuffer {
                    revision,
                    lines: VecDeque::with_capacity(MAX_LINES_PER_ACTION),
                    bytes: 0,
                });
            buf.revision = revision;
            buf.bytes += text.len();
            buf.lines.push_back(text.clone());
            while buf.lines.len() > MAX_LINES_PER_ACTION || buf.bytes > MAX_BYTES_PER_ACTION {
                if let Some(removed) = buf.lines.pop_front() {
                    buf.bytes = buf.bytes.saturating_sub(removed.len());
                    removed_bytes += removed.len();
                } else {
                    break;
                }
            }
            state.total_bytes = state
                .total_bytes
                .saturating_add(text.len())
                .saturating_sub(removed_bytes);
            state.order.retain(|current| current != &action_id);
            state.order.push_back(action_id.clone());
            while state.buffers.len() > MAX_BUFFERED_ACTIONS
                || state.total_bytes > MAX_BUFFERED_BYTES
            {
                let Some(expired) = state.order.pop_front() else {
                    break;
                };
                if let Some(buffer) = state.buffers.remove(&expired) {
                    state.total_bytes = state.total_bytes.saturating_sub(buffer.bytes);
                }
            }
        }
        // Best-effort broadcast (no receivers = ignored)
        let _ = self.tx.send(OutputLine { action_id, text });
    }

    /// Get all buffered lines for an action.
    pub fn get_lines(&self, action_id: &str) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state
            .buffers
            .get(action_id)
            .map(|buffer| buffer.lines.iter().cloned().collect())
            .unwrap_or_default()
    }

    pub fn snapshot(&self, action_id: &str) -> ActionOutputSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let buffer = state.buffers.get(action_id);
        ActionOutputSnapshot {
            action_id: action_id.into(),
            revision: buffer.map_or(state.revision, |b| b.revision),
            lines: buffer
                .map(|b| {
                    b.lines
                        .iter()
                        .skip(b.lines.len().saturating_sub(80))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
    pub fn snapshots(&self) -> Vec<ActionOutputSnapshot> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state
            .buffers
            .iter()
            .map(|(id, b)| ActionOutputSnapshot {
                action_id: id.clone(),
                revision: b.revision,
                lines: b
                    .lines
                    .iter()
                    .skip(b.lines.len().saturating_sub(80))
                    .cloned()
                    .collect(),
            })
            .collect()
    }

    /// Subscribe to the output broadcast (returns a new receiver).
    pub fn subscribe(&self) -> broadcast::Receiver<OutputLine> {
        self.tx.subscribe()
    }
}

fn truncate_utf8(mut value: String, maximum: usize) -> String {
    if value.len() <= maximum {
        return value;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.truncate(end);
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_the_oldest_action_buffer_at_the_global_limit() {
        let manager = OutputManager::new();
        for index in 0..=MAX_BUFFERED_ACTIONS {
            manager.append(&format!("action-{index}"), "line".into());
        }

        assert!(manager.get_lines("action-0").is_empty());
        assert_eq!(
            manager.get_lines(&format!("action-{MAX_BUFFERED_ACTIONS}")),
            ["line"]
        );
    }

    #[test]
    fn truncates_output_without_splitting_utf8() {
        let truncated = truncate_utf8("界".repeat(MAX_LINE_BYTES), MAX_LINE_BYTES);
        assert!(truncated.len() <= MAX_LINE_BYTES);
        assert!(truncated.chars().all(|character| character == '界'));
    }
}
