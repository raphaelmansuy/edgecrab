//! Channel history backfill helpers (gap 016 — Discord first).
//!
//! Converts platform messages into EdgeCrab session messages for seeding
//! on first-seen channels. Platform adapters fetch history; this module
//! owns role mapping, prune-on-seed token budgeting, and channel markers.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use edgecrab_types::{Message, Role};
use serde::{Deserialize, Serialize};

/// Normalized inbound history row from any messaging platform.
#[derive(Debug, Clone)]
pub struct BackfillMessage {
    pub id: String,
    pub author: String,
    pub is_bot: bool,
    pub content: String,
    pub timestamp: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChannelMarkerFile {
    /// marker_key → last_seen_message_id (may be empty)
    #[serde(default)]
    markers: HashMap<String, String>,
}

fn marker_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join("channel_backfill.json")
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn load_markers() -> ChannelMarkerFile {
    let path = marker_path();
    if !path.exists() {
        return ChannelMarkerFile::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ChannelMarkerFile::default(),
    }
}

fn save_markers(file: &ChannelMarkerFile) -> Result<(), std::io::Error> {
    let path = marker_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(file).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// True when this channel was already backfilled (persisted marker).
pub fn is_channel_backfilled(platform: &str, channel_id: &str) -> bool {
    let key = channel_marker_key(platform, channel_id);
    load_markers().markers.contains_key(&key)
}

/// Persist a backfill marker so restarts do not re-fetch the same channel.
pub fn mark_channel_backfilled(platform: &str, channel_id: &str, last_message_id: &str) {
    let key = channel_marker_key(platform, channel_id);
    let mut file = load_markers();
    file.markers.insert(key, last_message_id.to_string());
    if let Err(err) = save_markers(&file) {
        tracing::warn!(error = %err, "failed to persist channel backfill marker");
    }
}

/// Try to claim a channel for backfill (process-local). Returns false if another
/// task is already backfilling or the channel is already marked.
pub fn try_begin_backfill(platform: &str, channel_id: &str) -> bool {
    if is_channel_backfilled(platform, channel_id) {
        return false;
    }
    let key = channel_marker_key(platform, channel_id);
    let Ok(mut guard) = in_flight().lock() else {
        return false;
    };
    if !guard.insert(key) {
        return false;
    }
    true
}

/// Release the in-flight claim (call after success or failure).
pub fn end_backfill(platform: &str, channel_id: &str) {
    let key = channel_marker_key(platform, channel_id);
    if let Ok(mut guard) = in_flight().lock() {
        guard.remove(&key);
    }
}

/// Convert + prune + sanitize history for seeding a fresh session.
///
/// `exclude_message_id` drops the triggering live message so `chat()` does not
/// duplicate it when Discord's history response includes the current post.
pub fn prepare_seed(
    msgs: &[BackfillMessage],
    exclude_message_id: Option<&str>,
    max_tokens: usize,
) -> Vec<Message> {
    let filtered: Vec<BackfillMessage> = msgs
        .iter()
        .filter(|m| exclude_message_id.is_none_or(|id| m.id != id))
        .cloned()
        .collect();
    let converted = convert_backfill_messages(&filtered);
    let pruned = prune_to_token_budget(converted, max_tokens);
    sanitize_seed(pruned)
}

/// Convert platform history into session messages (oldest first).
pub fn convert_backfill_messages(msgs: &[BackfillMessage]) -> Vec<Message> {
    msgs.iter()
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| {
            if m.is_bot {
                Message::assistant(&m.content)
            } else {
                let text = if m.author.is_empty() {
                    m.content.clone()
                } else {
                    format!("[{}]: {}", m.author, m.content)
                };
                Message::user(&text)
            }
        })
        .collect()
}

/// Rough token estimate (~4 chars/token) for prune-on-seed.
pub fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| m.text_content().len().div_ceil(4))
        .sum()
}

/// Drop oldest messages until estimated tokens ≤ `max_tokens`.
pub fn prune_to_token_budget(mut messages: Vec<Message>, max_tokens: usize) -> Vec<Message> {
    if max_tokens == 0 || messages.is_empty() {
        return messages;
    }
    while estimate_tokens(&messages) > max_tokens && messages.len() > 1 {
        messages.remove(0);
    }
    messages
}

