//! Shared `/kanban` slash handler (CLI + gateway).

use edgecrab_state::{KanbanDb, KanbanStatus, KanbanTask};

/// Gateway origin for auto-subscribing to task notifications on create.
#[derive(Debug, Clone)]
pub struct KanbanNotifyOrigin {
    pub platform: String,
    pub chat_id: String,
    pub thread_id: Option<String>,
    pub user_id: Option<String>,
}

pub fn handle_kanban_slash(
    args: &str,
    home: Option<&std::path::Path>,
    notify: Option<&KanbanNotifyOrigin>,
) -> String {
    let db = match KanbanDb::open_default(home) {
        Ok(db) => db,
        Err(e) => return format!("Kanban board unavailable: {e}"),
    };
    let _ = db.reclaim_stale_claims();

    let trimmed = args.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("list") || trimmed.eq_ignore_ascii_case("ls") {
        return format_task_list(&db, None, 20);
    }

    let mut tokens = trimmed.split_whitespace().collect::<Vec<_>>();
    let sub = tokens.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();

    match sub.as_str() {
        "status" => format_board_status(&db),
        "show" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban show <task-id>".into();
            };
            match db.get_task(id) {
                Ok(Some(t)) => format_task_detail(&t),
                Ok(None) => format!("Task not found: {id}"),
                Err(e) => format!("Error: {e}"),
            }
        }
        "create" | "add" => {
            if tokens.len() < 2 {
                return "Usage: /kanban create <title> [description] [--max-runtime SECS]".into();
            }
            tokens.remove(0);
            let mut max_runtime: Option<i32> = None;
            if let Some(pos) = tokens.iter().position(|t| *t == "--max-runtime") {
                if pos + 1 >= tokens.len() {
                    return "Usage: --max-runtime requires a value in seconds".into();
                }
                max_runtime = tokens[pos + 1].parse::<i32>().ok().filter(|v| *v > 0);
                tokens.remove(pos + 1);
                tokens.remove(pos);
            }
            let title = tokens.join(" ");
            match db.create_task_with_runtime(&title, None, 0, max_runtime) {
                Ok(t) => {
                    let mut reply =
                        format!("Created task {} — {}\nStatus: todo", t.id, t.title);
                    if let Some(origin) = notify
                        && db
                            .add_notify_sub(
                                &t.id,
                                &origin.platform,
                                &origin.chat_id,
                                origin.thread_id.as_deref(),
                                origin.user_id.as_deref(),
                            )
                            .is_ok()
                    {
                        reply.push_str(&format!(
                            "\nYou'll be notified here when {} finishes.",
                            t.id
                        ));
                    }
                    reply
                }
                Err(e) => format!("Create failed: {e}"),
            }
        }
        "triage" => {
            if tokens.len() < 2 {
                return "Usage: /kanban triage <title>".into();
            }
            tokens.remove(0);
            let title = tokens.join(" ");
            match db.create_triage_task(&title, None, 0) {
                Ok(t) => format!("Created triage task {} — {}\nRun /kanban decompose {} to fan out.", t.id, t.title, t.id),
                Err(e) => format!("Triage create failed: {e}"),
            }
        }
        "decompose" => {
            "Usage: /kanban decompose <task-id> — run from gateway with agent LLM, or enable kanban.auto_decompose.".into()
        }
        "claim" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban claim <task-id> [worker-id]".into();
            };
            let worker = tokens.get(2).copied().unwrap_or("cli");
            match db.claim_task(id, worker, KanbanDb::default_claim_ttl_secs()) {
                Ok(t) => format!("Claimed {} → doing (worker: {worker})", t.id),
                Err(e) => format!("Claim failed: {e}"),
            }
        }
        "complete" | "finish" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban complete <task-id> [result note]".into();
            };
            let note = tokens.get(2..).map(|p| p.join(" "));
            match db.complete_task(id, None, note.as_deref()) {
                Ok(t) => format!("Completed {} — {}", t.id, t.title),
                Err(e) => format!("Complete failed: {e}"),
            }
        }
        "release" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban release <task-id>".into();
            };
            match db.release_task(id, None) {
                Ok(t) => format!("Released {} → todo", t.id),
                Err(e) => format!("Release failed: {e}"),
            }
        }
        "block" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban block <task-id> [reason]".into();
            };
            let reason = tokens.get(2..).map(|p| p.join(" "));
            match db.block_task(id, None, reason.as_deref()) {
                Ok(t) => format!("Blocked {} — {}", t.id, t.title),
                Err(e) => format!("Block failed: {e}"),
            }
        }
        "unblock" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban unblock <task-id>".into();
            };
            match db.unblock_task(id) {
                Ok(t) => format!("Unblocked {} → todo", t.id),
                Err(e) => format!("Unblock failed: {e}"),
            }
        }
        "comment" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban comment <task-id> <text>".into();
            };
            if tokens.len() < 3 {
                return "Usage: /kanban comment <task-id> <text>".into();
            }
            let text = tokens[2..].join(" ");
            match db.add_comment(id, "cli", &text) {
                Ok(c) => format!("Comment #{} on {id}", c.id),
                Err(e) => format!("Comment failed: {e}"),
            }
        }
        "schedule" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban schedule <task-id> --at <ISO8601>|--clear".into();
            };
            if tokens.contains(&"--clear") {
                match db.set_scheduled_at(id, None) {
                    Ok(()) => format!("Cleared scheduled_at on {id}"),
                    Err(e) => format!("Schedule clear failed: {e}"),
                }
            } else if let Some(pos) = tokens.iter().position(|t| *t == "--at") {
                let Some(at_str) = tokens.get(pos + 1) else {
                    return "Usage: /kanban schedule <task-id> --at <ISO8601>".into();
                };
                match parse_schedule_at(at_str) {
                    Ok(at) => match db.set_scheduled_at(id, Some(at)) {
                        Ok(()) => format!(
                            "Scheduled {id} — dispatch after {}",
                            chrono::DateTime::from_timestamp(at, 0)
                                .map(|dt| dt.to_rfc3339())
                                .unwrap_or_else(|| at.to_string())
                        ),
                        Err(e) => format!("Schedule failed: {e}"),
                    },
                    Err(e) => format!("Invalid --at timestamp: {e}"),
                }
            } else {
                "Usage: /kanban schedule <task-id> --at <ISO8601>|--clear".into()
            }
        }
        "archive" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban archive <task-id>".into();
            };
            match db.archive_task(id) {
                Ok(t) => format!("Archived {} — {}", t.id, t.title),
                Err(e) => format!("Archive failed: {e}"),
            }
        }
        "delete" | "rm" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban delete <task-id>".into();
            };
            match db.delete_task(id) {
                Ok(true) => format!("Deleted {id}"),
                Ok(false) => format!("Task not found: {id}"),
                Err(e) => format!("Delete failed: {e}"),
            }
        }
        "subscribe" | "notify" => {
            let Some(id) = tokens.get(1) else {
                return "Usage: /kanban subscribe <task-id>".into();
            };
            let Some(origin) = notify else {
                return "Subscribe only works from a gateway chat (/kanban subscribe <task-id>)".into();
            };
            match db.add_notify_sub(
                id,
                &origin.platform,
                &origin.chat_id,
                origin.thread_id.as_deref(),
                origin.user_id.as_deref(),
            ) {
                Ok(()) => format!("Subscribed — you'll be notified here when {id} finishes or blocks."),
                Err(e) => format!("Subscribe failed: {e}"),
            }
        }
        "link" => {
            let Some(parent) = tokens.get(1) else {
                return "Usage: /kanban link <parent-id> <child-id>".into();
            };
            let Some(child) = tokens.get(2) else {
                return "Usage: /kanban link <parent-id> <child-id>".into();
            };
            match db.link_tasks(parent, child) {
                Ok(()) => format!("Linked {parent} → {child}"),
                Err(e) => format!("Link failed: {e}"),
            }
        }
        "dispatch" | "tick" => {
            let cfg = crate::kanban_dispatcher::KanbanDispatchConfig::from_kanban_config(
                &crate::config::KanbanConfig {
                    claim_ttl_secs: KanbanDb::default_claim_ttl_secs() as u32,
                    max_workers: 3,
                    failure_limit: edgecrab_state::DEFAULT_FAILURE_LIMIT,
                    ..Default::default()
                },
            );
            match crate::kanban_dispatcher::dispatch_once(&db, &cfg, |_| true) {
                Ok(r) => format!(
                    "Dispatch tick: reclaimed={}, timed_out={}, promoted={}, spawned={}, \
                     unassigned={}, nonspawnable={}, at_capacity={}",
                    r.reclaimed,
                    r.timed_out,
                    r.promoted,
                    r.spawned,
                    r.skipped_unassigned,
                    r.skipped_nonspawnable,
                    r.skipped_at_capacity
                ),
                Err(e) => format!("Dispatch failed: {e}"),
            }
        }
        "boards" => {
            let home = home.map(std::path::Path::to_path_buf).unwrap_or_else(|| {
                std::env::var("EDGECRAB_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| dirs::home_dir().unwrap_or_default().join(".edgecrab"))
            });
            let sub = tokens.get(1).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
            match sub.as_str() {
                "list" | "ls" | "" => {
                    let current = edgecrab_state::get_current_board(&home);
                    let boards = edgecrab_state::list_board_slugs(&home);
                    format!(
                        "Kanban boards (current: {current}):\n{}",
                        boards.iter().map(|b| format!("  • {b}")).collect::<Vec<_>>().join("\n")
                    )
                }
                "switch" | "use" => {
                    let Some(slug) = tokens.get(2) else {
                        return "Usage: /kanban boards switch <slug>".into();
                    };
                    match edgecrab_state::set_current_board(&home, slug) {
                        Ok(()) => format!("Switched to board '{slug}'"),
                        Err(e) => format!("Switch failed: {e}"),
                    }
                }
                "create" => {
                    let Some(slug) = tokens.get(2) else {
                        return "Usage: /kanban boards create <slug>".into();
                    };
                    match edgecrab_state::ensure_board(&home, slug) {
                        Ok(path) => format!("Board '{slug}' ready at {}", path.display()),
                        Err(e) => format!("Create failed: {e}"),
                    }
                }
                _ => "Usage: /kanban boards [list|switch <slug>|create <slug>]".into(),
            }
        }
        "todo" | "doing" | "done" | "blocked" => {
            let status = KanbanStatus::parse(&sub);
            format_task_list(&db, status, 30)
        }
        _ => format!(
            "Unknown subcommand.\n\nUsage: /kanban [list|status|triage <title>|create|decompose|show|claim|complete|...]\n\n{}",
            format_board_status(&db)
        ),
    }
}

