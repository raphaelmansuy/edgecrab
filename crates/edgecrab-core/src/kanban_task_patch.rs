//! Kanban task PATCH — Hermes `PATCH /tasks/:id` subset.

use std::path::Path;

use edgecrab_state::KanbanDb;
use edgecrab_types::AgentError;
use serde::Deserialize;
use serde_json::Value;

use crate::kanban_api::task_to_json;
use crate::kanban_profiles::{normalize_profile_name, profile_exists};

/// Prefix for structured 409 responses (`validation_err` maps these to HTTP 409).
pub const CONFLICT_PREFIX: &str = "KANBAN_CONFLICT:";

/// Parse a conflict payload embedded in `AgentError::Validation`.
pub fn parse_conflict(msg: &str) -> Option<Value> {
    msg.strip_prefix(CONFLICT_PREFIX)
        .and_then(|raw| serde_json::from_str(raw).ok())
}

/// Partial task update from dashboard / API.
#[derive(Debug, Deserialize, Default)]
pub struct TaskPatch {
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<i32>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub result: Option<String>,
    /// Handoff summary when marking done (Hermes dashboard parity).
    pub summary: Option<String>,
    pub block_reason: Option<String>,
    /// Set/clear future dispatch time. JSON `null` clears the gate.
    #[serde(default)]
    pub scheduled_at: Option<Option<i64>>,
}

/// Apply patch and return updated task JSON `{ "task": ... }`.
pub fn patch_kanban_task(
    db: &KanbanDb,
    task_id: &str,
    patch: &TaskPatch,
    install_root: &Path,
) -> Result<Value, AgentError> {
    if db.get_task(task_id)?.is_none() {
        return Err(AgentError::Validation(format!("task '{task_id}' not found")));
    }

    if let Some(ref raw) = patch.assignee {
        let name = raw.trim();
        if name.is_empty() {
            db.set_task_assignee(task_id, None)?;
        } else {
            let canon = normalize_profile_name(name);
            if !profile_exists(install_root, &canon) {
                return Err(AgentError::Validation(format!(
                    "profile '{name}' does not exist"
                )));
            }
            db.set_task_assignee(task_id, Some(&canon))?;
        }
    }

    if let Some(ref status) = patch.status {
        apply_status(db, task_id, status.trim(), patch)?;
    }

    if let Some(priority) = patch.priority {
        db.set_task_priority(task_id, priority)?;
    }

    if patch.title.is_some() || patch.body.is_some() {
        let title = patch.title.as_deref();
        let body = patch.body.as_deref();
        db.edit_task(task_id, title, body)?;
    }

    if let Some(at) = patch.scheduled_at {
        db.set_scheduled_at(task_id, at)?;
    }

    let task = db
        .get_task(task_id)?
        .ok_or_else(|| AgentError::Validation(format!("task '{task_id}' not found")))?;
    Ok(serde_json::json!({ "task": task_to_json(&task) }))
}

fn apply_status(
    db: &KanbanDb,
    task_id: &str,
    status: &str,
    patch: &TaskPatch,
) -> Result<(), AgentError> {
    match status {
        "done" => {
            let result = patch
                .result
                .as_deref()
                .or(patch.summary.as_deref());
            db.complete_task(task_id, None, result)?;
        }
        "blocked" => {
            db.block_task(task_id, None, patch.block_reason.as_deref())?;
        }
        "todo" => {
            let current = db.get_task(task_id)?.map(|t| t.status);
            if current.as_deref() == Some("blocked") {
                db.unblock_task(task_id)?;
            } else {
                db.set_task_status(task_id, "todo")?;
            }
        }
        "triage" => {
            db.set_task_status(task_id, "triage")?;
        }
        "archived" => {
            db.archive_task(task_id)?;
        }
        "doing" => {
            return Err(AgentError::Validation(
                "cannot set status to 'doing' directly; use dispatcher".into(),
            ));
        }
        other => {
            return Err(AgentError::Validation(format!("unknown status: {other}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgecrab_state::KanbanDb;
    use edgecrab_types::AgentError;
    use tempfile::TempDir;

    fn test_db() -> (TempDir, std::sync::Arc<KanbanDb>) {
        let dir = TempDir::new().expect("tmpdir");
        let db = KanbanDb::open_default(Some(dir.path())).expect("open");
        (dir, db)
    }

    #[test]
    fn patch_assignee_and_title() {
        let (dir, db) = test_db();
        let t = db.create_task("Old", None, 0).expect("create");
        let patch = TaskPatch {
            title: Some("New title".into()),
            assignee: Some("default".into()),
            ..Default::default()
        };
        patch_kanban_task(&db, &t.id, &patch, dir.path()).expect("patch");
        let row = db.get_task(&t.id).expect("get").expect("row");
        assert_eq!(row.title, "New title");
        assert_eq!(row.assignee.as_deref(), Some("default"));
    }

    #[test]
    fn patch_todo_blocked_by_parent_returns_conflict() {
        let (dir, db) = test_db();
        let parent = db.create_task("Parent", None, 0).expect("parent");
        let child = db.create_task("Child", None, 0).expect("child");
        db.link_tasks(&parent.id, &child.id).expect("link");
        db.set_task_status(&child.id, "triage").expect("triage");
        let patch = TaskPatch {
            status: Some("todo".into()),
            ..Default::default()
        };
        let err = patch_kanban_task(&db, &child.id, &patch, dir.path()).unwrap_err();
        let AgentError::Validation(msg) = err else {
            panic!("expected validation conflict");
        };
        let body = parse_conflict(&msg).expect("conflict json");
        assert!(body.get("blockers").and_then(|v| v.as_array()).is_some_and(|a| !a.is_empty()));
    }
}
