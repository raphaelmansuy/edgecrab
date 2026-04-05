//! # Session mirroring — cross-platform message delivery records
//!
//! WHY mirroring: When a message is sent to a platform (via send_message
//! or cron delivery), this module records a "delivery-mirror" entry in
//! the target session so the agent has context about what was sent.
//!
//! Mirrors hermes-agent's `gateway/mirror.py`:
//! - Finds the matching session for a platform + chat_id pair
//! - Appends a mirror message to the session's transcript
//! - Never fatal — all errors are caught and logged

use edgecrab_state::SessionDb;
use std::sync::Arc;

/// Mirror a delivered message to the target session's transcript.
///
/// Finds the gateway session matching `platform` + `chat_id`, then writes
/// a mirror entry to the state DB. Returns `true` if mirrored successfully.
///
/// All errors are caught — this is never fatal.
pub fn mirror_to_session(
    db: &Arc<SessionDb>,
    platform: &str,
    chat_id: &str,
    message_text: &str,
    source_label: &str,
    thread_id: Option<&str>,
) -> bool {
    // Find the session matching this platform + chat_id
    let session_id = match find_session_id(db, platform, chat_id, thread_id) {
        Some(id) => id,
        None => {
            tracing::debug!(platform, chat_id, thread_id, "mirror: no session found");
            return false;
        }
    };

    // Append mirror message to the session
    let msg = edgecrab_types::Message {
        role: edgecrab_types::message::Role::Assistant,
        content: Some(edgecrab_types::message::Content::Text(format!(
            "[mirror from {source_label}] {message_text}"
        ))),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        reasoning: None,
        finish_reason: None,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    match db.save_message(&session_id, &msg, now) {
        Ok(()) => {
            tracing::debug!(session_id, source_label, "mirror: wrote to session");
            true
        }
        Err(e) => {
            tracing::debug!(error = %e, "mirror: failed to write");
            false
        }
    }
}

/// Find the active session_id for a platform + chat_id pair.
///
/// Scans recent sessions and matches by source (platform name) and the
/// persisted routing key in `user_id`. Gateway sessions persist `user_id`
/// as `chat_id` or `chat_id:thread_id`, which makes mirroring and channel
/// discovery deterministic without overloading the session title.
/// Returns the most recently active matching session.
fn find_session_id(
    db: &Arc<SessionDb>,
    platform: &str,
    chat_id: &str,
    thread_id: Option<&str>,
) -> Option<String> {
    let sessions = db.list_sessions(100).ok()?;
    let platform_lower = platform.to_lowercase();
    let exact_routing_key = match thread_id {
        Some(thread_id) if !thread_id.is_empty() => format!("{chat_id}:{thread_id}"),
        _ => chat_id.to_string(),
    };

    let mut best_match: Option<(String, f64)> = None;

    for session in sessions {
        let source = session.source.to_lowercase();
        if source != platform_lower {
            continue;
        }

        let record = match db.get_session(&session.id) {
            Ok(Some(record)) => record,
            _ => continue,
        };
        let Some(session_chat_id) = record.user_id.as_deref() else {
            continue;
        };
        if session_chat_id != exact_routing_key && session_chat_id != chat_id {
            continue;
        }

        let started = session.started_at;
        match &best_match {
            Some((_, best_time)) if started > *best_time => {
                best_match = Some((session.id, started));
            }
            None => {
                best_match = Some((session.id, started));
            }
            _ => {}
        }
    }

    best_match.map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_module_exports_function() {
        let _symbol = mirror_to_session
            as fn(
                &std::sync::Arc<edgecrab_state::SessionDb>,
                &str,
                &str,
                &str,
                &str,
                Option<&str>,
            ) -> bool;
    }
}
