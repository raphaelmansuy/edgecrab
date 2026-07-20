//! Platform adapter failure circuit breaker.
//!
//! Tracks consecutive delivery failures per platform name. After
//! the configured threshold failures the circuit opens and delivery short-circuits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

static GLOBAL: OnceLock<PlatformCircuitBreaker> = OnceLock::new();

#[derive(Debug, Clone, Copy, Default)]
struct CircuitState {
    consecutive_failures: u32,
    open: bool,
}

/// Process-wide platform failure counter.
pub struct PlatformCircuitBreaker {
    threshold: AtomicU32,
    states: Mutex<HashMap<String, CircuitState>>,
}

impl Default for PlatformCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformCircuitBreaker {
    pub fn new() -> Self {
        Self::with_threshold(5)
    }

    pub fn with_threshold(threshold: u32) -> Self {
        Self {
            threshold: AtomicU32::new(threshold.max(1)),
            states: Mutex::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static Self {
        GLOBAL.get_or_init(Self::new)
    }

    pub fn set_threshold(&self, threshold: u32) {
        self.threshold.store(threshold.max(1), Ordering::Relaxed);
    }

    pub fn is_open(&self, platform: &str) -> bool {
        self.states
            .lock()
            .ok()
            .and_then(|g| g.get(platform).map(|state| state.open))
            .unwrap_or(false)
    }

    /// Record a delivery failure. Returns `true` when the circuit just opened.
    pub fn record_failure(&self, platform: &str) -> bool {
        let mut states = match self.states.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let state = states.entry(platform.to_string()).or_default();
        if state.open {
            return false;
        }
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= self.threshold.load(Ordering::Relaxed) {
            state.open = true;
            tracing::warn!(
                platform,
                failures = state.consecutive_failures,
                "gateway platform circuit breaker opened"
            );
            return true;
        }
        false
    }

    pub fn record_success(&self, platform: &str) {
        if let Ok(mut states) = self.states.lock() {
            states.remove(platform);
        }
    }

    /// Operator/probe reset for an open circuit.
    pub fn reset(&self, platform: &str) {
        self.record_success(platform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold() {
        let b = PlatformCircuitBreaker::with_threshold(3);
        assert!(!b.record_failure("discord"));
        assert!(!b.record_failure("discord"));
        assert!(b.record_failure("discord"));
        assert!(b.is_open("discord"));
        assert!(
            !b.record_failure("discord"),
            "opening is a one-shot transition"
        );
        b.record_success("discord");
        assert!(!b.is_open("discord"));
    }

    #[test]
    fn success_resets_consecutive_failures() {
        let b = PlatformCircuitBreaker::with_threshold(2);
        assert!(!b.record_failure("slack"));
        b.record_success("slack");
        assert!(!b.record_failure("slack"));
        assert!(!b.is_open("slack"));
    }
}
