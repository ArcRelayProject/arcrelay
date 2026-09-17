use std::{collections::HashMap, sync::Mutex};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct RemoteWork {
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    stopping: bool,
    tokens: HashMap<String, CancellationToken>,
}
impl RemoteWork {
    pub fn register(&self, id: &str) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.stopping {
            return Err("remote file service is stopping".into());
        }
        if state.tokens.len() >= 16 {
            return Err("too many remote file transfers; wait for a running transfer".into());
        }
        state.tokens.insert(id.into(), CancellationToken::new());
        Ok(())
    }
    pub fn token(&self, id: &str) -> Result<CancellationToken, String> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tokens
            .get(id)
            .cloned()
            .ok_or_else(|| "remote file transfer not found".into())
    }
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        self.token(id)?.cancel();
        Ok(())
    }
    pub fn finish(&self, id: &str) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .tokens
            .remove(id);
    }
    pub fn shutdown(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stopping = true;
        for token in state.tokens.values() {
            token.cancel();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_cancellation_and_shutdown_do_not_require_a_runtime() {
        let work = RemoteWork::default();
        for id in 0..16 {
            work.register(&id.to_string()).unwrap();
        }
        assert!(work.register("overflow").is_err());
        work.cancel("0").unwrap();
        assert!(work.token("0").unwrap().is_cancelled());
        assert!(!work.token("1").unwrap().is_cancelled());
        work.finish("0");
        work.register("replacement").unwrap();
        work.shutdown();
        assert!(work.token("1").unwrap().is_cancelled());
        assert!(work.register("late").is_err());
    }
}
