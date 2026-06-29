//! Scheduled curator — deterministic prune on gateway tick (Hermes interval parity, no LLM).

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::archive::{format_prune_report, prune_idle_skills};
use super::backup::maybe_snapshot_before_mutate;
use super::context::SkillsScanContext;
use super::curator::{CuratorSettings, find_prune_candidates};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CuratorState {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub run_count: u64,
    #[serde(default)]
    pub last_run_summary: Option<String>,
}

fn state_path(home: &Path) -> std::path::PathBuf {
    home.join("curator-state.json")
}

pub fn load_curator_state(home: &Path) -> CuratorState {
    let path = state_path(home);
    if !path.is_file() {
        return CuratorState::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_curator_state(home: &Path, state: &CuratorState) {
    let path = state_path(home);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(state) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn set_curator_paused(home: &Path, paused: bool) {
    let mut state = load_curator_state(home);
    state.paused = paused;
    save_curator_state(home, &state);
}

fn parse_iso(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Runtime gates for scheduled passes (mirrors Hermes `should_run_now` defer-on-first-seed).
pub fn should_run_scheduled_curator(home: &Path, enabled: bool, interval_hours: u32) -> bool {
    if !enabled {
        return false;
    }
    let state = load_curator_state(home);
    if state.paused {
        return false;
    }
    let Some(last) = state.last_run_at.as_deref().and_then(parse_iso) else {
        return false;
    };
    let elapsed = Utc::now().signed_duration_since(last);
    elapsed.num_hours() >= i64::from(interval_hours.max(1))
}

/// Seed first-run deferral when curator is enabled but never run (Hermes parity).
pub fn seed_curator_deferred_if_needed(home: &Path, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    let mut state = load_curator_state(home);
    if state.last_run_at.is_some() {
        return None;
    }
    state.last_run_at = Some(Utc::now().to_rfc3339());
    state.last_run_summary = Some(
        "deferred first run — will prune after one interval; use /curator prune --dry-run to preview"
            .into(),
    );
    save_curator_state(home, &state);
    state.last_run_summary
}

/// Run deterministic prune (no LLM). Returns summary when a pass ran.
pub fn run_scheduled_curator_pass(
    home: &Path,
    ctx: &SkillsScanContext,
    settings: CuratorSettings,
    dry_run: bool,
) -> String {
    let candidates = find_prune_candidates(ctx, settings.archive_after_days);
    let report = prune_idle_skills(home, &candidates, dry_run, settings.prune_builtins);
    format_prune_report(&report, settings.archive_after_days)
}

/// Gateway/CLI hook — run scheduled prune if gates pass. Never panics.
pub fn maybe_run_scheduled_curator(
    home: &Path,
    ctx: &SkillsScanContext,
    settings: CuratorSettings,
    enabled: bool,
    interval_hours: u32,
) -> Option<String> {
    if !enabled {
        return None;
    }
    if load_curator_state(home).paused {
        return None;
    }
    if load_curator_state(home).last_run_at.is_none() {
        seed_curator_deferred_if_needed(home, true);
        return None;
    }
    if !should_run_scheduled_curator(home, enabled, interval_hours) {
        return None;
    }

    if let Some(note) = maybe_snapshot_before_mutate(
        home,
        settings.backup_enabled,
        settings.backup_keep,
        "pre-curator-scheduled",
    ) {
        tracing::info!("curator backup: {note}");
    }

    let summary = run_scheduled_curator_pass(home, ctx, settings, false);
    let mut state = load_curator_state(home);
    state.last_run_at = Some(Utc::now().to_rfc3339());
    state.run_count = state.run_count.saturating_add(1);
    state.last_run_summary = Some(summary.clone());
    save_curator_state(home, &state);
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_seeds_defer() {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        assert!(seed_curator_deferred_if_needed(dir.path(), true).is_some());
        assert!(load_curator_state(dir.path()).last_run_at.is_some());
        assert!(!should_run_scheduled_curator(dir.path(), true, 168));
    }
}
