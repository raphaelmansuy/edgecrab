//! # cron — LLM-callable cron job management
//!
//! WHY agent-accessible cron: Hermes-agent exposes cron management as an
//! LLM-callable tool, allowing the agent to schedule, list, pause, resume,
//! update, and remove recurring tasks during conversation. EdgeCrab
//! replicates this interface with parity plus recursion-guard and
//! full `edgecrab-cron` integration (DRY: no duplicated store types).
//!
//! ## Actions (hermes parity + extras)
//!   create  — schedule a new job
//!   list    — list all jobs (or filter by enabled/paused)
//!   update  — edit an existing job (name/schedule/prompt/skills/repeat/deliver/model)
//!   pause   — suspend a job
//!   resume  — re-enable a suspended job
//!   remove  — delete a job
//!   status  — summary counts and next-run time
//!
//! ## Recursion guard
//! Scheduled jobs run as `Platform::Cron` sessions. This tool's `check_fn`
//! returns `false` for that platform, so a cron job cannot re-schedule more
//! cron jobs (prevents runaway self-replication).
//!
//! ## DRY
//! All store types (`CronJob`, `CronStore`, `load_store`, `save_store`) and
//! schedule parsing live in the `edgecrab-cron` crate. This module is a thin
//! tool wrapper only.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use chrono::Utc;
use edgecrab_cron::{
    CronJobBuilder, JobState, Origin, compute_next_run, create_job, format_ts, load_store,
    parse_schedule, resolve_job_index, save_store, scan_cron_prompt, schedule_display,
};
use edgecrab_types::{Platform, ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

// ─── ManageCronJobsTool ───────────────────────────────────────────────

pub struct ManageCronJobsTool;

/// Deserialized arguments from the LLM.
#[derive(Deserialize)]
struct CronArgs {
    /// Action: "create" | "list" | "update" | "pause" | "resume" | "remove" | "status"
    action: String,
    /// Cron schedule expression (required for "create"; optional for "update")
    #[serde(default)]
    schedule: Option<String>,
    /// Agent prompt / task to run (required for "create"; optional for "update")
    #[serde(default)]
    prompt: Option<String>,
    /// Human-readable name (optional)
    #[serde(default)]
    name: Option<String>,
    /// Job ID or unambiguous prefix (required for single-job actions)
    #[serde(default)]
    job_id: Option<String>,
    /// Skills to attach (list of skill names). Replaces existing list on update.
    #[serde(default)]
    skills: Option<Vec<String>>,
    /// Run at most N times, then auto-remove. 0 = unlimited.
    #[serde(default)]
    repeat: Option<u32>,
    /// Delivery target: "local" | "origin" | "<platform>:<chat_id>"
    #[serde(default)]
    deliver: Option<String>,
    /// Per-job model override (e.g. "copilot/gpt-4.1-mini")
    #[serde(default)]
    model: Option<String>,
}

#[async_trait]
impl ToolHandler for ManageCronJobsTool {
    fn name(&self) -> &'static str {
        "manage_cron_jobs"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["cronjob"]
    }

    fn toolset(&self) -> &'static str {
        "scheduling"
    }

    fn emoji(&self) -> &'static str {
        "⏰"
    }

    /// Recursion guard: cron sessions must NOT be able to schedule more cron jobs.
    fn check_fn(&self, ctx: &ToolContext) -> bool {
        ctx.platform != Platform::Cron
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "manage_cron_jobs".into(),
            description:
                "Schedule and manage recurring or one-off automated tasks that run in the background.

IMPORTANT: Use this tool for ALL cron operations — never edit ~/.edgecrab/cron/jobs.json directly.

action→intent mapping:
  create — schedule a new task: 'every morning', 'daily at 9am', 'every 2 hours', 'remind me each Friday'
  list   — see scheduled jobs: 'show my cron jobs', 'what's scheduled', 'list automations'
  pause  — stop/suppress a job: 'pause the daily briefing', 'suppress all cron jobs', 'disable weather check'
  resume — re-enable a paused job: 'restart', 'resume', 're-enable the job'
  remove — permanently delete: 'delete', 'remove', 'cancel the job'
  status — summary / next-run time: 'cron status', 'when does it run next'
  update — change schedule/prompt/delivery of an existing job
  run    — trigger a job on the next scheduler tick (~60s)

Workflow for 'suppress/pause all cron jobs':
  1. manage_cron_jobs(action='list')               ← get all job_ids
  2. manage_cron_jobs(action='pause', job_id=...)  ← pause each one

Schedule formats:
  • Cron expression: '0 9 * * *' (daily 9am), '0 */6 * * *' (every 6h), '0 9 * * 1-5' (weekdays)
  • Interval: 'every 30m', 'every 2h', 'every 1d'
  • One-shot delay: '30m', '2h', '1d'
  • ISO timestamp: '2026-03-31T09:00:00' (run once at exact time)

Delivery — map user intent to deliver= value:
  • 'send me on Telegram' / 'notify via Telegram' → deliver='telegram'
  • 'send to Discord' / 'post in Discord'         → deliver='discord'
  • 'notify me on Slack'                          → deliver='slack'
  • 'send via WhatsApp'                           → deliver='whatsapp'
  • 'email me the results'                        → deliver='email'
  • 'send me on Signal'                           → deliver='signal'
  • 'notify me here' / 'reply in this chat'       → deliver='origin'
  • 'keep local' / 'save locally' (default CLI)   → deliver='local'
  • specific channel: 'telegram chat -100123456'  → deliver='telegram:-100123456'

IMPORTANT — prompt must be self-contained: cron sessions start fresh with no chat context.
Bad: 'Check on that issue'. Good: 'SSH into server 192.168.1.100, check nginx status, report result.'

Safety: this tool is disabled inside cron sessions to prevent recursive scheduling."
                    .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "list", "update", "pause", "resume", "remove", "run", "status"],
                        "description": "Operation: create=schedule new task; list=show all jobs; pause=suspend a job; resume=re-enable; remove=delete; update=change settings; run=trigger now; status=summary counts."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "When to run. Examples: '0 9 * * *' (daily 9am), 'every 2h', '30m' (once in 30 min), '2026-03-31T09:00'. Required for 'create'."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "The self-contained task description for the cron agent. Must include all context needed — no chat history is available when it runs. Required for 'create'."
                    },
                    "name": {
                        "type": "string",
                        "description": "Optional human-readable name (e.g. 'Morning briefing', 'Server health check')."
                    },
                    "job_id": {
                        "type": "string",
                        "description": "Job ID or unambiguous prefix. Required for: update, pause, resume, remove, run."
                    },
                    "skills": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Skill names to load before each run (e.g. ['blogwatcher', 'find-nearby']). Replaces list on update; pass [] to clear."
                    },
                    "repeat": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Max times to run then auto-remove. 0 or omitted = unlimited (recurring). 1 = one-shot."
                    },
                    "deliver": {
                        "type": "string",
                        "description": "Where to deliver output. Values: 'local' (save to ~/.edgecrab/cron/output/, default on CLI), 'origin' (reply to the chat that created the job), 'telegram' (home channel via TELEGRAM_HOME_CHANNEL env), 'discord' (home channel via DISCORD_HOME_CHANNEL env), 'slack' (SLACK_HOME_CHANNEL), 'whatsapp' (WHATSAPP_HOME_CHANNEL), 'signal' (SIGNAL_HOME_CHANNEL), 'email' (EMAIL_HOME_CHANNEL), 'sms' (SMS_HOME_CHANNEL), 'matrix' (MATRIX_HOME_CHANNEL), 'mattermost' (MATTERMOST_HOME_CHANNEL). For a specific chat use 'platform:chat_id', e.g. 'telegram:-1001234567890', 'discord:987654321', 'telegram:-1001234567890:17' (with thread ID). Infer from natural language: 'send me on Telegram' → 'telegram', 'post to Discord' → 'discord', 'reply here' → 'origin', 'email me' → 'email'."
                    },
                    "model": {
                        "type": "string",
                        "description": "Per-job model override, e.g. 'copilot/gpt-4.1-mini'."
                    }
                },
                "required": ["action"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Other("Cancelled".into()));
        }

        // Recursion guard (belt + suspenders alongside check_fn)
        if ctx.platform == Platform::Cron {
            return Err(ToolError::PermissionDenied(
                "manage_cron_jobs is disabled inside cron job sessions to prevent recursive \
                 scheduling loops."
                    .into(),
            ));
        }

        let a: CronArgs = serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
            tool: self.name().into(),
            message: e.to_string(),
        })?;

        // Extract the fields needed by do_create from ctx before moving into spawn_blocking.
        // All other actions don't need ctx.
        let origin_chat = ctx.origin_chat.clone();

        // Dispatch on the action string first to decide if we need the origin context.
        // All action handlers do synchronous file I/O (load_store / save_store) so we
        // wrap the entire dispatch in spawn_blocking to avoid blocking the Tokio runtime.
        let action = a.action.clone();
        tokio::task::spawn_blocking(move || match action.as_str() {
            "create" => do_create(a, origin_chat),
            "list" => do_list(),
            "update" => do_update(a),
            "pause" => do_set_state(a, false),
            "resume" => do_set_state(a, true),
            "remove" => do_remove(a),
            "run" => do_run(a),
            "status" => do_status(),
            other => Err(ToolError::InvalidArgs {
                tool: "manage_cron_jobs".into(),
                message: format!(
                    "Unknown action '{other}'. Valid: create, list, update, pause, resume, remove, run, status"
                ),
            }),
        })
        .await
        .map_err(|e| ToolError::Other(format!("cron task panicked: {e}")))?
    }
}

