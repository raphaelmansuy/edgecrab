//! Kanban dispatcher tick — Hermes `dispatch_once` subset for EdgeCrab.

use edgecrab_state::KanbanDb;
use edgecrab_types::AgentError;

use crate::kanban_profiles::{install_root_from, profile_exists};

/// Worker spawn request emitted by the dispatcher when a task is claimed.
#[derive(Debug, Clone)]
pub struct KanbanSpawnRequest {
    pub task_id: String,
    pub title: String,
    pub body: Option<String>,
    pub worker_id: String,
    pub assignee: String,
    /// Hermes `build_worker_context` — injected into worker prompt.
    pub worker_context: String,
}

/// One dispatcher tick outcome.
#[derive(Debug, Clone, Default)]
pub struct KanbanDispatchResult {
    pub reclaimed: usize,
    pub spawned: usize,
    pub skipped_at_capacity: bool,
    pub skipped_unassigned: usize,
    pub skipped_nonspawnable: usize,
    pub skipped_per_profile_capped: usize,
    pub respawn_guarded: usize,
    pub promoted: usize,
    pub timed_out: usize,
}

/// Dispatcher configuration (from `kanban:` config).
#[derive(Debug, Clone)]
pub struct KanbanDispatchConfig {
    pub claim_ttl_secs: i64,
    pub max_workers: u32,
    pub max_in_progress_per_profile: Option<u32>,
    pub failure_limit: u32,
    pub default_assignee: String,
    pub install_root: std::path::PathBuf,
    pub respawn_guard: crate::kanban_respawn_guard::KanbanRespawnGuardConfig,
}

impl KanbanDispatchConfig {
    pub fn from_kanban_config(cfg: &crate::config::KanbanConfig) -> Self {
        let root = install_root_from(crate::edgecrab_home());
        let per_profile = cfg.max_in_progress_per_profile.filter(|&n| n >= 1);
        Self {
            claim_ttl_secs: cfg.claim_ttl_secs.clamp(60, 86_400) as i64,
            max_workers: cfg.max_workers.max(1),
            max_in_progress_per_profile: per_profile,
            failure_limit: cfg.failure_limit.max(1),
            default_assignee: crate::kanban_profiles::resolve_default_assignee(cfg, &root),
            install_root: root,
            respawn_guard: crate::kanban_respawn_guard::KanbanRespawnGuardConfig::from_kanban_config(cfg),
        }
    }
}

fn task_assignee(task: &edgecrab_state::KanbanTask, cfg: &KanbanDispatchConfig) -> Option<String> {
    task
        .assignee
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            if cfg.default_assignee.is_empty() {
                None
            } else {
                Some(cfg.default_assignee.clone())
            }
        })
}

