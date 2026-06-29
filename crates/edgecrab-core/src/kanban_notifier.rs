//! Gateway kanban notifier — deliver terminal task events to subscribed chats.
//!
//! Hermes parity: `gateway/kanban_watchers.py::_kanban_notifier_watcher`.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use edgecrab_state::{
    KanbanDb, KanbanEvent, KanbanNotifySub, KanbanTask, KANBAN_NOTIFY_TERMINAL_KINDS,
    kanban_db_path_for_board, list_board_slugs,
};

fn payload_field(payload: Option<&str>, key: &str) -> Option<String> {
    let raw = payload?;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return v.get(key).and_then(|v| v.as_str()).map(str::to_string);
    }
    if key == "reason" || key == "error" || key == "summary" {
        return Some(raw.to_string());
    }
    None
}

/// Format a human-readable notifier message for one terminal event.
pub fn format_notifier_message(
    task_id: &str,
    title: &str,
    worker: Option<&str>,
    event: &KanbanEvent,
    task: Option<&KanbanTask>,
) -> Option<String> {
    let tag = worker.map(|w| format!("@{w} ")).unwrap_or_default();
    let title = if title.len() > 120 {
        crate::safe_truncate(title, 120)
    } else {
        title
    };
    let msg = match event.kind.as_str() {
        "completed" => {
            let handoff = payload_field(event.payload.as_deref(), "summary")
                .or_else(|| task.and_then(|t| t.result.clone()))
                .map(|s| {
                    let line = s.lines().next().unwrap_or(&s);
                    let line = if line.len() > 200 {
                        safe_truncate_local(line, 200)
                    } else {
                        line.to_string()
                    };
                    format!("\n{line}")
                })
                .unwrap_or_default();
            format!("✔ {tag}Kanban {task_id} done — {title}{handoff}")
        }
        "blocked" => {
            let reason = payload_field(event.payload.as_deref(), "reason")
                .map(|r| format!(": {}", safe_truncate_local(&r, 160)))
                .unwrap_or_default();
            format!("⏸ {tag}Kanban {task_id} blocked{reason}")
        }
        "gave_up" | "spawn_auto_blocked" => {
            let err = payload_field(event.payload.as_deref(), "error")
                .map(|e| format!("\n{}", safe_truncate_local(&e, 200)))
                .unwrap_or_default();
            format!(
                "✖ {tag}Kanban {task_id} gave up after repeated failures{err}"
            )
        }
        "crashed" => format!(
            "✖ {tag}Kanban {task_id} worker crashed; dispatcher will retry"
        ),
        "timed_out" => {
            let limit = payload_field(event.payload.as_deref(), "limit_seconds")
                .unwrap_or_else(|| "0".into());
            format!("⏱ {tag}Kanban {task_id} timed out (max_runtime={limit}s); will retry")
        }
        _ => return None,
    };
    Some(msg)
}

/// One outbound notification the gateway should deliver.
#[derive(Debug, Clone)]
pub struct KanbanNotifyOutbound {
    pub board_slug: String,
    pub db_path: PathBuf,
    pub sub: KanbanNotifySub,
    pub old_cursor: i64,
    pub cursor: i64,
    pub event: KanbanEvent,
    pub task: Option<KanbanTask>,
    pub message: String,
}

pub type KanbanNotifyDeliverFn = Arc<
    dyn Fn(KanbanNotifyOutbound) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
        + Send
        + Sync,
>;

pub type KanbanActivePlatformsFn = Arc<dyn Fn() -> HashSet<String> + Send + Sync>;

#[derive(Debug, Clone)]
pub(crate) struct KanbanNotifyBatch {
    board_slug: String,
    db_path: PathBuf,
    sub: KanbanNotifySub,
    old_cursor: i64,
    cursor: i64,
    events: Vec<KanbanEvent>,
    task: Option<KanbanTask>,
}

