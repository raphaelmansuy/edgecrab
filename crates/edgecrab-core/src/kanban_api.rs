//! Read-only Kanban HTTP API payloads — Hermes dashboard `/api/plugins/kanban/*` subset.

use std::path::Path;
use std::sync::Arc;

use edgecrab_state::{
    KanbanComment, KanbanDb, KanbanEvent, KanbanRun, KanbanStatus, KanbanTask, get_current_board,
    kanban_db_path_for_board, kanban_home, list_board_slugs,
};
use edgecrab_types::AgentError;
use serde_json::{json, Value};

use crate::kanban_task_patch::{patch_kanban_task, TaskPatch};

pub fn task_to_json(t: &KanbanTask) -> Value {
    json!({
        "id": t.id,
        "title": t.title,
        "body": t.body,
        "status": t.status,
        "priority": t.priority,
        "worker_id": t.worker_id,
        "claim_expires": t.claim_expires,
        "result": t.result,
        "created_at": t.created_at,
        "updated_at": t.updated_at,
        "consecutive_failures": t.consecutive_failures,
        "last_failure_error": t.last_failure_error,
        "max_retries": t.max_retries,
        "current_run_id": t.current_run_id,
        "max_runtime_seconds": t.max_runtime_seconds,
        "assignee": t.assignee,
        "scheduled_at": t.scheduled_at,
    })
}

fn run_to_json(r: &KanbanRun) -> Value {
    json!({
        "id": r.id,
        "task_id": r.task_id,
        "status": r.status,
        "worker_id": r.worker_id,
        "started_at": r.started_at,
        "ended_at": r.ended_at,
        "outcome": r.outcome,
        "summary": r.summary,
        "error": r.error,
    })
}

fn comment_to_json(c: &KanbanComment) -> Value {
    json!({
        "id": c.id,
        "task_id": c.task_id,
        "author": c.author,
        "body": c.body,
        "created_at": c.created_at,
    })
}

fn event_to_json(ev: &KanbanEvent) -> Value {
    let payload = ev
        .payload
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
    json!({
        "id": ev.id,
        "task_id": ev.task_id,
        "kind": ev.kind,
        "payload": payload,
        "created_at": ev.created_at,
    })
}

fn open_board_db(home: &Path, board: Option<&str>) -> Result<(String, Arc<KanbanDb>), AgentError> {
    let slug = board
        .and_then(|b| {
            let s = b.trim().to_ascii_lowercase();
            if s.is_empty() { None } else { Some(s) }
        })
        .unwrap_or_else(|| get_current_board(home));
    let path = kanban_db_path_for_board(home, Some(&slug));
    let db = KanbanDb::open(&path)?;
    Ok((slug, db))
}

fn status_counts(db: &KanbanDb) -> Result<Value, AgentError> {
    let mut counts = json!({});
    for (key, status) in [
        ("triage", KanbanStatus::Triage),
        ("todo", KanbanStatus::Todo),
        ("doing", KanbanStatus::Doing),
        ("blocked", KanbanStatus::Blocked),
        ("done", KanbanStatus::Done),
    ] {
        let n = db.list_tasks(Some(status), 500)?.len();
        counts[key] = json!(n);
    }
    Ok(counts)
}

/// Full board grouped by column — `GET /api/kanban/board`.
pub fn board_snapshot(home: Option<&Path>, board: Option<&str>) -> Result<Value, AgentError> {
    let home = kanban_home(home);
    let (slug, db) = open_board_db(&home, board)?;
    let _ = db.reclaim_stale_claims();

    let mut columns = json!({
        "triage": [],
        "todo": [],
        "doing": [],
        "blocked": [],
        "done": [],
    });

    for t in db.list_tasks(None, 500)? {
        if t.status == "archived" {
            continue;
        }
        let col = match t.status.as_str() {
            "triage" => "triage",
            "doing" => "doing",
            "blocked" => "blocked",
            "done" => "done",
            _ => "todo",
        };
        if let Some(arr) = columns.get_mut(col).and_then(|v| v.as_array_mut()) {
            arr.push(task_to_json(&t));
        }
    }

    Ok(json!({
        "board": slug,
        "current": get_current_board(&home),
        "columns": columns,
        "counts": status_counts(&db)?,
    }))
}

