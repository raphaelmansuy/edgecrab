//! Trajectory types for session recording and RL training.
//!
//! Trajectories capture the full conversation history including tool
//! calls, token usage, and metadata for offline analysis and training.

use serde::{Deserialize, Serialize};

use crate::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    pub session_id: String,
    pub model: String,
    pub timestamp: String,
    pub messages: Vec<Message>,
    pub metadata: TrajectoryMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryMetadata {
    pub task_id: Option<String>,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub api_calls: u32,
    pub tools_used: Vec<String>,
    pub completed: bool,
    pub duration_seconds: f64,
}

/// Extract thinking blocks from assistant content.
///
/// Returns (cleaned_content, optional_reasoning).
/// Handles `<think>…</think>` tags used by reasoning models.
pub fn extract_reasoning(content: &str) -> (String, Option<String>) {
    let re = regex::Regex::new(r"(?s)<think>(.*?)</think>").expect("valid regex");
    let reasoning = re.captures(content).map(|c| c[1].trim().to_string());
    let cleaned = re.replace_all(content, "").trim().to_string();
    (cleaned, reasoning)
}

/// Check if content after think block is empty (thinking exhaustion).
pub fn has_content_after_think(content: &str) -> bool {
    let re = regex::Regex::new(r"(?s)<think>.*?</think>").expect("valid regex");
    let after = re.replace_all(content, "").trim().to_string();
    !after.is_empty()
}

/// Convert legacy `<REASONING_SCRATCHPAD>` tags to `<think>` tags.
///
/// Both tag formats are supported by reasoning models.
pub fn convert_scratchpad_to_think(content: &str) -> String {
    content
        .replace("<REASONING_SCRATCHPAD>", "<think>")
        .replace("</REASONING_SCRATCHPAD>", "</think>")
}

/// Check if content has an opening scratchpad tag without a closing one.
pub fn has_incomplete_scratchpad(content: &str) -> bool {
    content.contains("<REASONING_SCRATCHPAD>") && !content.contains("</REASONING_SCRATCHPAD>")
}

/// Save trajectory as JSONL (append).
pub fn save_trajectory(path: &std::path::Path, trajectory: &Trajectory) -> std::io::Result<()> {
    use std::io::Write;
    let json = serde_json::to_string(trajectory)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{json}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_reasoning_with_think_tags() {
        let content = "<think>Let me figure this out...</think>The answer is 42.";
        let (cleaned, reasoning) = extract_reasoning(content);
        assert_eq!(cleaned, "The answer is 42.");
        assert_eq!(reasoning.as_deref(), Some("Let me figure this out..."));
    }

    #[test]
    fn extract_reasoning_no_tags() {
        let content = "Just a normal response.";
        let (cleaned, reasoning) = extract_reasoning(content);
        assert_eq!(cleaned, "Just a normal response.");
        assert!(reasoning.is_none());
    }

    #[test]
    fn has_content_after_think_true() {
        assert!(has_content_after_think(
            "<think>thinking...</think>Real content here."
        ));
    }

    #[test]
    fn has_content_after_think_false() {
        assert!(!has_content_after_think("<think>only thinking</think>"));
    }

    #[test]
    fn convert_scratchpad() {
        let input = "<REASONING_SCRATCHPAD>stuff</REASONING_SCRATCHPAD>";
        assert_eq!(convert_scratchpad_to_think(input), "<think>stuff</think>");
    }

    #[test]
    fn incomplete_scratchpad() {
        assert!(has_incomplete_scratchpad(
            "<REASONING_SCRATCHPAD>partial reasoning"
        ));
        assert!(!has_incomplete_scratchpad(
            "<REASONING_SCRATCHPAD>complete</REASONING_SCRATCHPAD>"
        ));
    }

    #[test]
    fn trajectory_roundtrip() {
        let traj = Trajectory {
            session_id: "s1".into(),
            model: "test-model".into(),
            timestamp: "2026-03-28T00:00:00Z".into(),
            messages: vec![Message::user("hello"), Message::assistant("hi")],
            metadata: TrajectoryMetadata {
                task_id: Some("t1".into()),
                total_tokens: 100,
                total_cost: 0.001,
                api_calls: 1,
                tools_used: vec!["read_file".into()],
                completed: true,
                duration_seconds: 2.5,
            },
        };
        let json = serde_json::to_string(&traj).expect("serialize");
        let deser: Trajectory = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deser.session_id, "s1");
        assert_eq!(deser.messages.len(), 2);
    }
}