fn safe_truncate_local(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

fn collect_board_deliveries(
    _home: &Path,
    db_path: &Path,
    board_slug: &str,
    active_platforms: &HashSet<String>,
) -> Vec<KanbanNotifyBatch> {
    let Ok(db) = KanbanDb::open(db_path) else {
        return Vec::new();
    };
    let Ok(subs) = db.list_notify_subs() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for sub in subs {
        let platform = sub.platform.to_ascii_lowercase();
        if !active_platforms.contains(&platform) {
            continue;
        }
        let Ok((old_cursor, cursor, events)) = db.claim_unseen_events_for_sub(
            &sub.task_id,
            &sub.platform,
            &sub.chat_id,
            Some(sub.thread_id.as_str()).filter(|s| !s.is_empty()),
            KANBAN_NOTIFY_TERMINAL_KINDS,
        ) else {
            continue;
        };
        if events.is_empty() {
            continue;
        }
        let task = db.get_task(&sub.task_id).ok().flatten();
        out.push(KanbanNotifyBatch {
            board_slug: board_slug.to_string(),
            db_path: db_path.to_path_buf(),
            sub,
            old_cursor,
            cursor,
            events,
            task,
        });
    }
    out
}

/// Collect pending notifier deliveries across all boards (blocking).
pub(crate) fn collect_notifier_deliveries(
    home: &Path,
    active_platforms: &HashSet<String>,
) -> Vec<KanbanNotifyBatch> {
    if active_platforms.is_empty() {
        return Vec::new();
    }
    let mut seen_db_paths = HashSet::new();
    let mut deliveries = Vec::new();
    for slug in list_board_slugs(home) {
        let db_path = kanban_db_path_for_board(home, Some(&slug));
        let resolved = db_path
            .canonicalize()
            .unwrap_or(db_path)
            .to_string_lossy()
            .to_string();
        if !seen_db_paths.insert(resolved.clone()) {
            continue;
        }
        deliveries.extend(collect_board_deliveries(
            home,
            Path::new(&resolved),
            &slug,
            active_platforms,
        ));
    }
    deliveries
}

/// Rewind a failed delivery claim so the notifier can retry.
pub fn rewind_notifier_claim(
    db_path: &Path,
    sub: &KanbanNotifySub,
    claimed_cursor: i64,
    old_cursor: i64,
) -> bool {
    KanbanDb::open(db_path)
        .ok()
        .and_then(|db| {
            db.rewind_notify_cursor(
                &sub.task_id,
                &sub.platform,
                &sub.chat_id,
                Some(sub.thread_id.as_str()).filter(|s| !s.is_empty()),
                claimed_cursor,
                old_cursor,
            )
            .ok()
        })
        .unwrap_or(false)
}

/// Remove subscriptions once a task is truly done.
pub fn cleanup_notifier_subs(db_path: &Path, task_id: &str) {
    if let Ok(db) = KanbanDb::open(db_path) {
        let _ = db.remove_notify_subs_for_task(task_id);
    }
}

const MAX_SEND_FAILURES: u32 = 3;

/// Poll notify subscriptions and deliver terminal events via the gateway.
pub fn spawn_kanban_notifier(
    home: impl AsRef<Path>,
    interval_secs: u64,
    active_platforms_fn: KanbanActivePlatformsFn,
    deliver_fn: KanbanNotifyDeliverFn,
) -> tokio::task::JoinHandle<()> {
    let home = home.as_ref().to_path_buf();
    let interval = Duration::from_secs(interval_secs.max(5));
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut fail_counts: HashMap<(String, String, String, String), u32> = HashMap::new();
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let platforms = active_platforms_fn();
            if platforms.is_empty() {
                continue;
            }
            let home_clone = home.clone();
            let deliveries = tokio::task::spawn_blocking(move || {
                collect_notifier_deliveries(&home_clone, &platforms)
            })
            .await
            .unwrap_or_default();

            for batch in deliveries {
                let sub_key = (
                    batch.sub.task_id.clone(),
                    batch.sub.platform.clone(),
                    batch.sub.chat_id.clone(),
                    batch.sub.thread_id.clone(),
                );
                let worker = batch
                    .task
                    .as_ref()
                    .and_then(|t| t.worker_id.as_deref());
                let title = batch
                    .task
                    .as_ref()
                    .map(|t| t.title.as_str())
                    .unwrap_or(&batch.sub.task_id);
                let mut send_failed = false;
                for ev in &batch.events {
                    let Some(message) =
                        format_notifier_message(&batch.sub.task_id, title, worker, ev, batch.task.as_ref())
                    else {
                        continue;
                    };
                    let is_completed = ev.kind == "completed";
                    let outbound = KanbanNotifyOutbound {
                        board_slug: batch.board_slug.clone(),
                        db_path: batch.db_path.clone(),
                        sub: batch.sub.clone(),
                        old_cursor: batch.old_cursor,
                        cursor: batch.cursor,
                        event: ev.clone(),
                        task: batch.task.clone(),
                        message,
                    };
                    let deliver = deliver_fn.clone();
                    match deliver(outbound).await {
                        Ok(()) => {
                            fail_counts.remove(&sub_key);
                            if is_completed {
                                let db_path = batch.db_path.clone();
                                let task_id = batch.sub.task_id.clone();
                                let _ = tokio::task::spawn_blocking(move || {
                                    cleanup_notifier_subs(&db_path, &task_id);
                                })
                                .await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                task_id = %batch.sub.task_id,
                                platform = %batch.sub.platform,
                                error = %e,
                                "kanban notifier: delivery failed"
                            );
                            send_failed = true;
                            break;
                        }
                    }
                }
                if send_failed {
                    let count = fail_counts.entry(sub_key.clone()).or_insert(0);
                    *count += 1;
                    if *count >= MAX_SEND_FAILURES {
                        tracing::warn!(
                            task_id = %batch.sub.task_id,
                            platform = %batch.sub.platform,
                            "kanban notifier: dropping dead subscription after repeated send failures"
                        );
                        let db_path = batch.db_path.clone();
                        let task_id = batch.sub.task_id.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            cleanup_notifier_subs(&db_path, &task_id);
                        })
                        .await;
                        fail_counts.remove(&sub_key);
                    } else {
                        let db_path = batch.db_path.clone();
                        let sub = batch.sub.clone();
                        let claimed = batch.cursor;
                        let old = batch.old_cursor;
                        let _ = tokio::task::spawn_blocking(move || {
                            rewind_notifier_claim(&db_path, &sub, claimed, old);
                        })
                        .await;
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_completed_with_summary() {
        let ev = KanbanEvent {
            id: 1,
            task_id: "kb-1".into(),
            kind: "completed".into(),
            payload: Some(r#"{"summary":"Shipped auth refactor"}"#.into()),
            created_at: 0,
        };
        let msg = format_notifier_message("kb-1", "Auth", Some("worker-a"), &ev, None)
            .expect("msg");
        assert!(msg.contains("done"));
        assert!(msg.contains("Shipped"));
    }

    #[test]
    fn format_gave_up() {
        let ev = KanbanEvent {
            id: 2,
            task_id: "kb-2".into(),
            kind: "gave_up".into(),
            payload: Some(r#"{"error":"spawn failed"}"#.into()),
            created_at: 0,
        };
        let msg = format_notifier_message("kb-2", "Flaky", None, &ev, None).expect("msg");
        assert!(msg.contains("gave up"));
        assert!(msg.contains("spawn failed"));
    }
}