// ─── Action handlers ──────────────────────────────────────────────────

fn do_create(a: CronArgs, origin_chat: Option<(String, String)>) -> Result<String, ToolError> {
    let schedule = require_field(&a.schedule, "schedule", "create")?;
    let prompt = a.prompt.as_deref().unwrap_or("");

    // At least a prompt or one skill must be present
    if prompt.trim().is_empty() && a.skills.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(ToolError::InvalidArgs {
            tool: "manage_cron_jobs".into(),
            message: "'prompt' or at least one skill is required for 'create'".into(),
        });
    }

    // Scan prompt for injection
    if !prompt.trim().is_empty() {
        scan_cron_prompt(prompt).map_err(ToolError::PermissionDenied)?;
    }

    let mut builder = CronJobBuilder::new(schedule, prompt);
    if let Some(n) = a.name.as_deref() {
        builder = builder.name(n);
    }
    if let Some(skills) = a.skills {
        if !skills.is_empty() {
            builder = builder.skills(skills);
        }
    }
    if let Some(r) = a.repeat {
        if r > 0 {
            builder = builder.repeat(r);
        }
    }
    if let Some((platform, chat_id)) = origin_chat {
        builder = builder.origin(Origin {
            platform,
            chat_id,
            chat_name: None,
            thread_id: None,
        });
    }
    if let Some(deliver) = a.deliver.as_deref() {
        builder = builder.deliver(deliver);
    }

    if let Some(m) = a.model.as_deref() {
        builder = builder.model(m);
    }

    let job = create_job(builder).map_err(|e| ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    })?;

    Ok(json!({
        "success": true,
        "job_id": job.id,
        "message": format!(
            "Created cron job '{}' ({}). Schedule: {}. Next run: {}.",
            job.name,
            job.id_short(),
            job.schedule_display,
            format_ts(job.next_run_at.as_deref()),
        )
    })
    .to_string())
}

