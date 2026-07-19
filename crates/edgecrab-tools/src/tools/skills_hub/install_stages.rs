//! Shared install pipeline stage vocabulary (CLI + TUI theatre).
//!
//! DRY: one enum / one formatter for human and JSON storytelling.

use serde::Serialize;

/// Install pipeline stages: Fetch → Quarantine → Scan → Gate → Commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStage {
    Fetch,
    Quarantine,
    Scan,
    Gate,
    Commit,
}

impl InstallStage {
    pub const ALL: [InstallStage; 5] = [
        InstallStage::Fetch,
        InstallStage::Quarantine,
        InstallStage::Scan,
        InstallStage::Gate,
        InstallStage::Commit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fetch => "Fetch",
            Self::Quarantine => "Quarantine",
            Self::Scan => "Scan",
            Self::Gate => "Gate",
            Self::Commit => "Commit",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Fetch => 0,
            Self::Quarantine => 1,
            Self::Scan => 2,
            Self::Gate => 3,
            Self::Commit => 4,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Quarantine => "quarantine",
            Self::Scan => "scan",
            Self::Gate => "gate",
            Self::Commit => "commit",
        }
    }
}

/// Human theatre line, e.g. `[✓] Fetch  [→] Quarantine  [ ] Scan …`
pub fn format_stages_human(current: Option<InstallStage>) -> String {
    let cur_idx = current.map(InstallStage::index);
    InstallStage::ALL
        .iter()
        .map(|stage| {
            let marker = match cur_idx {
                Some(i) if stage.index() < i => "✓",
                Some(i) if stage.index() == i => "→",
                _ => " ",
            };
            format!("[{marker}] {}", stage.label())
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Machine-readable install progress / outcome.
#[derive(Debug, Clone, Serialize)]
pub struct InstallStageReport {
    pub stages: Vec<&'static str>,
    pub current: Option<&'static str>,
    pub skill: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl InstallStageReport {
    pub fn in_progress(skill: &str, current: InstallStage) -> Self {
        Self {
            stages: InstallStage::ALL.iter().map(|s| s.as_str()).collect(),
            current: Some(current.as_str()),
            skill: skill.to_string(),
            status: "in_progress".into(),
            message: None,
        }
    }

    pub fn done(skill: &str, message: &str) -> Self {
        Self {
            stages: InstallStage::ALL.iter().map(|s| s.as_str()).collect(),
            current: Some(InstallStage::Commit.as_str()),
            skill: skill.to_string(),
            status: "ok".into(),
            message: Some(message.to_string()),
        }
    }

    pub fn failed(skill: &str, message: &str) -> Self {
        Self {
            stages: InstallStage::ALL.iter().map(|s| s.as_str()).collect(),
            current: None,
            skill: skill.to_string(),
            status: "error".into(),
            message: Some(message.to_string()),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            format!(
                r#"{{"status":"error","skill":"{}","message":"serialize failed"}}"#,
                self.skill
            )
        })
    }
}

pub fn format_stages_json(
    skill: &str,
    current: Option<InstallStage>,
    status: &str,
    message: Option<&str>,
) -> String {
    let report = InstallStageReport {
        stages: InstallStage::ALL.iter().map(|s| s.as_str()).collect(),
        current: current.map(InstallStage::as_str),
        skill: skill.to_string(),
        status: status.into(),
        message: message.map(str::to_string),
    };
    report.to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_marks_current_stage() {
        let line = format_stages_human(Some(InstallStage::Scan));
        assert!(line.contains("[→] Scan"));
        assert!(line.contains("[✓] Fetch"));
        assert!(line.contains("[ ] Gate"));
    }

    #[test]
    fn json_round_trip_fields() {
        let json = format_stages_json("demo", Some(InstallStage::Gate), "in_progress", None);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["skill"], "demo");
        assert_eq!(v["current"], "gate");
        assert_eq!(v["status"], "in_progress");
    }
}