fn format_board_status(db: &KanbanDb) -> String {
    let mut lines = vec!["Kanban board (~/.edgecrab/kanban.db):".to_string()];
    for (label, status) in [
        ("Triage", Some(KanbanStatus::Triage)),
        ("Todo", Some(KanbanStatus::Todo)),
        ("Doing", Some(KanbanStatus::Doing)),
        ("Blocked", Some(KanbanStatus::Blocked)),
        ("Done", Some(KanbanStatus::Done)),
    ] {
        match db.list_tasks(status, 500) {
            Ok(tasks) => lines.push(format!("  {label}: {}", tasks.len())),
            Err(e) => lines.push(format!("  {label}: error ({e})")),
        }
    }
    lines.push("\n/kanban list — all cards".into());
    lines.join("\n")
}

fn format_task_list(db: &KanbanDb, status: Option<KanbanStatus>, limit: usize) -> String {
    match db.list_tasks(status, limit) {
        Ok(tasks) if tasks.is_empty() => "No kanban tasks.".into(),
        Ok(tasks) => {
            let header = match status {
                Some(s) => format!("Kanban ({}, {} shown):", s.as_str(), tasks.len()),
                None => format!("Kanban ({} tasks):", tasks.len()),
            };
            let mut out = vec![header, String::new()];
            for t in tasks {
                out.push(format_task_line(&t));
            }
            out.join("\n")
        }
        Err(e) => format!("List failed: {e}"),
    }
}

