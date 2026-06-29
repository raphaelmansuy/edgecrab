//! In-process kanban worker registry — interrupt isolated agents on timeout.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::agent::Agent;

fn registry() -> &'static Mutex<HashMap<String, Weak<Agent>>> {
    static REG: OnceLock<Mutex<HashMap<String, Weak<Agent>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a gateway-spawned kanban worker for mid-flight cancellation.
pub fn register_worker(task_id: &str, agent: &Arc<Agent>) {
    if let Ok(mut map) = registry().lock() {
        map.insert(task_id.to_string(), Arc::downgrade(agent));
    }
}

/// Drop worker registration when the chat turn finishes.
pub fn unregister_worker(task_id: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(task_id);
    }
}

/// Hard-interrupt a running worker (e.g. max_runtime exceeded).
pub fn cancel_worker(task_id: &str) -> bool {
    let agent = registry().lock().ok().and_then(|map| {
        map.get(task_id)
            .and_then(|weak| weak.upgrade())
    });
    if let Some(agent) = agent {
        agent.interrupt();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unregister_missing_is_noop() {
        unregister_worker("kb-nonexistent");
    }
}
