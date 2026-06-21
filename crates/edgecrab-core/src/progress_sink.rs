//! Progress sink abstraction — unifies TUI stream events behind one trait (spec 015 P2.1).

use tokio::sync::mpsc::UnboundedSender;

use crate::agent::StreamEvent;

/// Harness progress surface — conversation loop emits; CLI/gateway adapt.
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: StreamEvent);
}

/// Adapter that forwards to the existing `StreamEvent` channel.
pub struct StreamEventSink {
    tx: UnboundedSender<StreamEvent>,
}

impl StreamEventSink {
    pub fn new(tx: UnboundedSender<StreamEvent>) -> Self {
        Self { tx }
    }
}

impl ProgressSink for StreamEventSink {
    fn emit(&self, event: StreamEvent) {
        let _ = self.tx.send(event);
    }
}

/// DRY helper for optional stream channels in the conversation loop.
pub fn emit_optional(tx: Option<&UnboundedSender<StreamEvent>>, event: StreamEvent) {
    if let Some(tx) = tx {
        StreamEventSink::new(tx.clone()).emit(event);
    }
}

/// Activity shelf / status notices.
pub fn emit_activity(tx: Option<&UnboundedSender<StreamEvent>>, notice: impl Into<String>) {
    emit_optional(tx, StreamEvent::ActivityNotice(notice.into()));
}

/// Turn completion — unified exit surface for TUI/gateway/ACP.
pub fn emit_run_finished(
    tx: Option<&UnboundedSender<StreamEvent>>,
    outcome: edgecrab_types::RunOutcome,
) {
    emit_optional(tx, StreamEvent::RunFinished { outcome });
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_types::{CompletionDecision, ExitReason, RunOutcome};

    #[test]
    fn stream_event_sink_forwards() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = StreamEventSink::new(tx);
        let outcome = RunOutcome::new(
            CompletionDecision::Completed,
            ExitReason::ModelReturnedFinalText,
            "Done.",
        );
        sink.emit(StreamEvent::RunFinished { outcome });
        let event = rx.try_recv().expect("event");
        assert!(matches!(event, StreamEvent::RunFinished { .. }));
    }

    #[test]
    fn emit_optional_noops_without_channel() {
        emit_optional(None, StreamEvent::ActivityNotice("ok".into()));
    }

    #[test]
    fn emit_activity_forwards_notice() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        emit_activity(Some(&tx), "compress done");
        let event = rx.try_recv().expect("event");
        assert!(matches!(event, StreamEvent::ActivityNotice(ref s) if s == "compress done"));
    }

    #[test]
    fn emit_run_finished_forwards_outcome() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let outcome = RunOutcome::new(
            CompletionDecision::Incomplete,
            ExitReason::Interrupted,
            "Stopped.",
        );
        emit_run_finished(Some(&tx), outcome);
        assert!(matches!(rx.try_recv(), Ok(StreamEvent::RunFinished { .. })));
    }
}