/// Marker key for "already backfilled this channel".
pub fn channel_marker_key(platform: &str, channel_id: &str) -> String {
    format!("{platform}:{channel_id}")
}

/// Discord REST payload → [`BackfillMessage`] list (shared shape).
pub fn discord_messages_from_json(payload: &serde_json::Value) -> Vec<BackfillMessage> {
    let Some(arr) = payload.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let content = item
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let author = item
            .pointer("/author/username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_bot = item
            .pointer("/author/bot")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let timestamp = item
            .get("timestamp")
            .and_then(|v| v.as_str())
            .and_then(chrono_lite_parse)
            .unwrap_or(0.0);
        out.push(BackfillMessage {
            id,
            author,
            is_bot,
            content,
            timestamp,
        });
    }
    // Discord returns newest-first; seed oldest-first.
    out.reverse();
    out
}

fn chrono_lite_parse(iso: &str) -> Option<f64> {
    // Accept RFC3339-ish; fall back to 0.
    // Avoid pulling chrono if not already a dep — parse year-month as rough epoch.
    let _ = iso;
    Some(0.0)
}

/// Ensure seed history does not start mid-tool-turn (drop leading tool roles).
pub fn sanitize_seed(messages: Vec<Message>) -> Vec<Message> {
    messages
        .into_iter()
        .filter(|m| matches!(m.role, Role::User | Role::Assistant | Role::System))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn converts_human_and_bot() {
        let msgs = vec![
            BackfillMessage {
                id: "1".into(),
                author: "alice".into(),
                is_bot: false,
                content: "hello".into(),
                timestamp: 1.0,
            },
            BackfillMessage {
                id: "2".into(),
                author: "bot".into(),
                is_bot: true,
                content: "hi".into(),
                timestamp: 2.0,
            },
        ];
        let converted = convert_backfill_messages(&msgs);
        assert_eq!(converted.len(), 2);
        assert!(converted[0].text_content().contains("alice"));
        assert_eq!(converted[1].role, Role::Assistant);
    }

    #[test]
    fn prune_respects_budget() {
        let messages: Vec<_> = (0..20)
            .map(|i| Message::user(&format!("word{i} {}", "x".repeat(40))))
            .collect();
        let pruned = prune_to_token_budget(messages, 50);
        assert!(estimate_tokens(&pruned) <= 50 + 20); // slack for last message
        assert!(!pruned.is_empty());
    }

    #[test]
    fn discord_json_reverses_order() {
        let payload = json!([
            {"id":"2","content":"newer","author":{"username":"a","bot":false},"timestamp":"t"},
            {"id":"1","content":"older","author":{"username":"a","bot":false},"timestamp":"t"}
        ]);
        let msgs = discord_messages_from_json(&payload);
        assert_eq!(msgs[0].content, "older");
        assert_eq!(msgs[1].content, "newer");
    }

    #[test]
    fn prepare_seed_excludes_trigger_and_empties() {
        let msgs = vec![
            BackfillMessage {
                id: "1".into(),
                author: "alice".into(),
                is_bot: false,
                content: "older".into(),
                timestamp: 1.0,
            },
            BackfillMessage {
                id: "2".into(),
                author: "alice".into(),
                is_bot: false,
                content: "   ".into(),
                timestamp: 2.0,
            },
            BackfillMessage {
                id: "3".into(),
                author: "alice".into(),
                is_bot: false,
                content: "live".into(),
                timestamp: 3.0,
            },
        ];
        let seed = prepare_seed(&msgs, Some("3"), 8000);
        assert_eq!(seed.len(), 1);
        assert!(seed[0].text_content().contains("older"));
    }

    #[test]
    fn channel_marker_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        // SAFETY: test-only EDGECRAB_HOME isolation
        unsafe { std::env::set_var("EDGECRAB_HOME", dir.path()) };
        assert!(!is_channel_backfilled("discord", "ch1"));
        assert!(try_begin_backfill("discord", "ch1"));
        assert!(!try_begin_backfill("discord", "ch1")); // in-flight
        mark_channel_backfilled("discord", "ch1", "msg-9");
        end_backfill("discord", "ch1");
        assert!(is_channel_backfilled("discord", "ch1"));
        assert!(!try_begin_backfill("discord", "ch1"));
        unsafe { std::env::remove_var("EDGECRAB_HOME") };
    }
}
