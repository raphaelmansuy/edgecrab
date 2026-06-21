//! Turn prologue — one-time per-turn loop setup (spec 017 P1-1 / HA-55 stub).
//!
//! DRY: `conversation.rs` calls `begin_turn()` instead of inlining tracker init.

use crate::config::HarnessConfig;
use crate::turn_dispatch::TurnDispatchTrackers;

/// Mutable per-turn state initialized before the main ReAct loop.
#[derive(Debug)]
pub struct TurnPrologueState {
    pub trackers: TurnDispatchTrackers,
    pub compression_llm_failures: u32,
    pub pressure_warned: bool,
}

impl TurnPrologueState {
    /// Initialize dispatch trackers and per-turn counters.
    pub fn begin(harness: &HarnessConfig) -> Self {
        Self {
            trackers: TurnDispatchTrackers::with_harness(3, harness),
            compression_llm_failures: 0,
            pressure_warned: false,
        }
    }
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
    }
}