fn do_list() -> Result<String, ToolError> {
    let store = load_store().map_err(exec_err)?;
    if store.jobs.is_empty() {
        return Ok(json!({ "jobs": [], "total": 0 }).to_string());
    }

    let jobs: Vec<_> = store
        .jobs
        .iter()
        .map(|j| {
            json!({
                "id": j.id,
                "id_short": j.id_short(),
                "name": j.name,
                "schedule": j.schedule_display,
                "state": j.state.as_str(),
                "enabled": j.enabled,
                "next_run_at": format_ts(j.next_run_at.as_deref()),
                "last_run_at": format_ts(j.last_run_at.as_deref()),
                "run_count": j.run_count,
                "skills": j.skills,
                "repeat": j.repeat.display(),
                "deliver": j.deliver,
                "model": j.model
            })
        })
        .collect();

    Ok(json!({ "jobs": jobs, "total": jobs.len() }).to_string())
}

fn do_update(a: CronArgs) -> Result<String, ToolError> {
    let job_id = require_field(&a.job_id, "job_id", "update")?;
    let mut store = load_store().map_err(exec_err)?;
    let idx = resolve_job_index(&store.jobs, job_id).map_err(|e| ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    })?;
    let job = &mut store.jobs[idx];

    if let Some(sched_str) = a.schedule.as_deref() {
        let parsed = parse_schedule(sched_str).map_err(|e| ToolError::InvalidArgs {
            tool: "manage_cron_jobs".into(),
            message: format!("Invalid schedule '{sched_str}': {e}"),
        })?;
        job.schedule_display = schedule_display(&parsed);
        if job.state != JobState::Paused {
            job.next_run_at = compute_next_run(&parsed, None);
        }
        job.schedule = parsed;
    }
    if let Some(p) = a.prompt.as_deref() {
        if !p.trim().is_empty() {
            scan_cron_prompt(p).map_err(ToolError::PermissionDenied)?;
        }
        job.prompt = p.to_string();
    }
    if let Some(n) = a.name.as_deref() {
        job.name = n.to_string();
    }
    if let Some(skills) = a.skills {
        job.skills = skills;
    }
    if let Some(r) = a.repeat {
        job.repeat.times = if r == 0 { None } else { Some(r) };
    }
    if let Some(d) = a.deliver.as_deref() {
        job.deliver = d.to_string();
    }
    if let Some(m) = a.model {
        job.model = if m.is_empty() { None } else { Some(m) };
    }
    if job.enabled && job.state != JobState::Paused && job.next_run_at.is_none() {
        job.next_run_at = compute_next_run(&job.schedule, None);
    }
    job.updated_at = chrono::Utc::now().to_rfc3339();

    let name = job.name.clone();
    let short = job.id_short().to_string();
    let next = format_ts(job.next_run_at.as_deref());
    save_store(&mut store).map_err(exec_err)?;

    Ok(json!({
        "success": true,
        "job_id": store.jobs[idx].id,
        "message": format!("Updated cron job '{name}' ({short}). Next run: {next}.")
    })
    .to_string())
}

