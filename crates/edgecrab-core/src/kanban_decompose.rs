//! Kanban decomposer — Hermes `kanban_decompose.py` profile routing parity.

use std::path::Path;
use std::sync::Arc;

use edgecrab_state::{KanbanDb, KanbanDecomposeChild, KanbanStatus, kanban_db_path_for_board, list_board_slugs};
use edgequake_llm::LLMProvider;
use serde::Deserialize;
use serde_json::Value;

use crate::auxiliary_model::resolve_side_task_provider_and_model;
use crate::config::{AppConfig, KanbanDecomposerConfig};
use crate::kanban_profiles::{
    format_roster_for_prompt, install_root, list_profile_roster, normalize_assignee_choice,
    resolve_default_assignee, resolve_orchestrator_profile, valid_assignee_names,
};

const SYSTEM_PROMPT: &str = r#"You are the Kanban decomposer for EdgeCrab.

A user dropped a rough idea into the Triage column. Break it into a small graph of concrete child tasks and route each one to the best-matching profile from the available roster.

You will be given:
  - The original task title and body
  - The list of available profiles (each with name + description)
  - The fallback "default_assignee" used when no profile fits

Output a single JSON object:

Fan-out (2–6 parallel/sequenced tasks):
{
  "fanout": true,
  "rationale": "<one sentence>",
  "tasks": [
    {
      "title": "<imperative title, <= 80 chars>",
      "body": "<detailed spec for the worker>",
      "assignee": "<profile name from the roster, or null for default>",
      "parents": [<int>, ...]
    }
  ]
}

Single task (no useful decomposition):
{
  "fanout": false,
  "rationale": "<one sentence>",
  "title": "<tightened title>",
  "body": "<concrete spec>",
  "assignee": "<profile name from the roster, or null for default>"
}

Rules:
- "parents" are 0-based indices into the same "tasks" list (data dependencies).
- Prefer parallelism — independent tasks get empty parents.
- Pick assignees from the roster by matching DESCRIPTION (not just name).
- Each child body must stand alone for a fresh worker.
- No preamble, no code fences — JSON only.
"#;

#[derive(Debug, Clone)]
pub struct DecomposeOutcome {
    pub task_id: String,
    pub ok: bool,
    pub reason: String,
    pub fanout: bool,
    pub child_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct FanoutResponse {
    #[serde(default)]
    fanout: bool,
    #[serde(default)]
    rationale: String,
    #[serde(default)]
    tasks: Vec<LlmTask>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LlmTask {
    title: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    parents: Vec<usize>,
}

struct DecomposeContext {
    default_assignee: String,
    orchestrator: String,
    valid_names: std::collections::HashSet<String>,
}

fn decompose_context(app_cfg: &AppConfig, install_root: &Path) -> DecomposeContext {
    DecomposeContext {
        default_assignee: resolve_default_assignee(&app_cfg.kanban, install_root),
        orchestrator: resolve_orchestrator_profile(&app_cfg.kanban, install_root),
        valid_names: valid_assignee_names(install_root),
    }
}

fn extract_json_object(raw: &str) -> Option<Value> {
    let stripped = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let start = stripped.find('{')?;
    let end = stripped.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&stripped[start..=end]).ok()
}

fn llm_tasks_to_children(tasks: Vec<LlmTask>, ctx: &DecomposeContext) -> Vec<KanbanDecomposeChild> {
    tasks
        .into_iter()
        .map(|t| KanbanDecomposeChild {
            title: t.title.trim().chars().take(200).collect(),
            body: t.body.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()),
            assignee: Some(normalize_assignee_choice(
                t.assignee.as_deref(),
                &ctx.default_assignee,
                &ctx.valid_names,
            )),
            parents: t.parents,
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn decompose_task(
    db: &KanbanDb,
    task_id: &str,
    provider: Arc<dyn LLMProvider>,
    main_model: &str,
    decomposer_cfg: &KanbanDecomposerConfig,
    auxiliary_model: Option<&str>,
    app_cfg: &AppConfig,
    install_root: &Path,
) -> DecomposeOutcome {
    let ctx = decompose_context(app_cfg, install_root);
    let roster = list_profile_roster(install_root);

    let task = match db.get_task(task_id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: "task not found".into(),
                fanout: false,
                child_ids: None,
            };
        }
        Err(e) => {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("db error: {e}"),
                fanout: false,
                child_ids: None,
            };
        }
    };