/// Run one dispatcher tick: reclaim stale claims, then spawn workers for claimable tasks.
pub fn dispatch_once(
    db: &KanbanDb,
    cfg: &KanbanDispatchConfig,
    mut spawn: impl FnMut(KanbanSpawnRequest) -> bool,
) -> Result<KanbanDispatchResult, AgentError> {
    let reclaimed = db.reclaim_stale_claims_with_limit(cfg.failure_limit)?;
    let timed_out = db.enforce_max_runtime_with(cfg.failure_limit, |task_id| {
        crate::kanban_workers::cancel_worker(task_id);
    })?;
    let promoted = db.recompute_ready(cfg.failure_limit)?;
    let mut result = KanbanDispatchResult {
        reclaimed,
        timed_out,
        promoted,
        ..Default::default()
    };

    let doing = db.count_doing_tasks()?;
    if doing >= cfg.max_workers as usize {
        result.skipped_at_capacity = true;
        return Ok(result);
    }

    let slots = cfg.max_workers as usize - doing;
    let candidates = db.list_claimable_tasks(slots)?;
    let mut per_profile_running = db.count_doing_by_assignee()?;
    let per_profile_cap = cfg
        .max_in_progress_per_profile
        .map(|n| n as usize);

    for task in candidates {
        if db.count_doing_tasks()? >= cfg.max_workers as usize {
            result.skipped_at_capacity = true;
            break;
        }

        let mut assignee = match task_assignee(&task, cfg) {
            Some(a) => a,
            None => {
                result.skipped_unassigned += 1;
                continue;
            }
        };

        if let Some(cap) = per_profile_cap {
            let current = per_profile_running.get(&assignee).copied().unwrap_or(0);
            if current >= cap {
                result.skipped_per_profile_capped += 1;
                continue;
            }
        }

        if task.assignee.as_deref().map(str::trim).filter(|s| !s.is_empty()).is_none()
            && !cfg.default_assignee.is_empty()
        {
            let _ = db.apply_default_assignee(&task.id, &cfg.default_assignee);
            assignee = cfg.default_assignee.clone();
        }

        if !profile_exists(&cfg.install_root, &assignee) {
            result.skipped_nonspawnable += 1;
            continue;
        }

        if crate::kanban_respawn_guard::check_respawn_guard(db, &task.id, &cfg.respawn_guard).is_some() {
            result.respawn_guarded += 1;
            continue;
        }

        let worker_id = format!("dispatcher-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        if db
            .claim_task(&task.id, &worker_id, cfg.claim_ttl_secs)
            .is_err()
        {
            continue;
        }
        let worker_context = edgecrab_state::build_worker_context(db, &task.id)
            .unwrap_or_else(|_| {
                format!(
                    "# Kanban task {}: {}\n",
                    task.id,
                    task.title
                )
            });
        let ok = spawn(KanbanSpawnRequest {
            task_id: task.id.clone(),
            title: task.title.clone(),
            body: task.body.clone(),
            worker_id: worker_id.clone(),
            assignee: assignee.clone(),
            worker_context,
        });
        if !ok {
            let _ = db.release_task(&task.id, Some(&worker_id));
            continue;
        }
        *per_profile_running.entry(assignee.clone()).or_insert(0) += 1;
        result.spawned += 1;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use edgecrab_state::KanbanDb;
    use tempfile::TempDir;

    fn test_db() -> (TempDir, std::sync::Arc<KanbanDb>) {
        let dir = TempDir::new().expect("tmpdir");
        let db = KanbanDb::open_default(Some(dir.path())).expect("open");
        (dir, db)
    }

    fn test_cfg(root: &Path) -> KanbanDispatchConfig {
        KanbanDispatchConfig {
            claim_ttl_secs: 900,
            max_workers: 2,
            max_in_progress_per_profile: None,
            failure_limit: 2,
            default_assignee: "default".into(),
            install_root: root.to_path_buf(),
            respawn_guard: crate::kanban_respawn_guard::KanbanRespawnGuardConfig {
                rate_limit_cooldown_secs: 300,
                success_window_secs: 3600,
                pr_window_secs: 86_400,
            },
        }
    }

    #[test]
    fn dispatch_respects_parent_dependency() {
        let (dir, db) = test_db();
        let cfg = test_cfg(dir.path());
        let parent = db
            .create_task_with_assignee("Parent", None, 0, None, Some("default"))
            .expect("parent");
        let child = db
            .create_task_with_assignee("Child", None, 0, None, Some("default"))
            .expect("child");
        db.link_tasks(&parent.id, &child.id).expect("link");

        let mut spawned = Vec::new();
        let result = dispatch_once(&db, &cfg, |req| {
            spawned.push(req.task_id);
            true
        })
        .expect("dispatch");
        assert_eq!(result.spawned, 1);
        assert_eq!(spawned, vec![parent.id.clone()]);

        db.complete_task(&parent.id, None, Some("done"))
            .expect("complete parent");
        let result2 = dispatch_once(&db, &cfg, |req| {
            spawned.push(req.task_id);
            true
        })
        .expect("dispatch2");
        assert_eq!(result2.spawned, 1);
        assert!(spawned.contains(&child.id));
    }

    #[test]
    fn dispatch_skips_unknown_assignee() {
        let (dir, db) = test_db();
        let cfg = test_cfg(dir.path());
        let _task = db
            .create_task_with_assignee("Ghost", None, 0, None, Some("no-such-profile"))
            .expect("create");
        let result = dispatch_once(&db, &cfg, |_| true).expect("dispatch");
        assert_eq!(result.spawned, 0);
        assert_eq!(result.skipped_nonspawnable, 1);
    }

    #[test]
    fn dispatch_per_profile_cap_defers_excess() {
        let (dir, db) = test_db();
        let mut cfg = test_cfg(dir.path());
        cfg.max_workers = 10;
        cfg.max_in_progress_per_profile = Some(1);
        for i in 0..3 {
            db.create_task_with_assignee(
                &format!("Task {i}"),
                None,
                0,
                None,
                Some("default"),
            )
            .expect("create");
        }
        let result = dispatch_once(&db, &cfg, |_| true).expect("dispatch");
        assert_eq!(result.spawned, 1);
        assert_eq!(result.skipped_per_profile_capped, 2);
    }

    #[test]
    fn dispatch_respawn_guard_blocks_auth_error_task() {
        let (dir, db) = test_db();
        let cfg = test_cfg(dir.path());
        let t = db.create_task("Fail", None, 0).expect("create");
        db.claim_task(&t.id, "w1", 900).expect("claim");
        db.handle_worker_failure(&t.id, "w1", "403 unauthorized", 5)
            .expect("fail");
        let result = dispatch_once(&db, &cfg, |_| true).expect("dispatch");
        assert_eq!(result.spawned, 0);
        assert_eq!(result.respawn_guarded, 1);
    }
}