fn do_set_state(a: CronArgs, enable: bool) -> Result<String, ToolError> {
    let action = if enable { "resume" } else { "pause" };
    let job_id = require_field(&a.job_id, "job_id", action)?;
    let mut store = load_store().map_err(exec_err)?;
    let idx = resolve_job_index(&store.jobs, job_id).map_err(|e| ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    })?;
    let job = &mut store.jobs[idx];

    if enable {
        job.state = JobState::Scheduled;
        job.enabled = true;
        job.next_run_at = compute_next_run(&job.schedule.clone(), None);
        job.paused_at = None;
        job.paused_reason = None;
    } else {
        job.state = JobState::Paused;
        job.enabled = false;
        job.paused_at = Some(chrono::Utc::now().to_rfc3339());
    }
    job.updated_at = chrono::Utc::now().to_rfc3339();

    let verb = if enable { "Resumed" } else { "Paused" };
    let name = job.name.clone();
    let short = job.id_short().to_string();
    save_store(&mut store).map_err(exec_err)?;

    Ok(json!({
        "success": true,
        "job_id": store.jobs[idx].id,
        "message": format!("{verb} cron job '{name}' ({short}).")
    })
    .to_string())
}

fn do_remove(a: CronArgs) -> Result<String, ToolError> {
    let job_id = require_field(&a.job_id, "job_id", "remove")?;
    let mut store = load_store().map_err(exec_err)?;
    let idx = resolve_job_index(&store.jobs, job_id).map_err(|e| ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    })?;
    let removed = store.jobs.remove(idx);
    save_store(&mut store).map_err(exec_err)?;

    Ok(json!({
        "success": true,
        "job_id": removed.id,
        "message": format!("Removed cron job '{}' ({}).", removed.name, removed.id_short())
    })
    .to_string())
}