    if task.status != "triage" {
        return DecomposeOutcome {
            task_id: task_id.to_string(),
            ok: false,
            reason: "task is not in triage".into(),
            fanout: false,
            child_ids: None,
        };
    }

    let (side_provider, _side_model) = resolve_side_task_provider_and_model(
        decomposer_cfg.model.as_deref(),
        auxiliary_model,
        provider,
        main_model,
        "kanban decomposer",
    );

    let body = task.body.as_deref().unwrap_or("(empty)");
    let user = format!(
        "Task id: {}\nTitle: {}\nBody:\n{body}\n\n\
         Available profiles (assignees you may pick from):\n{}\n\n\
         Default assignee (used when no profile fits a task): {}",
        task.id,
        task.title,
        format_roster_for_prompt(&roster),
        ctx.default_assignee,
    );
    let messages = vec![
        edgequake_llm::ChatMessage::system(SYSTEM_PROMPT),
        edgequake_llm::ChatMessage::user(&user),
    ];
    let options = edgequake_llm::CompletionOptions {
        max_tokens: Some(decomposer_cfg.max_tokens as usize),
        temperature: Some(0.3),
        ..Default::default()
    };

    let raw = match side_provider
        .chat_with_tools(&messages, &[], None, Some(&options))
        .await
    {
        Ok(r) => r.content,
        Err(e) => {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("LLM error: {e}"),
                fanout: false,
                child_ids: None,
            };
        }
    };

    let Some(val) = extract_json_object(&raw) else {
        return DecomposeOutcome {
            task_id: task_id.to_string(),
            ok: false,
            reason: "failed to parse decomposer JSON".into(),
            fanout: false,
            child_ids: None,
        };
    };

    let parsed: FanoutResponse = match serde_json::from_value(val) {
        Ok(p) => p,
        Err(e) => {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("invalid decomposer shape: {e}"),
                fanout: false,
                child_ids: None,
            };
        }
    };

    if !parsed.fanout {
        let title = parsed
            .title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| task.title.clone());
        let body = parsed.body.as_deref().or(task.body.as_deref());
        let assignee = if task.assignee.is_some() {
            None
        } else {
            Some(normalize_assignee_choice(
                parsed.assignee.as_deref(),
                &ctx.default_assignee,
                &ctx.valid_names,
            ))
        };
        match db.specify_triage_task(
            task_id,
            &title,
            body,
            assignee.as_deref(),
            Some("decomposer"),
        ) {
            Ok(true) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: true,
                reason: parsed.rationale,
                fanout: false,
                child_ids: None,
            },
            Ok(false) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: "task moved out of triage before specify".into(),
                fanout: false,
                child_ids: None,
            },
            Err(e) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("specify failed: {e}"),
                fanout: false,
                child_ids: None,
            },
        }
    } else {
        let children = llm_tasks_to_children(parsed.tasks, &ctx);
        if children.is_empty() {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: "fanout=true but tasks list empty".into(),
                fanout: true,
                child_ids: None,
            };
        }
        match db.decompose_triage_task(
            task_id,
            Some(&ctx.orchestrator),
            &children,
            Some("decomposer"),
        ) {
            Ok(Some(ids)) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: true,
                reason: format!(
                    "{} — decomposed into {} children",
                    parsed.rationale,
                    ids.len()
                ),
                fanout: true,
                child_ids: Some(ids),
            },
            Ok(None) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: "task moved out of triage before decompose".into(),
                fanout: true,
                child_ids: None,
            },
            Err(e) => DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("decompose rejected: {e}"),
                fanout: true,
                child_ids: None,
            },
        }
    }
}