/// List boards with per-board counts — `GET /api/kanban/boards`.
pub fn boards_list(home: Option<&Path>) -> Result<Value, AgentError> {
    let home = kanban_home(home);
    let current = get_current_board(&home);
    let mut boards = Vec::new();
    for slug in list_board_slugs(&home) {
        let path = kanban_db_path_for_board(&home, Some(&slug));
        let counts = KanbanDb::open(&path)
            .ok()
            .and_then(|db| status_counts(&db).ok())
            .unwrap_or_else(|| json!({}));
        let total = counts
            .as_object()
            .map(|m| m.values().filter_map(|v| v.as_u64()).sum::<u64>())
            .unwrap_or(0);
        boards.push(json!({
            "slug": slug,
            "is_current": slug == current,
            "counts": counts,
            "total": total,
        }));
    }
    Ok(json!({ "boards": boards, "current": current }))
}

/// Task detail with runs + comments — `GET /api/kanban/tasks/:id`.
pub fn task_detail(
    home: Option<&Path>,
    board: Option<&str>,
    task_id: &str,
) -> Result<Value, AgentError> {
    let home = kanban_home(home);
    let (slug, db) = open_board_db(&home, board)?;
    let Some(task) = db.get_task(task_id)? else {
        return Err(AgentError::Validation(format!("task '{task_id}' not found")));
    };
    let runs = db
        .list_task_runs(task_id, 20)?
        .into_iter()
        .map(|r| run_to_json(&r))
        .collect::<Vec<_>>();
    let comments = db
        .list_comments(task_id)?
        .into_iter()
        .map(|c| comment_to_json(&c))
        .collect::<Vec<_>>();
    let worker_context = edgecrab_state::build_worker_context(&db, task_id)?;
    Ok(json!({
        "board": slug,
        "task": task_to_json(&task),
        "runs": runs,
        "comments": comments,
        "worker_context": worker_context,
    }))
}

/// Tail task lifecycle events — `GET /api/kanban/events` and WebSocket poll loop.
pub fn events_since(
    home: Option<&Path>,
    board: Option<&str>,
    since_id: i64,
    limit: usize,
) -> Result<Value, AgentError> {
    let home = kanban_home(home);
    let (slug, db) = open_board_db(&home, board)?;
    let (cursor, events) = db.list_events_after(since_id, limit)?;
    let events_json = events.iter().map(event_to_json).collect::<Vec<_>>();
    Ok(json!({
        "board": slug,
        "cursor": cursor,
        "events": events_json,
    }))
}

/// Partial task update — `PATCH /api/kanban/tasks/:id`.
pub fn patch_task(
    home: Option<&Path>,
    board: Option<&str>,
    task_id: &str,
    patch: &TaskPatch,
    install_root: &Path,
) -> Result<Value, AgentError> {
    let home = kanban_home(home);
    let (_slug, db) = open_board_db(&home, board)?;
    patch_kanban_task(&db, task_id, patch, install_root)
}

/// Hard-delete task — `DELETE /api/kanban/tasks/:id`.
pub fn delete_task(
    home: Option<&Path>,
    board: Option<&str>,
    task_id: &str,
) -> Result<Value, AgentError> {
    crate::kanban_workers::cancel_worker(task_id);
    let home = kanban_home(home);
    let (slug, db) = open_board_db(&home, board)?;
    if !db.delete_task(task_id)? {
        return Err(AgentError::Validation(format!("task '{task_id}' not found")));
    }
    Ok(json!({
        "deleted": true,
        "task_id": task_id,
        "board": slug,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_state::KanbanDb;
    use tempfile::TempDir;

    #[test]
    fn board_snapshot_groups_by_status() {
        let dir = TempDir::new().expect("tmpdir");
        let home = dir.path();
        let db = KanbanDb::open_default(Some(home)).expect("open");
        let t = db.create_task("Test card", None, 0).expect("create");
        db.claim_task(&t.id, "w1", 900).expect("claim");

        let snap = board_snapshot(Some(home), None).expect("snap");
        assert_eq!(snap["board"], "default");
        assert_eq!(snap["columns"]["doing"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn events_since_returns_cursor_and_payload() {
        let dir = TempDir::new().expect("tmpdir");
        let home = dir.path();
        let db = KanbanDb::open_default(Some(home)).expect("open");
        let t = db.create_task("Evt", None, 0).expect("create");
        db.claim_task(&t.id, "w1", 900).expect("claim");
        db.complete_task(&t.id, Some("w1"), Some("ok")).expect("done");

        let body = events_since(Some(home), None, 0, 100).expect("events");
        assert_eq!(body["board"], "default");
        assert!(body["cursor"].as_i64().unwrap_or(0) > 0);
        let events = body["events"].as_array().expect("arr");
        assert!(events.iter().any(|e| e["kind"] == "completed"));
    }
}