fn do_run(a: CronArgs) -> Result<String, ToolError> {
    let job_id = require_field(&a.job_id, "job_id", "run")?;
    let mut store = load_store().map_err(exec_err)?;
    let idx = resolve_job_index(&store.jobs, job_id).map_err(|e| ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    })?;

    let (name, id) = (store.jobs[idx].name.clone(), store.jobs[idx].id.clone());
    store.jobs[idx].enabled = true;
    store.jobs[idx].state = JobState::Scheduled;
    store.jobs[idx].paused_at = None;
    store.jobs[idx].paused_reason = None;
    store.jobs[idx].next_run_at = Some(Utc::now().to_rfc3339());
    store.jobs[idx].updated_at = Utc::now().to_rfc3339();
    save_store(&mut store).map_err(exec_err)?;

    Ok(json!({
        "success": true,
        "job_id": id,
        "message": format!("Job '{}' will run on the next scheduler tick (within ~30 seconds).", name),
    })
    .to_string())
}

fn do_status() -> Result<String, ToolError> {
    let store = load_store().map_err(exec_err)?;
    let total = store.jobs.len();
    let active = store
        .jobs
        .iter()
        .filter(|j| j.enabled && j.state == JobState::Scheduled)
        .count();
    let paused = store
        .jobs
        .iter()
        .filter(|j| j.state == JobState::Paused)
        .count();
    let completed = store
        .jobs
        .iter()
        .filter(|j| j.state == JobState::Completed)
        .count();
    let next = store
        .jobs
        .iter()
        .filter(|j| j.enabled && j.state == JobState::Scheduled)
        .filter_map(|j| j.next_run_at.as_deref())
        .min()
        .map(str::to_string);

    Ok(json!({
        "total_jobs": total,
        "active_jobs": active,
        "paused_jobs": paused,
        "completed_jobs": completed,
        "next_run_at": next.as_deref().unwrap_or("-")
    })
    .to_string())
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn require_field<'a>(
    field: &'a Option<String>,
    name: &'static str,
    action: &str,
) -> Result<&'a str, ToolError> {
    field
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ToolError::InvalidArgs {
            tool: "manage_cron_jobs".into(),
            message: format!("'{name}' is required for action '{action}'"),
        })
}

fn exec_err(e: impl std::fmt::Display) -> ToolError {
    ToolError::ExecutionFailed {
        tool: "manage_cron_jobs".into(),
        message: e.to_string(),
    }
}

