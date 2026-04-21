//! # Mission Steering
//!
//! Allows users to inject guidance into a running agent loop without stopping it.
//!
//! ## Design
//!
//! ```text
//! TUI (App)
//!   Ctrl+S key → SteeringOverlay opens
//!   Enter      → steer_tx.send(SteeringEvent { .. })
//!              → pending_steer_count += 1 (optimistic)
//!
//!              │  mpsc::UnboundedChannel
//!              ▼
//! execute_loop
//!   at every tool-dispatch boundary:
//!     drain_pending_steers(steer_rx)
//!       → None                (nothing pending)
//!       → Some("[⛵ STEER] …") → push as Message::user
//!                              → emit StreamEvent::SteerApplied
//! ```
//!
//! ## First Principles
//!
//! * **Loop safety** — `UnboundedSender` is `Send + Clone`, never blocks the TUI.
//! * **Cache preservation** — system prompt is NEVER rebuilt to inject a steer.
//! * **Injection only at tool boundaries** — preserves strict user/assistant alternation.
//! * **Security** — all steer text is scanned for prompt injection before injection.

use std::time::Instant;

// ─── Public Types ─────────────────────────────────────────────────────────────

/// Classifies the user's intent when steering a running agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SteeringKind {
    /// Add context or a hint — the agent loop continues normally.
    ///
    /// Does NOT signal the cancellation token. The steer is held until the
    /// current tool finishes, then injected before the next API call.
    #[default]
    Hint,

    /// Suggest a different approach — a strong signal to the LLM.
    ///
    /// Does NOT signal the cancellation token. Semantically stronger than
    /// Hint; the LLM receives an explicit "please redirect" cue.
    Redirect,

    /// Request a graceful stop after the current tool completes.
    ///
    /// Signals the cancellation token so long-running tools are interrupted
    /// at their next `cancel.is_cancelled()` checkpoint, then the steer
    /// message is injected as the agent exits.
    Stop,
}

impl std::fmt::Display for SteeringKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hint => write!(f, "HINT"),
            Self::Redirect => write!(f, "REDIRECT"),
            Self::Stop => write!(f, "STOP"),
        }
    }
}

/// A user-initiated guidance signal to inject into a running agent loop.
#[derive(Debug, Clone)]
pub struct SteeringEvent {
    /// The intent / strength of this steer.
    pub kind: SteeringKind,
    /// The guidance text provided by the user.
    pub message: String,
    /// When this event was created.
    pub timestamp: Instant,
}

impl SteeringEvent {
    /// Construct a new steering event with the current timestamp.
    pub fn new(kind: SteeringKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            timestamp: Instant::now(),
        }
    }
}

/// Sender end of the steering channel.
///
/// Cloneable and `Send`. Store in `App`; send events from the TUI keybinding handler.
pub type SteeringSender = tokio::sync::mpsc::UnboundedSender<SteeringEvent>;

/// Receiver end of the steering channel.
///
/// Held by `execute_loop` for the duration of the conversation. Non-cloneable.
pub type SteeringReceiver = tokio::sync::mpsc::UnboundedReceiver<SteeringEvent>;

/// Maximum byte length of a single steer message (EC-11: prevent context overflow).
pub const MAX_STEER_MESSAGE_BYTES: usize = 2000;

// ─── Channel helpers ──────────────────────────────────────────────────────────

/// Create a new steering channel pair.
pub fn steering_channel() -> (SteeringSender, SteeringReceiver) {
    tokio::sync::mpsc::unbounded_channel()
}

// ─── Drain + build helpers ────────────────────────────────────────────────────

/// Drain all pending steering events from the receiver (non-blocking).
///
/// Returns `None` when the channel is empty. Returns the combined message
/// string when one or more events are pending.
///
/// ## Security
///
/// All steer text passes through the injection scanner before being returned.
/// High-severity threats are replaced with a blocked placeholder.
///
/// ## EC-11 (message too long)
///
/// Individual messages are truncated to `MAX_STEER_MESSAGE_BYTES`.
pub fn drain_pending_steers(rx: &mut SteeringReceiver) -> Option<(String, SteeringKind)> {
    let mut events: Vec<SteeringEvent> = Vec::new();

    // Non-blocking drain — collect all currently-buffered events.
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }

    if events.is_empty() {
        return None;
    }

    // Determine the "strongest" kind in the batch.
    // Stop > Redirect > Hint
    let strongest_kind = events
        .iter()
        .max_by_key(|e| match e.kind {
            SteeringKind::Stop => 2u8,
            SteeringKind::Redirect => 1,
            SteeringKind::Hint => 0,
        })
        .map(|e| e.kind.clone())
        .unwrap_or(SteeringKind::Hint);

    let message = build_steer_message(&events);
    Some((message, strongest_kind))
}

