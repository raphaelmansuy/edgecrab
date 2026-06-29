//! Skill curator — stale detection, archive/restore, deterministic prune (dry-run).

use std::path::Path;

use chrono::{DateTime, Utc};

use super::archive::{
    PruneCandidate, archive_skill, format_archived_list, format_prune_report, list_archived,
    prune_idle_skills, restore_skill,
};
use super::backup::{format_backup_list, maybe_snapshot_before_mutate, rollback_snapshot};
use super::context::SkillsScanContext;
use super::discovery::scan_skill_commands;
use super::scheduler::{load_curator_state, run_scheduled_curator_pass, set_curator_paused};
use super::usage::{self, last_activity_at};

#[derive(Debug, Clone, Copy)]
pub struct CuratorSettings {
    pub stale_after_days: u32,
    pub archive_after_days: u32,
    pub prune_builtins: bool,
    pub backup_enabled: bool,
    pub backup_keep: u32,
}

impl Default for CuratorSettings {
    fn default() -> Self {
        Self {
            stale_after_days: 30,
            archive_after_days: 90,
            prune_builtins: false,
            backup_enabled: true,
            backup_keep: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    NeverUsed,
    NotUsedSince { days: i64 },
}

#[derive(Debug, Clone)]
pub struct StaleSkill {
    pub name: String,
    pub slug: String,
    pub reason: StaleReason,
}

fn parse_last_used(iso: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn idle_days_since(rec: &usage::UsageRecord) -> Option<i64> {
    let iso = last_activity_at(rec)?;
    let last = parse_last_used(iso)?;
    Some(Utc::now().signed_duration_since(last).num_days())
}

/// Installed skills with no recent activity (slash, view, or patch).
pub fn find_stale_skills(ctx: &SkillsScanContext, stale_after_days: u32) -> Vec<StaleSkill> {
    let commands = scan_skill_commands(ctx);
    let store = usage::read_store_public(ctx.edgecrab_home.as_path());
    let threshold = chrono::Duration::days(i64::from(stale_after_days.max(1)));
    let now = Utc::now();

    let mut stale = Vec::new();
    for (slug, info) in commands {
        let record = store.get(&info.name);
        if record.is_some_and(|r| r.pinned) {
            continue;
        }
        let reason = match record.and_then(|r| last_activity_at(r)) {
            None => StaleReason::NeverUsed,
            Some(iso) => {
                let Some(last) = parse_last_used(iso) else {
                    stale.push(StaleSkill {
                        name: info.name.clone(),
                        slug: slug.clone(),
                        reason: StaleReason::NeverUsed,
                    });
                    continue;
                };
                let age = now.signed_duration_since(last);
                if age > threshold {
                    StaleReason::NotUsedSince {
                        days: age.num_days(),
                    }
                } else {
                    continue;
                }
            }
        };
        stale.push(StaleSkill {
            name: info.name,
            slug,
            reason,
        });
    }
    stale.sort_by(|a, b| a.name.cmp(&b.name));
    stale
}

/// Skills idle longer than `archive_after_days` (candidates for `/curator prune`).
pub fn find_prune_candidates(
    ctx: &SkillsScanContext,
    archive_after_days: u32,
) -> Vec<PruneCandidate> {
    let commands = scan_skill_commands(ctx);
    let store = usage::read_store_public(ctx.edgecrab_home.as_path());
    let threshold = i64::from(archive_after_days.max(1));
    let mut out = Vec::new();
    for (slug, info) in commands {
        let Some(rec) = store.get(&info.name) else {
            out.push(PruneCandidate {
                name: info.name.clone(),
                slug: slug.clone(),
                idle_days: i64::MAX,
            });
            continue;
        };
        if rec.pinned {
            continue;
        }
        let idle = idle_days_since(rec).unwrap_or(i64::MAX);
        if idle >= threshold {
            out.push(PruneCandidate {
                name: info.name,
                slug,
                idle_days: idle,
            });
        }
    }
    out.sort_by(|a, b| {
        b.idle_days
            .cmp(&a.idle_days)
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

pub fn format_curator_status(
    ctx: &SkillsScanContext,
    home: &Path,
    settings: CuratorSettings,
) -> String {
    let installed = scan_skill_commands(ctx).len();
    let stale = find_stale_skills(ctx, settings.stale_after_days);
    let archived = list_archived(home).len();
    let state = load_curator_state(home);
    let paused = if state.paused { "yes" } else { "no" };
    let last_run = state.last_run_at.as_deref().unwrap_or("never");
    let usage_lines = usage::format_usage_summary(home, 5);
    format!(
        "Skill curator\n\
         Installed slash skills: {installed}\n\
         Archived: {archived}\n\
         Paused: {paused}  Last scheduled run: {last_run}\n\
         Stale threshold: {} days idle\n\
         Archive threshold: {} days idle\n\
         Prune bundled skills: {}\n\
         Stale skills: {}\n\
         \n\
         {usage_lines}\n\
         \n\
         Commands:\n\
           /curator stale [days]\n\
           /curator prune [--dry-run]\n\
           /curator pause | resume\n\
           /curator archive <name>  /curator restore <name>\n\
           /curator list-archived\n\
           /curator backups | rollback <id>\n\
           /skills usage  /skills pin <name>",
        settings.stale_after_days,
        settings.archive_after_days,
        settings.prune_builtins,
        stale.len()
    )
}

pub fn format_stale_report(ctx: &SkillsScanContext, stale_after_days: u32) -> String {
    let stale = find_stale_skills(ctx, stale_after_days);
    if stale.is_empty() {
        return format!("No stale skills (threshold: {stale_after_days} days without activity).");
    }
    let mut lines = vec![format!(
        "Stale skills ({} installed, threshold {} days):",
        scan_skill_commands(ctx).len(),
        stale_after_days
    )];
    for skill in stale {
        let detail = match skill.reason {
            StaleReason::NeverUsed => "never invoked".into(),
            StaleReason::NotUsedSince { days } => format!("last activity {days} days ago"),
        };
        lines.push(format!("  /{} ({}) — {detail}", skill.slug, skill.name));
    }
    lines.push(String::new());
    lines.push(
        "Pin: /skills pin <name>  Archive: /curator archive <name>  Bulk: /curator prune --dry-run"
            .into(),
    );
    lines.join("\n")
}

/// Handle `/curator` subcommands (CLI + gateway).
pub fn handle_curator_subcommand(
    ctx: &SkillsScanContext,
    home: &Path,
    args: &str,
    settings: CuratorSettings,
) -> String {
    let tokens: Vec<&str> = args.split_whitespace().collect();
    let first = tokens
        .first()
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    let dry_run = tokens.iter().any(|t| *t == "--dry-run" || *t == "-n");
    match first.as_str() {
        "" | "status" => format_curator_status(ctx, home, settings),
        "stale" | "report" | "review" => {
            let days = tokens
                .get(1)
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(settings.stale_after_days);
            format_stale_report(ctx, days)
        }
        "list-archived" | "archived" | "archive-list" => format_archived_list(home),
        "archive" => {
            let Some(name) = tokens.get(1) else {
                return "Usage: /curator archive <skill-name>".into();
            };
            match archive_skill(home, name, settings.prune_builtins) {
                Ok(msg) => msg,
                Err(e) => format!("Archive failed: {e}"),
            }
        }
        "restore" | "unarchive" => {
            let Some(name) = tokens.get(1) else {
                return "Usage: /curator restore <skill-name>".into();
            };
            match restore_skill(home, name) {
                Ok(msg) => msg,
                Err(e) => format!("Restore failed: {e}"),
            }
        }
        "prune" | "run" => {
            let days = tokens
                .iter()
                .find_map(|t| t.strip_prefix("--days=").and_then(|d| d.parse().ok()))
                .unwrap_or(settings.archive_after_days);
            let mut run_settings = settings;
            run_settings.archive_after_days = days;
            if dry_run {
                let candidates = find_prune_candidates(ctx, days);
                let report = prune_idle_skills(home, &candidates, true, settings.prune_builtins);
                format_prune_report(&report, days)
            } else {
                let backup_note = maybe_snapshot_before_mutate(
                    home,
                    settings.backup_enabled,
                    settings.backup_keep,
                    "pre-curator-prune",
                );
                let summary = run_scheduled_curator_pass(home, ctx, run_settings, false);
                let mut state = load_curator_state(home);
                state.last_run_at = Some(chrono::Utc::now().to_rfc3339());
                state.run_count = state.run_count.saturating_add(1);
                state.last_run_summary = Some(summary.clone());
                super::scheduler::save_curator_state(home, &state);
                if let Some(note) = backup_note {
                    format!("{note}\n\n{summary}")
                } else {
                    summary
                }
            }
        }
        "backups" | "backup" | "snapshots" => format_backup_list(home),
        "rollback" | "restore-backup" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /curator rollback <backup-id>".into();
            };
            match rollback_snapshot(home, id) {
                Ok(msg) => msg,
                Err(e) => format!("Rollback failed: {e}"),
            }
        }
        "pause" => {
            set_curator_paused(home, true);
            "Curator paused (scheduled passes suppressed).".into()
        }
        "resume" => {
            set_curator_paused(home, false);
            "Curator resumed.".into()
        }
        "help" => "Skill curator commands:\n\
             /curator status — usage + stale/archived counts\n\
             /curator stale [days] — list idle skills\n\
             /curator prune [--dry-run] [--days=N] — archive idle skills\n\
             /curator pause | resume — gate scheduled gateway passes\n\
             /curator archive <name> — move one skill to .archive/\n\
             /curator restore <name> — recover from .archive/\n\
             /curator list-archived — list archived skills\n\
             /curator backups — list pre-run snapshots\n\
             /curator rollback <id> — restore from snapshot\n\
             /skills pin|unpin <name> — exclude from stale/prune"
            .into(),
        other => format!("Unknown /curator subcommand '{other}'. Try: /curator help"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(home: &std::path::Path, slug: &str, name: &str) {
        let dir = home.join("skills").join(slug);
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n"),
        )
        .expect("write");
    }

    #[test]
    fn never_used_skill_is_stale() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "alpha", "Alpha Skill");
        let ctx = SkillsScanContext::from_home(dir.path());
        let stale = find_stale_skills(&ctx, 30);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].reason, StaleReason::NeverUsed);
    }

    #[test]
    fn recently_used_skill_not_stale() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "beta", "Beta Skill");
        usage::bump_use(dir.path(), "Beta Skill");
        let ctx = SkillsScanContext::from_home(dir.path());
        assert!(find_stale_skills(&ctx, 30).is_empty());
    }

    #[test]
    fn view_activity_prevents_stale() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "viewed", "Viewed Skill");
        usage::bump_view(dir.path(), "Viewed Skill");
        let ctx = SkillsScanContext::from_home(dir.path());
        assert!(find_stale_skills(&ctx, 30).is_empty());
    }

    #[test]
    fn pinned_skill_skipped_by_curator() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "gamma", "Gamma Skill");
        usage::set_pinned(dir.path(), "Gamma Skill", true);
        let ctx = SkillsScanContext::from_home(dir.path());
        assert!(find_stale_skills(&ctx, 30).is_empty());
    }
}