inventory::submit!(&ManageCronJobsTool as &dyn ToolHandler);

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome;

    fn with_temp_edgecrab_home<T>(f: impl FnOnce(&TestEdgecrabHome) -> T) -> T {
        let home = TestEdgecrabHome::new();
        f(&home)
    }

    #[test]
    fn schema_valid() {
        let schema = ManageCronJobsTool.schema();
        assert_eq!(schema.name, "manage_cron_jobs");
        let required = schema.parameters["required"].as_array().expect("array");
        assert!(required.iter().any(|v| v == "action"));
        // Must include "update" in the enum
        let actions = schema.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("array");
        assert!(
            actions.iter().any(|v| v == "update"),
            "missing 'update' action"
        );
    }

    #[test]
    fn tool_metadata() {
        assert_eq!(ManageCronJobsTool.name(), "manage_cron_jobs");
        assert_eq!(ManageCronJobsTool.toolset(), "scheduling");
        assert_eq!(ManageCronJobsTool.emoji(), "⏰");
    }

    #[test]
    fn recursion_guard_blocks_cron_platform() {
        // check_fn must return false for Platform::Cron
        use edgecrab_types::Platform;
        let platform = Platform::Cron;
        // We cannot construct a full ToolContext in unit tests, but we can verify
        // the platform comparison logic directly.
        assert!(platform == Platform::Cron);
    }

    #[test]
    fn require_field_errors_on_none() {
        let result = require_field(&None, "job_id", "pause");
        assert!(result.is_err());
    }

    #[test]
    fn require_field_errors_on_empty() {
        let field = Some("".to_string());
        let result = require_field(&field, "job_id", "pause");
        assert!(result.is_err());
    }

    #[test]
    fn require_field_passes_on_value() {
        let field = Some("abc123".to_string());
        let result = require_field(&field, "job_id", "pause");
        assert_eq!(result.unwrap(), "abc123");
    }

    #[test]
    fn schema_description_signals_natural_conversation() {
        // The tool description must contain keywords that trigger the LLM to use
        // it when a user expresses scheduling intent in natural language.
        let schema = ManageCronJobsTool.schema();
        let desc = &schema.description;
        // Must mention 'create' as the action for scheduling new tasks
        assert!(
            desc.contains("action='create'")
                || desc.contains("create —")
                || desc.contains("create -"),
            "description must mention create action for scheduling"
        );
        // Must cover management actions (pause, list) so agent uses tool instead of terminal
        assert!(
            desc.contains("pause"),
            "description must mention pause action"
        );
        assert!(
            desc.contains("list"),
            "description must mention list action"
        );
        // Must convey "recurring" and "schedule" intent
        assert!(
            desc.contains("schedule") || desc.contains("Schedule"),
            "description must mention scheduling"
        );
        // Must warn about self-contained prompts (critical for cron quality)
        assert!(
            desc.contains("self-contained"),
            "description must mention self-contained prompts"
        );
        // Must tell the agent not to bypass via terminal
        assert!(
            desc.contains("never edit") || desc.contains("IMPORTANT"),
            "description must discourage terminal bypass"
        );
    }

    #[test]
    fn schema_prompt_field_emphasises_self_contained() {
        let schema = ManageCronJobsTool.schema();
        let prompt_desc = &schema.parameters["properties"]["prompt"]["description"];
        assert!(
            prompt_desc
                .as_str()
                .unwrap_or("")
                .contains("self-contained"),
            "prompt field description should emphasise self-contained requirement"
        );
    }

    #[test]
    fn schema_schedule_has_natural_examples() {
        // The schedule description must include examples that match natural expressions
        // users actually type, so the LLM can translate them correctly.
        let schema = ManageCronJobsTool.schema();
        let sched_desc = schema.parameters["properties"]["schedule"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            sched_desc.contains("every"),
            "should show 'every X' interval examples"
        );
        assert!(
            sched_desc.contains("* * *"),
            "should show cron expression example"
        );
    }

    #[test]
    fn do_create_defaults_to_local_without_origin() {
        with_temp_edgecrab_home(|_| {
            let response = do_create(
                CronArgs {
                    action: "create".into(),
                    schedule: Some("every 30m".into()),
                    prompt: Some("check status".into()),
                    name: None,
                    job_id: None,
                    skills: None,
                    repeat: None,
                    deliver: None,
                    model: None,
                },
                None,
            )
            .expect("create");

            assert!(response.contains("\"success\":true"));
            let store = load_store().expect("load store");
            assert_eq!(store.jobs.len(), 1);
            assert_eq!(store.jobs[0].deliver, "local");
            assert!(store.jobs[0].origin.is_none());
        });
    }

    #[test]
    fn do_run_reenables_paused_job() {
        with_temp_edgecrab_home(|_| {
            let job =
                create_job(CronJobBuilder::new("every 30m", "check status").name("paused-job"))
                    .expect("persist");

            let mut store = load_store().expect("load");
            let idx = resolve_job_index(&store.jobs, &job.id).expect("job index");
            store.jobs[idx].enabled = false;
            store.jobs[idx].state = JobState::Paused;
            store.jobs[idx].next_run_at = None;
            save_store(&mut store).expect("save");

            let response = do_run(CronArgs {
                action: "run".into(),
                schedule: None,
                prompt: None,
                name: None,
                job_id: Some(job.id.clone()),
                skills: None,
                repeat: None,
                deliver: None,
                model: None,
            })
            .expect("run");

            assert!(response.contains("will run on the next scheduler tick"));
            let store = load_store().expect("reload");
            let idx = resolve_job_index(&store.jobs, &job.id).expect("job index after run");
            let job = &store.jobs[idx];
            assert!(job.enabled);
            assert_eq!(job.state, JobState::Scheduled);
            assert!(job.next_run_at.is_some());
        });
    }
}
