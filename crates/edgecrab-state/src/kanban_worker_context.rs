//! Worker context builder — Hermes `build_worker_context` subset.
//!
//! Bounded markdown handoff injected into dispatched workers and `kanban_show`.

use crate::{KanbanDb, KanbanRun};
use edgecrab_types::AgentError;

const MAX_PRIOR_ATTEMPTS: usize = 10;
const MAX_COMMENTS: usize = 30;
const MAX_FIELD_BYTES: usize = 4 * 1024;
const MAX_BODY_BYTES: usize = 8 * 1024;
const MAX_COMMENT_BYTES: usize = 2 * 1024;

fn cap(s: &str, limit: usize) -> String {
    let s = s.trim();
    let char_count = s.chars().count();
    if char_count <= limit {
        return s.to_string();
    }
    let truncated: String = s.chars().take(limit).collect();
    format!(
        "{truncated}… [truncated, {} chars omitted]",
        char_count - limit
    )
}

fn format_ts(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| unix.to_string())
}

/// Full text a worker should read to understand its task (bounded).
pub fn build_worker_context(db: &KanbanDb, task_id: &str) -> Result<String, AgentError> {
    let task = db
        .get_task(task_id)?
        .ok_or_else(|| AgentError::Validation(format!("unknown task {task_id}")))?;

    let mut lines = vec![
        format!("# Kanban task {}: {}", task.id, task.title),
        String::new(),
        format!(
            "Assignee: {}",
            task.assignee.as_deref().unwrap_or("(unassigned)")
        ),
        format!("Status:   {}", task.status),
    ];
    if let Some(secs) = task.max_runtime_seconds.filter(|&s| s > 0) {
        lines.push(format!("Max runtime: {secs}s"));
    }
    if let Some(at) = task.scheduled_at {
        lines.push(format!("Scheduled at: {}", format_ts(at)));
    }
    lines.push(String::new());

    if let Some(body) = task.body.as_deref().filter(|b| !b.trim().is_empty()) {
        lines.push("## Body".into());
        lines.push(cap(body, MAX_BODY_BYTES));
        lines.push(String::new());
    }

    append_prior_attempts(db, task_id, &mut lines)?;
    append_parent_results(db, task_id, &mut lines)?;
    append_comments(db, task_id, &mut lines)?;

    Ok(format!("{}\n", lines.join("\n").trim_end()))
}

fn append_prior_attempts(
    db: &KanbanDb,
    task_id: &str,
    lines: &mut Vec<String>,
) -> Result<(), AgentError> {
    let all = db
        .list_task_runs(task_id, 100)?
        .into_iter()
        .filter(|r| r.ended_at.is_some())
        .collect::<Vec<_>>();
    if all.is_empty() {
        return Ok(());
    }
    let (omitted, shown): (usize, Vec<KanbanRun>) = if all.len() > MAX_PRIOR_ATTEMPTS {
        let omitted = all.len() - MAX_PRIOR_ATTEMPTS;
        (omitted, all.into_iter().take(MAX_PRIOR_ATTEMPTS).rev().collect())
    } else {
        (0, all.into_iter().rev().collect())
    };
    lines.push("## Prior attempts on this task".into());
    if omitted > 0 {
        lines.push(format!(
            "_({omitted} earlier attempt{} omitted; showing most recent {})_",
            if omitted == 1 { "" } else { "s" },
            shown.len()
        ));
    }
    let first_idx = omitted + 1;
    for (offset, run) in shown.iter().enumerate() {
        let idx = first_idx + offset;
        let ts = run.ended_at.map(format_ts).unwrap_or_default();
        let worker = run.worker_id.as_deref().unwrap_or("(unknown)");
        let outcome = run.outcome.as_deref().unwrap_or(&run.status);
        lines.push(format!("### Attempt {idx} — {outcome} ({worker}, {ts})"));
        if let Some(summary) = run.summary.as_deref().filter(|s| !s.trim().is_empty()) {
            lines.push(cap(summary, MAX_FIELD_BYTES));
        }
        if let Some(error) = run.error.as_deref().filter(|s| !s.trim().is_empty()) {
            lines.push(format!("_error_: {}", cap(error, MAX_FIELD_BYTES)));
        }
        lines.push(String::new());
    }
    Ok(())
}