/// List triage task ids on one board db.
pub fn list_triage_ids(db: &KanbanDb) -> Result<Vec<String>, edgecrab_types::AgentError> {
    Ok(db
        .list_tasks(Some(KanbanStatus::Triage), 200)?
        .into_iter()
        .map(|t| t.id)
        .collect())
}

/// Auto-decompose up to `per_tick` triage tasks across all boards.
pub async fn run_auto_decompose_tick(
    home: &Path,
    provider: Arc<dyn LLMProvider>,
    main_model: &str,
    cfg: &AppConfig,
) -> usize {
    if !cfg.kanban.enabled || !cfg.kanban.auto_decompose {
        return 0;
    }
    let root = install_root();
    let per_tick = cfg.kanban.auto_decompose_per_tick.max(1);
    let mut attempted = 0usize;
    let mut decomposed = 0usize;

    for slug in list_board_slugs(home) {
        if attempted >= per_tick as usize {
            break;
        }
        let path = kanban_db_path_for_board(home, Some(&slug));
        let Ok(db) = KanbanDb::open(&path) else {
            continue;
        };
        let triage_ids = list_triage_ids(&db).unwrap_or_default();
        for task_id in triage_ids {
            if attempted >= per_tick as usize {
                break;
            }
            attempted += 1;
            let outcome = decompose_task(
                &db,
                &task_id,
                provider.clone(),
                main_model,
                &cfg.auxiliary.kanban_decomposer,
                cfg.auxiliary.model.as_deref(),
                cfg,
                &root,
            )
            .await;
            if outcome.ok {
                decomposed += 1;
                tracing::info!(
                    task_id = %outcome.task_id,
                    fanout = outcome.fanout,
                    reason = %outcome.reason,
                    "kanban: auto-decomposed triage task"
                );
            } else {
                tracing::debug!(
                    task_id = %outcome.task_id,
                    reason = %outcome.reason,
                    "kanban: auto-decompose skipped/failed"
                );
            }
        }
    }
    decomposed
}

/// Synchronous decompose for slash commands and HTTP API.
pub async fn decompose_task_by_id(
    home: Option<&Path>,
    task_id: &str,
    provider: Arc<dyn LLMProvider>,
    main_model: &str,
    cfg: &AppConfig,
) -> DecomposeOutcome {
    let home = edgecrab_state::kanban_home(home);
    let path = edgecrab_state::kanban_db_path_for_board(&home, None);
    let root = install_root();
    let db = match KanbanDb::open(&path) {
        Ok(db) => db,
        Err(e) => {
            return DecomposeOutcome {
                task_id: task_id.to_string(),
                ok: false,
                reason: format!("open db: {e}"),
                fanout: false,
                child_ids: None,
            };
        }
    };
    decompose_task(
        &db,
        task_id,
        provider,
        main_model,
        &cfg.auxiliary.kanban_decomposer,
        cfg.auxiliary.model.as_deref(),
        cfg,
        &root,
    )
    .await
}

pub fn decompose_outcome_json(outcome: &DecomposeOutcome) -> serde_json::Value {
    serde_json::json!({
        "ok": outcome.ok,
        "task_id": outcome.task_id,
        "reason": outcome.reason,
        "fanout": outcome.fanout,
        "child_ids": outcome.child_ids,
    })
}

/// Format a decompose outcome for slash command replies.
pub fn format_decompose_outcome(outcome: &DecomposeOutcome) -> String {
    if outcome.ok {
        if outcome.fanout {
            let ids = outcome
                .child_ids
                .as_ref()
                .map(|v| v.join(", "))
                .unwrap_or_default();
            format!("✅ Decomposed `{}` → {ids}\n{}", outcome.task_id, outcome.reason)
        } else {
            format!(
                "✅ Specified `{}` (single task, triage → todo)\n{}",
                outcome.task_id, outcome.reason
            )
        }
    } else {
        format!("❌ Decompose `{}` failed: {}", outcome.task_id, outcome.reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_strips_fences() {
        let raw = "```json\n{\"fanout\": false, \"title\": \"x\"}\n```";
        let v = extract_json_object(raw).expect("json");
        assert_eq!(v["fanout"], false);
    }
}
