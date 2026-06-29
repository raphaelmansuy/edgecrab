//! Shared clarify callback wiring — Telegram inline buttons resolve via the gateway broker.

use std::sync::{Arc, OnceLock};

use crate::interactions::InteractionBroker;

static BROKER: OnceLock<Arc<InteractionBroker>> = OnceLock::new();

/// Install the gateway interaction broker (call once at gateway startup).
pub fn install_interaction_broker(broker: Arc<InteractionBroker>) {
    let _ = BROKER.set(broker);
}

/// Peek clarify question/choices by interaction id (button callback lookup).
pub async fn peek_clarify(interaction_id: u64) -> Option<(String, Option<Vec<String>>)> {
    let broker = BROKER.get()?;
    broker.peek_clarify_by_id(interaction_id).await
}

/// Resolve a clarify button callback by pending interaction id.
pub async fn resolve_clarify_button(interaction_id: u64, answer: String) -> bool {
    let Some(broker) = BROKER.get() else {
        return false;
    };
    broker.resolve_clarify_by_id(interaction_id, answer).await
}