/// Build the injection-safe combined steer message from a batch of events.
pub fn build_steer_message(events: &[SteeringEvent]) -> String {
    if events.is_empty() {
        return String::new();
    }

    let combined = if events.len() == 1 {
        let text = truncate_steer_text(&events[0].message);
        format!("[⛵ STEER] {text}")
    } else {
        events
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let text = truncate_steer_text(&e.message);
                format!("[⛵ STEER] ({}) {}", i + 1, text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Security: scan for prompt injection
    scan_and_sanitize_steer(combined)
}

/// Truncate a steer message to `MAX_STEER_MESSAGE_BYTES` at a valid char boundary.
fn truncate_steer_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.len() <= MAX_STEER_MESSAGE_BYTES {
        return trimmed.to_string();
    }
    // Find safe UTF-8 boundary
    let mut end = MAX_STEER_MESSAGE_BYTES;
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}… (truncated at {} chars)",
        &trimmed[..end],
        MAX_STEER_MESSAGE_BYTES
    )
}

/// Scan a combined steer message for prompt injection.
///
/// On high-severity threat, replace with a blocked placeholder.
/// On medium/low severity, log a warning but allow through.
fn scan_and_sanitize_steer(message: String) -> String {
    let threats = crate::prompt_builder::scan_for_injection(&message);
    if threats.is_empty() {
        return message;
    }

    let has_high = threats
        .iter()
        .any(|t| t.severity == crate::prompt_builder::ThreatSeverity::High);

    if has_high {
        tracing::warn!(
            threats = threats.len(),
            "steer message blocked: high-severity injection pattern detected"
        );
        "[⛵ STEER] [blocked: content flagged by injection scanner]".to_string()
    } else {
        tracing::warn!(
            threats = threats.len(),
            "steer message: medium/low injection patterns detected, allowing through"
        );
        message
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_single_steer() {
        let events = vec![SteeringEvent::new(SteeringKind::Hint, "focus on auth")];
        let msg = build_steer_message(&events);
        assert_eq!(msg, "[⛵ STEER] focus on auth");
    }

    #[test]
    fn test_build_multiple_steers() {
        let events = vec![
            SteeringEvent::new(SteeringKind::Hint, "focus on auth"),
            SteeringEvent::new(SteeringKind::Redirect, "skip the DB step"),
        ];
        let msg = build_steer_message(&events);
        assert!(msg.contains("(1) focus on auth"));
        assert!(msg.contains("(2) skip the DB step"));
    }

    #[test]
    fn test_truncation() {
        let long_text = "x".repeat(3000);
        let events = vec![SteeringEvent::new(SteeringKind::Hint, long_text)];
        let msg = build_steer_message(&events);
        assert!(msg.contains("truncated"));
        assert!(msg.len() < 2100); // should be well under
    }

    #[test]
    fn test_strongest_kind_stop_wins() {
        let events = vec![
            SteeringEvent::new(SteeringKind::Hint, "a"),
            SteeringEvent::new(SteeringKind::Stop, "b"),
            SteeringEvent::new(SteeringKind::Redirect, "c"),
        ];
        let (_, kind) = drain_pending_steers(&mut {
            let (tx, mut rx) = steering_channel();
            for e in events {
                let _ = tx.send(e);
            }
            rx
        })
        .expect("drain should return the strongest steering event");
        assert_eq!(kind, SteeringKind::Stop);
    }

    #[tokio::test]
    async fn test_drain_empty() {
        let (_tx, mut rx) = steering_channel();
        assert!(drain_pending_steers(&mut rx).is_none());
    }

    #[tokio::test]
    async fn test_drain_collects_all() {
        let (tx, mut rx) = steering_channel();
        for i in 0..5 {
            let _ = tx.send(SteeringEvent::new(SteeringKind::Hint, format!("hint {i}")));
        }
        let result = drain_pending_steers(&mut rx);
        assert!(result.is_some());
        let (msg, _) = result.expect("drain should return collected steering hints");
        assert!(msg.contains("(1)") && msg.contains("(5)"));
    }
}