fn format_task_line(t: &KanbanTask) -> String {
    let worker = t
        .worker_id
        .as_deref()
        .map(|w| format!(" @{w}"))
        .unwrap_or_default();
    format!(
        "  [{}] {} — {}{}",
        t.status, t.id, t.title, worker
    )
}

fn format_task_detail(t: &KanbanTask) -> String {
    let mut out = vec![
        format!("Task: {}", t.id),
        format!("Title: {}", t.title),
        format!("Status: {}", t.status),
        format!("Priority: {}", t.priority),
    ];
    if let Some(body) = &t.body {
        out.push(format!("Body: {body}"));
    }
    if let Some(w) = &t.worker_id {
        out.push(format!("Worker: {w}"));
    }
    if let Some(r) = &t.result {
        out.push(format!("Result: {r}"));
    }
    if let Some(at) = t.scheduled_at {
        out.push(format!(
            "Scheduled at: {}",
            chrono::DateTime::from_timestamp(at, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| at.to_string())
        ));
    }
    out.join("\n")
}

fn parse_schedule_at(raw: &str) -> Result<i64, String> {
    if let Ok(ts) = raw.parse::<i64>() {
        return Ok(ts);
    }
    chrono::DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.timestamp())
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S")
                .map(|ndt| ndt.and_utc().timestamp())
        })
        .map_err(|e| e.to_string())
}

