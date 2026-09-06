use super::SoundEvent;
use arcrelay_automation::{ActivityStatus, AutomationActivity};
use std::collections::{HashMap, VecDeque};

const MAX_TRACKED_RUNS: usize = 256;
#[derive(Default)]
struct Run {
    started: bool,
    confirmations: Vec<Option<usize>>,
    terminal: bool,
    recovered: bool,
}
pub(crate) struct AutomationSoundTracker {
    started_at: chrono::DateTime<chrono::Utc>,
    runs: HashMap<String, Run>,
    order: VecDeque<String>,
}
impl Default for AutomationSoundTracker {
    fn default() -> Self {
        Self {
            started_at: chrono::Utc::now(),
            runs: HashMap::new(),
            order: VecDeque::new(),
        }
    }
}
impl AutomationSoundTracker {
    pub fn update(&mut self, activity: &AutomationActivity) -> Option<SoundEvent> {
        let confirmation = activity
            .steps
            .iter()
            .find(|step| step.status == ActivityStatus::AwaitingConfirmation)
            .map(|step| step.index);
        // Infer the baseline from incoming metadata; do not load historical
        // activities or definitions merely to initialize sound feedback.
        if activity.created_at < self.started_at && !self.runs.contains_key(&activity.id) {
            self.reserve(&activity.id);
            self.runs.insert(
                activity.id.clone(),
                Run {
                    recovered: true,
                    started: activity
                        .steps
                        .iter()
                        .any(|step| step.started_at.is_some_and(|at| at < self.started_at)),
                    terminal: activity.finished_at.is_some_and(|at| at < self.started_at),
                    confirmations: activity.confirmed_steps.iter().copied().map(Some).collect(),
                },
            );
        }
        self.transition(&activity.id, activity.status, confirmation)
    }
    fn reserve(&mut self, id: &str) {
        if !self.runs.contains_key(id) {
            while self.runs.len() >= MAX_TRACKED_RUNS {
                if let Some(old) = self.order.pop_front() {
                    self.runs.remove(&old);
                }
            }
            self.order.push_back(id.to_string());
        }
    }
    pub(super) fn transition(
        &mut self,
        id: &str,
        status: ActivityStatus,
        confirmation: Option<usize>,
    ) -> Option<SoundEvent> {
        self.reserve(id);
        let run = self.runs.entry(id.to_string()).or_default();
        if run.terminal {
            return None;
        }
        match status {
            ActivityStatus::Running if !run.started => {
                run.started = true;
                Some(SoundEvent::AutomationStarted)
            }
            ActivityStatus::AwaitingConfirmation if !run.confirmations.contains(&confirmation) => {
                run.confirmations.push(confirmation);
                Some(SoundEvent::AutomationConfirmation)
            }
            ActivityStatus::Succeeded | ActivityStatus::Failed | ActivityStatus::Interrupted => {
                run.terminal = true;
                match status {
                    ActivityStatus::Succeeded => Some(SoundEvent::AutomationSucceeded),
                    ActivityStatus::Failed => Some(SoundEvent::AutomationFailed),
                    ActivityStatus::Interrupted if !run.recovered => {
                        Some(SoundEvent::AutomationInterrupted)
                    }
                    _ => None,
                }
            }
            ActivityStatus::Canceled | ActivityStatus::Skipped => {
                run.terminal = true;
                None
            }
            _ => None,
        }
    }
}
