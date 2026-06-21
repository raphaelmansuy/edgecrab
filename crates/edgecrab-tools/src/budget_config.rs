//! Per-tool spill thresholds — Hermes `budget_config.PINNED_THRESHOLDS` parity (spec 015 P2.5).
//!
//! `read_file` is pinned out of **turn-budget** spill to prevent
//! persist → read artifact → re-spill loops. Proactive per-result spill still applies.

use crate::artifact_spill::SpillConfig;

/// Tools never candidates for `enforce_turn_budget` spill (layer 3).
pub const TURN_BUDGET_SPILL_EXEMPT: &[&str] = &["read_file", "computer_use"];

/// Whether this tool may be spilled by aggregate turn-budget enforcement.
pub fn exempt_from_turn_budget_spill(tool_name: &str) -> bool {
    TURN_BUDGET_SPILL_EXEMPT.contains(&tool_name)
}

/// Effective byte threshold for proactive per-result spill (layer 2).
pub fn resolve_proactive_spill_threshold(tool_name: &str, config: &SpillConfig) -> usize {
    if !config.enabled {
        return usize::MAX;
    }
    // Artifact reads are already compact stubs — never re-spill.
    if tool_name == "read_file" {
        return config.threshold;
    }
    config.threshold
}

/// Paths under session artifact dirs should not be re-spilled on read.
pub fn is_artifact_spill_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains(".edgecrab-artifacts/") || lower.contains(".edgecrab-artifacts\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact_spill::SpillConfig;

    #[test]
    fn ha24_read_file_exempt_from_turn_budget_spill() {
        assert!(exempt_from_turn_budget_spill("read_file"));
        assert!(!exempt_from_turn_budget_spill("terminal"));
    }

    #[test]
    fn artifact_path_detected() {
        assert!(is_artifact_spill_path(
            ".edgecrab-artifacts/ses1/read_file_001.md"
        ));
        assert!(!is_artifact_spill_path("src/main.rs"));
    }

    #[test]
    fn proactive_threshold_disabled_when_spill_off() {
        let config = SpillConfig {
            enabled: false,
            threshold: 100,
            preview_lines: 10,
        };
        assert_eq!(
            resolve_proactive_spill_threshold("terminal", &config),
            usize::MAX
        );
    }
}
