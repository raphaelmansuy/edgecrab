//! Gateway kanban watcher — reclaim + dispatch tick (Hermes `_kanban_dispatcher_watcher` subset).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use edgecrab_state::{KanbanDb, kanban_db_path_for_board, list_board_slugs};

use crate::kanban_dispatcher::{KanbanDispatchConfig, KanbanSpawnRequest, dispatch_once};

pub type KanbanSpawnFn = Arc<dyn Fn(KanbanSpawnRequest) -> bool + Send + Sync>;

fn tick_boards(
    home: &Path,
    dispatch_cfg: &KanbanDispatchConfig,
    spawn_fn: Option<&KanbanSpawnFn>,
) -> Option<crate::kanban_dispatcher::KanbanDispatchResult> {
    let mut aggregate = crate::kanban_dispatcher::KanbanDispatchResult::default();
    let mut any = false;
    for slug in list_board_slugs(home) {
        let path = kanban_db_path_for_board(home, Some(&slug));
        let Ok(db) = KanbanDb::open(&path) else {
            continue;
        };
        let board_result = if let Some(spawn) = spawn_fn {
            dispatch_once(&db, dispatch_cfg, |req| spawn(req)).ok()
        } else {
            let reclaimed = db
                .reclaim_stale_claims_with_limit(dispatch_cfg.failure_limit)
                .ok();
            let timed_out = db
                .enforce_max_runtime_with(dispatch_cfg.failure_limit, |task_id| {
                    crate::kanban_workers::cancel_worker(task_id);
                })
                .ok();
            let promoted = db.recompute_ready(dispatch_cfg.failure_limit).ok();
            reclaimed.map(|reclaimed| {
                crate::kanban_dispatcher::KanbanDispatchResult {
                    reclaimed,
                    timed_out: timed_out.unwrap_or(0),
                    promoted: promoted.unwrap_or(0),
                    ..Default::default()
                }
            })
        };
        if let Some(r) = board_result {
            any = true;
            aggregate.reclaimed += r.reclaimed;
            aggregate.spawned += r.spawned;
            aggregate.promoted += r.promoted;
            aggregate.timed_out += r.timed_out;
            aggregate.skipped_at_capacity |= r.skipped_at_capacity;
            aggregate.skipped_per_profile_capped += r.skipped_per_profile_capped;
            aggregate.respawn_guarded += r.respawn_guarded;
        }
    }
    any.then_some(aggregate)
}

/// Spawn background tick: reclaim stale claims and optionally dispatch workers.
pub fn spawn_kanban_watcher(
    home: impl AsRef<Path>,
    interval_secs: u64,
    dispatch_cfg: KanbanDispatchConfig,
    spawn_fn: Option<KanbanSpawnFn>,
) -> tokio::task::JoinHandle<()> {
    let home = home.as_ref().to_path_buf();
    let interval = Duration::from_secs(interval_secs.max(15));
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let spawn = spawn_fn.clone();
            let cfg = dispatch_cfg.clone();
            let home = home.clone();
            let run = tokio::task::spawn_blocking(move || tick_boards(&home, &cfg, spawn.as_ref()))
                .await;
            if let Ok(Some(result)) = run {
                if result.reclaimed > 0 {
                    tracing::info!(reclaimed = result.reclaimed, "kanban: reclaimed stale claims");
                }
                if result.promoted > 0 {
                    tracing::info!(promoted = result.promoted, "kanban: promoted ready tasks");
                }
                if result.spawned > 0 {
                    tracing::info!(spawned = result.spawned, "kanban: dispatched workers");
                }
                if result.timed_out > 0 {
                    tracing::info!(timed_out = result.timed_out, "kanban: timed out long-running workers");
                }
                if result.skipped_per_profile_capped > 0 {
                    tracing::debug!(
                        deferred = result.skipped_per_profile_capped,
                        "kanban: deferred tasks at per-profile cap"
                    );
                }
                if result.respawn_guarded > 0 {
                    tracing::debug!(
                        guarded = result.respawn_guarded,
                        "kanban: deferred tasks by respawn guard"
                    );
                }
            }
        }
    })
}

/// Back-compat alias — reclaim-only watcher when no spawn callback is provided.
pub fn spawn_kanban_reaper(home: impl AsRef<Path>, interval_secs: u64) -> tokio::task::JoinHandle<()> {
    spawn_kanban_watcher(
        home,
        interval_secs,
        KanbanDispatchConfig::from_kanban_config(&crate::config::KanbanConfig {
            claim_ttl_secs: 900,
            max_workers: 1,
            failure_limit: 2,
            ..Default::default()
        }),
        None,
    )
}