/// Gateway `/kanban` handler — async decompose when agent + LLM available.
pub async fn handle_kanban_slash_gateway(
    args: &str,
    home: Option<&std::path::Path>,
    notify: Option<&KanbanNotifyOrigin>,
    agent: Option<std::sync::Arc<crate::Agent>>,
) -> String {
    let sub = args.split_whitespace().next().unwrap_or("");
    if sub.eq_ignore_ascii_case("decompose") {
        let task_id = args.split_whitespace().nth(1).unwrap_or("");
        if task_id.is_empty() {
            return "Usage: /kanban decompose <task-id>".into();
        }
        let Some(agent) = agent else {
            return "Kanban decompose requires a running gateway agent.".into();
        };
        let cfg = crate::AppConfig::load().unwrap_or_default();
        if !cfg.kanban.enabled {
            return "Kanban is disabled in config.".into();
        }
        let provider = agent.provider_handle().await;
        let model = agent.model().await;
        let outcome = crate::kanban_decompose::decompose_task_by_id(
            home,
            task_id,
            provider,
            &model,
            &cfg,
        )
        .await;
        return crate::kanban_decompose::format_decompose_outcome(&outcome);
    }
    handle_kanban_slash(args, home, notify)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn slash_schedule_and_clear() {
        let dir = TempDir::new().expect("tmpdir");
        fs::create_dir_all(dir.path()).expect("mkdir");
        let created = handle_kanban_slash("create Schedule test", Some(dir.path()), None);
        let id = created
            .split_whitespace()
            .find(|w| w.starts_with("kb-"))
            .expect("task id");
        let scheduled = handle_kanban_slash(
            &format!("schedule {id} --at 2099-01-01T12:00:00Z"),
            Some(dir.path()),
            None,
        );
        assert!(scheduled.contains("Scheduled"));
        let cleared = handle_kanban_slash(&format!("schedule {id} --clear"), Some(dir.path()), None);
        assert!(cleared.contains("Cleared"));
    }
}
