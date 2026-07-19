//! Turn prologue — one-time per-turn loop setup (spec 017 / 022-014 wave-2).
//!
//! Hermes anchor: `agent/turn_context.py` (`build_turn_context`, preflight gates).
//!
//! DRY: `conversation.rs` calls [`TurnPrologueState::begin`] and preflight helpers
//! instead of inlining tracker init + estimate gates.

use crate::config::HarnessConfig;
use crate::turn_dispatch::TurnDispatchTrackers;

/// Mutable per-turn state initialized before the main ReAct loop.
#[derive(Debug)]
pub struct TurnPrologueState {
    pub trackers: TurnDispatchTrackers,
    pub compression_llm_failures: u32,
    pub pressure_warned: bool,
    /// Rough token estimate at turn open (optional; set by loop after first estimate).
    pub opening_rough_tokens: Option<usize>,
    /// Whether preflight compression was evaluated this turn.
    pub preflight_evaluated: bool,
    /// Whether preflight compression ran (vs deferred/skipped).
    pub preflight_ran: bool,
}

impl TurnPrologueState {
    /// Initialize dispatch trackers and per-turn counters.
    pub fn begin(harness: &HarnessConfig) -> Self {
        Self {
            trackers: TurnDispatchTrackers::with_harness(3, harness),
            compression_llm_failures: 0,
            pressure_warned: false,
            opening_rough_tokens: None,
            preflight_evaluated: false,
            preflight_ran: false,
        }
    }

    pub fn note_preflight(&mut self, ran: bool) {
        self.preflight_evaluated = true;
        self.preflight_ran = ran;
    }
}

/// Hermes `_should_run_preflight_estimate` parity (issue few-but-huge).
///
/// Returns `true` when either:
/// - message count exceeds protected ranges (historical gate), **or**
/// - a cheap rough token estimate already crosses the threshold (few large msgs).
pub fn should_run_preflight_estimate(
    message_count: usize,
    protect_first_n: usize,
    protect_last_n: usize,
    rough_tokens: usize,
    threshold_tokens: usize,
) -> bool {
    let protected = protect_first_n.saturating_add(protect_last_n);
    let count_gate = message_count > protected;
    let size_gate = threshold_tokens > 0 && rough_tokens >= threshold_tokens;
    count_gate || size_gate
}

/// Hermes `_compression_made_progress` parity.
///
/// Progress = fewer messages **or** material token reduction (>5%).
pub fn compression_made_progress(
    orig_len: usize,
    new_len: usize,
    orig_tokens: usize,
    new_tokens: usize,
) -> bool {
    if new_len < orig_len {
        return true;
    }
    orig_tokens > 0 && new_tokens < (orig_tokens * 95) / 100
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HarnessConfig;

    #[test]
    fn ha55_prologue_initializes_trackers() {
        let state = TurnPrologueState::begin(&HarnessConfig::default());
        assert!(!state.trackers.guardrail_halt);
        assert_eq!(state.compression_llm_failures, 0);
        assert!(!state.pressure_warned);
        assert!(!state.preflight_evaluated);
    }

    #[test]
    fn preflight_count_gate() {
        // 25 messages, protect 0+20 → need more than 20
        assert!(should_run_preflight_estimate(25, 0, 20, 100, 50_000));
        assert!(!should_run_preflight_estimate(10, 0, 20, 100, 50_000));
    }

    #[test]
    fn preflight_few_but_huge_size_gate() {
        // Only 5 messages but huge rough estimate
        assert!(should_run_preflight_estimate(5, 0, 20, 80_000, 50_000));
        assert!(!should_run_preflight_estimate(5, 0, 20, 1_000, 50_000));
    }

    #[test]
    fn compression_progress_token_only() {
        assert!(compression_made_progress(10, 10, 100_000, 80_000));
        assert!(!compression_made_progress(10, 10, 100_000, 96_000)); // only 4%
        assert!(compression_made_progress(12, 10, 100_000, 100_000));
    }
}
