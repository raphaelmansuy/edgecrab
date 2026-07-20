//! Typed agent-turn lifecycle phases and transition observability (AE9).

/// Coarse lifecycle phase for one agent turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPhase {
    Prologue,
    Preflight,
    ModelCall,
    Response,
    ToolDispatch,
    Epilogue,
    Complete,
}

impl TurnPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prologue => "prologue",
            Self::Preflight => "preflight",
            Self::ModelCall => "model_call",
            Self::Response => "response",
            Self::ToolDispatch => "tool_dispatch",
            Self::Epilogue => "epilogue",
            Self::Complete => "complete",
        }
    }
}

/// Tracks the current phase and emits tracing/metrics hooks on transitions.
#[derive(Debug)]
pub struct TurnPhaseTracker {
    current: TurnPhase,
}

impl TurnPhaseTracker {
    pub fn begin() -> Self {
        Self {
            current: TurnPhase::Prologue,
        }
    }

    pub const fn current(&self) -> TurnPhase {
        self.current
    }

    pub fn transition(&mut self, next: TurnPhase) {
        if self.current == next {
            return;
        }
        let previous = self.current;
        tracing::debug!(
            from = previous.as_str(),
            to = next.as_str(),
            "agent turn phase transition"
        );
        crate::otel_metrics::record_turn_phase_transition(previous.as_str(), next.as_str());
        self.current = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_transition_golden_sequence() {
        let mut tracker = TurnPhaseTracker::begin();
        let mut observed = vec![tracker.current().as_str()];
        for phase in [
            TurnPhase::Preflight,
            TurnPhase::ModelCall,
            TurnPhase::Response,
            TurnPhase::ToolDispatch,
            TurnPhase::Preflight,
            TurnPhase::ModelCall,
            TurnPhase::Response,
            TurnPhase::Epilogue,
            TurnPhase::Complete,
        ] {
            tracker.transition(phase);
            observed.push(tracker.current().as_str());
        }
        assert_eq!(
            observed.join(" -> "),
            "prologue -> preflight -> model_call -> response -> tool_dispatch -> preflight -> model_call -> response -> epilogue -> complete"
        );
    }
}
