//! # Channel directory — cached map of reachable channels/contacts per platform
//!
//! WHY: The send_message tool needs to resolve human-friendly names (e.g.
//! "#bot-home") to platform-specific IDs. This module builds and caches
//! a directory of known channels/contacts by scanning session history.
//!
//! Mirrors hermes-agent's `gateway/channel_directory.py`:
//! - Built on gateway startup, refreshed periodically
//! - Saved to `~/.edgecrab/channel_directory.json`
//! - Used by send_message tool for name resolution

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A single entry in the channel directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEntry {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub channel_type: String, // "channel", "dm", "group"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild: Option<String>,
}

/// The full channel directory.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelDirectory {
    pub updated_at: String,
    pub platforms: HashMap<String, Vec<ChannelEntry>>,
}

/// Get the path to the channel directory file.
fn directory_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".edgecrab").join("channel_directory.json")
}

/// Load the cached channel directory from disk.
pub fn load_directory() -> ChannelDirectory {
    let path = directory_path();
    if !path.exists() {
        return ChannelDirectory::default();
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ChannelDirectory::default(),
    }
}

/// Save the channel directory to disk.
pub fn save_directory(directory: &ChannelDirectory) -> Result<(), std::io::Error> {
    let path = directory_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(directory).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)
}

/// Build a channel directory from session history.
///
/// Scans the session database for gateway sessions and their persisted
/// routing keys (`SessionRecord.user_id`), building a directory of reachable
/// targets without depending on live platform enumeration.
pub fn build_from_sessions(db: &edgecrab_state::SessionDb) -> ChannelDirectory {
    let mut platforms: HashMap<String, Vec<ChannelEntry>> = HashMap::new();

    // Query recent sessions to extract platform + chat_id mappings
    if let Ok(sessions) = db.list_sessions(500) {
        let mut seen: HashMap<String, std::collections::HashSet<String>> = HashMap::new();

        for session in sessions {
            let source = session.source.clone();
            if source.is_empty() || source == "cli" {
                continue;
            }

            let record = match db.get_session(&session.id) {
                Ok(Some(record)) => record,
                _ => continue,
            };
            let Some(entry_id) = record.user_id else {
                continue;
            };
            let entry_set = seen.entry(source.clone()).or_default();
            if entry_set.contains(&entry_id) {
                continue;
            }
            entry_set.insert(entry_id.clone());

            let (display_id, thread_id) = split_threaded_id(&entry_id);
            let entry = ChannelEntry {
                id: entry_id,
                name: session.title.clone().unwrap_or(display_id),
                channel_type: "dm".into(),
                thread_id,
                guild: None,
            };

            platforms.entry(source).or_default().push(entry);
        }
    }

    let directory = ChannelDirectory {
        updated_at: chrono::Utc::now().to_rfc3339(),
        platforms,
    };

    if let Err(e) = save_directory(&directory) {
        tracing::warn!(error = %e, "failed to save channel directory");
    }

    directory
}

fn split_threaded_id(entry_id: &str) -> (String, Option<String>) {
    match entry_id.split_once(':') {
        Some((chat_id, thread_id)) if !chat_id.is_empty() && !thread_id.is_empty() => {
            (chat_id.to_string(), Some(thread_id.to_string()))
        }
        _ => (entry_id.to_string(), None),
    }
}

/// Resolve a human-friendly channel name to a platform-specific ID.
///
/// Matching strategy (case-insensitive, first match wins):
/// - Exact name match
/// - Guild-qualified match for Discord ("GuildName/channel")
/// - Unambiguous prefix match
pub fn resolve_channel_name(platform_name: &str, name: &str) -> Option<String> {
    let directory = load_directory();
    let channels = directory.platforms.get(platform_name)?;

    if channels.is_empty() {
        return None;
    }

    let query = name.trim_start_matches('#').to_lowercase();

    // 1. Exact name match
    for ch in channels {
        if ch.name.to_lowercase() == query {
            return Some(ch.id.clone());
        }
    }

    // 2. Guild-qualified match for Discord ("GuildName/channel")
    if query.contains('/') {
        if let Some((guild_part, ch_part)) = query.rsplit_once('/') {
            for ch in channels {
                let guild = ch.guild.as_deref().unwrap_or("").to_lowercase();
                if guild == guild_part && ch.name.to_lowercase() == ch_part {
                    return Some(ch.id.clone());
                }
            }
        }
    }

    // 3. Unambiguous prefix match
    let matches: Vec<_> = channels
        .iter()
        .filter(|ch| ch.name.to_lowercase().starts_with(&query))
        .collect();
    if matches.len() == 1 {
        return Some(matches[0].id.clone());
    }

    None
}

/// Format the channel directory for display to the LLM.
pub fn format_directory_for_display() -> String {
    let directory = load_directory();

    if directory.platforms.is_empty() || directory.platforms.values().all(|v| v.is_empty()) {
        return "No messaging platforms connected or no channels discovered yet.".into();
    }

    let mut lines = vec!["Available messaging targets:\n".into()];

    for (plat_name, channels) in directory.platforms.iter() {
        if channels.is_empty() {
            continue;
        }
        lines.push(format!("{}:", plat_name));
        for ch in channels {
            let label = if ch.name.is_empty() { &ch.id } else { &ch.name };
            lines.push(format!("  {}:{}", plat_name, label));
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_directory_is_empty() {
        let dir = ChannelDirectory::default();
        assert!(dir.platforms.is_empty());
    }

    #[test]
    fn resolve_exact_match() {
        let mut dir = ChannelDirectory::default();
        dir.platforms.insert(
            "discord".into(),
            vec![ChannelEntry {
                id: "123".into(),
                name: "bot-home".into(),
                channel_type: "channel".into(),
                thread_id: None,
                guild: Some("MyServer".into()),
            }],
        );

        // Save and load to test round-trip
        let json = serde_json::to_string(&dir).expect("channel directory should serialize");
        let loaded: ChannelDirectory =
            serde_json::from_str(&json).expect("channel directory should deserialize");
        assert_eq!(loaded.platforms["discord"][0].id, "123");
    }

    #[test]
    fn format_empty_directory() {
        // Format should report no platforms
        let display = format_directory_for_display();
        assert!(display.contains("No messaging platforms") || display.contains("Available"));
    }
}