fn append_parent_results(
    db: &KanbanDb,
    task_id: &str,
    lines: &mut Vec<String>,
) -> Result<(), AgentError> {
    let parent_ids = db.parent_ids(task_id)?;
    let mut wrote_header = false;
    for pid in parent_ids {
        let Some(parent) = db.get_task(&pid)? else {
            continue;
        };
        if parent.status != "done" {
            continue;
        }
        if !wrote_header {
            lines.push("## Parent task results".into());
            wrote_header = true;
        }
        lines.push(format!("### {}", parent.id));
        let runs = db
            .list_task_runs(&pid, 20)?
            .into_iter()
            .filter(|r| r.outcome.as_deref() == Some("completed"))
            .collect::<Vec<_>>();
        if let Some(run) = runs.first() {
            if let Some(summary) = run.summary.as_deref().filter(|s| !s.trim().is_empty()) {
                lines.push(cap(summary, MAX_FIELD_BYTES));
            } else if let Some(result) = parent.result.as_deref().filter(|s| !s.trim().is_empty())
            {
                lines.push(cap(result, MAX_FIELD_BYTES));
            } else {
                lines.push("(no result recorded)".into());
            }
        } else if let Some(result) = parent.result.as_deref().filter(|s| !s.trim().is_empty()) {
            lines.push(cap(result, MAX_FIELD_BYTES));
        } else {
            lines.push("(no result recorded)".into());
        }
        lines.push(String::new());
    }
    Ok(())
}

fn append_comments(
    db: &KanbanDb,
    task_id: &str,
    lines: &mut Vec<String>,
) -> Result<(), AgentError> {
    let all = db.list_comments(task_id)?;
    if all.is_empty() {
        return Ok(());
    }
    let (omitted, shown) = if all.len() > MAX_COMMENTS {
        let omitted = all.len() - MAX_COMMENTS;
        (omitted, &all[all.len() - MAX_COMMENTS..])
    } else {
        (0, all.as_slice())
    };
    lines.push("## Comment thread".into());
    if omitted > 0 {
        lines.push(format!(
            "_({omitted} earlier comment{} omitted; showing most recent {})_",
            if omitted == 1 { "" } else { "s" },
            shown.len()
        ));
    }
    for c in shown {
        let author = c.author.replace('`', "");
        lines.push(format!(
            "comment from worker `{author}` at {}:",
            format_ts(c.created_at)
        ));
        lines.push(cap(&c.body, MAX_COMMENT_BYTES));
        lines.push(String::new());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KanbanDb;
    use tempfile::TempDir;

    #[test]
    fn context_includes_prior_attempts_and_parent_handoff() {
        let dir = TempDir::new().expect("tmpdir");
        let db = KanbanDb::open_default(Some(dir.path())).expect("open");
        let parent = db.create_task("Parent", Some("parent body"), 0).expect("parent");
        db.claim_task(&parent.id, "w0", 900).expect("claim p");
        db.complete_task(&parent.id, Some("w0"), Some("parent shipped summary"))
            .expect("done p");

        let child = db.create_task("Child", Some("child body"), 0).expect("child");
        db.link_tasks(&parent.id, &child.id).expect("link");

        db.claim_task(&child.id, "w1", 900).expect("claim c");
        let _ = db.handle_worker_failure(&child.id, "w1", "tests failed", 5);
        db.claim_task(&child.id, "w2", 900).expect("reclaim");

        let ctx = build_worker_context(&db, &child.id).expect("ctx");
        assert!(ctx.contains("Prior attempts on this task"));
        assert!(ctx.contains("Parent task results"));
        assert!(ctx.contains("parent shipped summary"));
        assert!(ctx.contains("child body"));
    }
}
