//! Platform adapter failure circuit breaker (Wave 3 — minimal stub).
//!
//! Tracks consecutive delivery failures per platform name. After
//! [`FAILURE_THRESHOLD`] failures the circuit opens and `deliver()` short-circuits
//! until a successful send resets the counter.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const FAILURE_THRESHOLD: u32 = 5;

static GLOBAL: OnceLock<PlatformCircuitBreaker> = OnceLock::new();

/// Process-wide platform failure counter.
pub struct PlatformCircuitBreaker {
    failures: Mutex<HashMap<String, u32>>,
    open: Mutex<HashMap<String, bool>>,
}

impl Default for PlatformCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformCircuitBreaker {
    pub fn new() -> Self {
        Self {
            failures: Mutex::new(HashMap::new()),
            open: Mutex::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static Self {
        GLOBAL.get_or_init(Self::new)
    }

    pub fn is_open(&self, platform: &str) -> bool {
        self.open
            .lock()
            .ok()
            .and_then(|g| g.get(platform).copied())
            .unwrap_or(false)
    }

    /// Record a delivery failure. Returns `true` when the circuit just opened.
    pub fn record_failure(&self, platform: &str) -> bool {
        let mut failures = match self.failures.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let count = failures.entry(platform.to_string()).or_insert(0);
        *count = count.saturating_add(1);
        if *count >= FAILURE_THRESHOLD {
            if let Ok(mut open) = self.open.lock() {
                open.insert(platform.to_string(), true);
            }
            tracing::warn!(
                platform,
                failures = *count,
                "gateway platform circuit breaker opened"
            );
            return true;
        }
        false
    }

    pub fn record_success(&self, platform: &str) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.remove(platform);
        }
        if let Ok(mut open) = self.open.lock() {
            open.remove(platform);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_after_threshold() {
        let b = PlatformCircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            b.record_failure("discord");
        }
        assert!(b.is_open("discord"));
        b.record_success("discord");
        assert!(!b.is_open("discord"));
    }
}
