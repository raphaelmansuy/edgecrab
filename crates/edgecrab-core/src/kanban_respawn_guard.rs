//! Respawn guard — Hermes `check_respawn_guard` subset.

use std::sync::LazyLock;

use edgecrab_state::KanbanDb;
use regex::Regex;

/// Guard configuration (from `kanban:` config).
#[derive(Debug, Clone)]
pub struct KanbanRespawnGuardConfig {
    pub rate_limit_cooldown_secs: i64,
    pub success_window_secs: i64,
    pub pr_window_secs: i64,
}

impl KanbanRespawnGuardConfig {
    pub fn from_kanban_config(cfg: &crate::config::KanbanConfig) -> Self {
        Self {
            rate_limit_cooldown_secs: cfg.rate_limit_cooldown_secs as i64,
            success_window_secs: cfg.respawn_guard_success_window_secs as i64,
            pr_window_secs: cfg.respawn_guard_pr_window_secs as i64,
        }
    }
}

static BLOCKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(quota|rate[\s_\-]?limit|429|403|auth\w*|unauthorized|forbidden|billing|subscription|access[\s_]denied|permission[\s_]denied|invalid[\s_]api[\s_]key)\b",
    )
    .expect("respawn blocker regex")
});

static PR_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)https?://github\.com/[^/\s]+/[^/\s]+/pull/\d+").expect("pr url regex")
});

/// Return guard reason if the task should not spawn this tick, else `None`.
pub fn check_respawn_guard(
    db: &KanbanDb,
    task_id: &str,
    cfg: &KanbanRespawnGuardConfig,
) -> Option<String> {
    let task = db.get_task(task_id).ok().flatten()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    if let Ok(Some(run)) = db.latest_ended_run(task_id) {
        if run.outcome.as_deref() == Some("rate_limited") {
            if cfg.rate_limit_cooldown_secs <= 0 {
                return None;
            }
            if let Some(ended) = run.ended_at
                && now - ended < cfg.rate_limit_cooldown_secs
            {
                return Some("rate_limit_cooldown".into());
            }
            return None;
        }
        if run.outcome.as_deref() == Some("completed")
            && cfg.success_window_secs > 0
            && run
                .ended_at
                .is_some_and(|ended| now - ended < cfg.success_window_secs)
        {
            return Some("recent_success".into());
        }
    }

    if let Some(err) = task.last_failure_error.as_deref()
        && BLOCKER_RE.is_match(err)
    {
        return Some("blocker_auth".into());
    }

    if cfg.pr_window_secs > 0 {
        let cutoff = now - cfg.pr_window_secs;
        if let Ok(comments) = db.list_comments(task_id) {
            for c in comments {
                if c.created_at >= cutoff && PR_URL_RE.is_match(&c.body) {
                    return Some("active_pr".into());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_state::KanbanDb;
    use tempfile::TempDir;

    fn cfg() -> KanbanRespawnGuardConfig {
        KanbanRespawnGuardConfig {
            rate_limit_cooldown_secs: 300,
            success_window_secs: 3600,
            pr_window_secs: 86400,
        }
    }

    #[test]
    fn blocker_auth_defers_spawn() {
        let dir = TempDir::new().expect("tmpdir");
        let db = KanbanDb::open_default(Some(dir.path())).expect("open");
        let t = db.create_task("Auth fail", None, 0).expect("create");
        db.claim_task(&t.id, "w1", 900).expect("claim");
        let _ = db.handle_worker_failure(&t.id, "w1", "HTTP 403 forbidden: invalid api key", 5);
        let reason = check_respawn_guard(&db, &t.id, &cfg());
        assert_eq!(reason.as_deref(), Some("blocker_auth"));
    }

    #[test]
    fn rate_limit_cooldown_after_requeue() {
        let dir = TempDir::new().expect("tmpdir");
        let db = KanbanDb::open_default(Some(dir.path())).expect("open");
        let t = db.create_task("RL", None, 0).expect("create");
        db.claim_task(&t.id, "w1", 900).expect("claim");
        db.requeue_rate_limited(&t.id, "w1", "429 rate_limit exceeded")
            .expect("requeue");
        let reason = check_respawn_guard(&db, &t.id, &cfg());
        assert_eq!(reason.as_deref(), Some("rate_limit_cooldown"));
    }
}
