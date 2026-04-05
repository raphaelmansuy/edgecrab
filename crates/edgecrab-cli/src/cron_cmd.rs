//! CLI cron commands — thin wrapper over `edgecrab-cron`.
//!
//! All storage, scheduling, and security logic lives in the `edgecrab-cron` crate.
//! This module only handles CLI presentation and agent execution.
//!
//! ## Supported schedule formats (hermes-agent parity)
//!   - `30m` / `2h` / `1d`  — one-shot delay
//!   - `every 30m`           — recurring interval
//!   - `0 9 * * *`           — cron expression
//!   - `2026-03-15T09:00`    — ISO 8601 one-shot
//!
//! ## Recursion guard
//! Cron job sessions use `Platform::Cron`. The `manage_cron_jobs` tool's
//! `check_fn` returns `false` for `Platform::Cron`, so scheduled jobs
//! cannot recursively create or mutate other cron jobs.

use anyhow::Context;
use chrono::Utc;
use edgecrab_cron::{
    CronJob, CronJobBuilder, Deliver, JobState, Origin, SILENT_HINT, SILENT_MARKER, Schedule,
    TickLock, advance_next_run_past_now, advance_pre_exec, create_job, format_ts, load_store,
    mark_job_run, resolve_job_index, save_output, save_store, scan_cron_prompt, schedule_display,
};
use edgecrab_tools::registry::GatewaySender;
use edgecrab_types::Platform;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::cli_args::{CliArgs, CronCommand};
use crate::create_provider;
use crate::gateway_cmd;
use crate::runtime::{
    build_agent, build_tool_registry_with_mcp_discovery, load_runtime, open_state_db,
};

const CRON_SESSION_PREFIX: &str = "cron";
const MAX_PLATFORM_OUTPUT: usize = 4000;
const TRUNCATED_VISIBLE: usize = 3800;

pub async fn run(command: CronCommand, args: &CliArgs) -> anyhow::Result<()> {
    match command {
        CronCommand::List { all } => list_cmd(all),
        CronCommand::Status => status_cmd(),
        CronCommand::Tick => {
            tick_due_jobs(args, true, None, None).await?;
            Ok(())
        }
        CronCommand::Create {
            schedule,
            prompt,
            name,
            skills,
            repeat,
            deliver,
        } => {
            let prompt_text = prompt.join(" ");
            create_cmd(
                &schedule,
                &prompt_text,
                name.as_deref(),
                &skills,
                repeat,
                deliver.as_deref(),
            )
        }
        CronCommand::Edit {
            id,
            schedule,
            prompt,
            name,
            skills,
            add_skills,
            remove_skills,
            clear_skills,
            deliver,
        } => edit_cmd(
            &id,
            schedule.as_deref(),
            prompt.as_deref(),
            name.as_deref(),
            &skills,
            &add_skills,
            &remove_skills,
            clear_skills,
            deliver.as_deref(),
        ),
        CronCommand::Pause { id } => pause_cmd(&id),
        CronCommand::Resume { id } => resume_cmd(&id),
        CronCommand::Run { id } => run_job_cmd(&id, args).await,
        CronCommand::Remove { id } => remove_cmd(&id),
    }
}

// ─── Status snapshot (used by gateway_cmd and status_cmd) ─────────────

pub struct CronStatus {
    pub total_jobs: usize,
    pub active_jobs: usize,
    pub paused_jobs: usize,
    pub next_run_at: Option<i64>,
}

pub fn status_snapshot() -> anyhow::Result<CronStatus> {
    let store = load_store()?;
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
    let next_run_at = store
        .jobs
        .iter()
        .filter(|j| j.enabled && j.state == JobState::Scheduled)
        .filter_map(|j| j.next_run_at.as_deref())
        .filter_map(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc).timestamp())
        .min();

    Ok(CronStatus {
        total_jobs: store.jobs.len(),
        active_jobs: active,
        paused_jobs: paused,
        next_run_at,
    })
}

// ─── Tick ─────────────────────────────────────────────────────────────

pub async fn tick_due_jobs(
    args: &CliArgs,
    verbose: bool,
    sender: Option<Arc<dyn GatewaySender>>,
    tui_tx: Option<mpsc::UnboundedSender<String>>,
) -> anyhow::Result<usize> {
    // File-based lock: only one tick runs at a time across all processes.
    let _lock = match TickLock::try_acquire()? {
        Some(lock) => lock,
        None => {
            if verbose {
                println!("Another scheduler tick is already running — skipping.");
            }
            return Ok(0);
        }
    };

    let mut store = load_store()?;
    let now = Utc::now();

    // ── Phase 1: Collect due jobs; fast-forward stale recurring jobs ──────────
    //
    // Hermes-agent parity: for recurring jobs that are past their grace window
    // (e.g. the machine was offline for hours), fast-forward `next_run_at` to
    // the next future slot and skip this tick.  This prevents a burst of missed
    // runs firing all at once after a gateway restart.
    //
    // Grace window scales with schedule period: daily → 2 h, hourly → 30 m,
    // 10-min job → 5 m.  Defined in `edgecrab_cron::advance_next_run_past_now`.
    let mut has_stale = false;
    let mut due_indices: Vec<usize> = Vec::new();

    for (i, job) in store.jobs.iter_mut().enumerate() {
        if !job.is_due(&now) {
            continue;
        }

        // For recurring schedules: check whether the missed window is still
        // within the catch-up grace period.  Outside grace → fast-forward.
        let stale = match &job.schedule {
            Schedule::Interval { .. } | Schedule::Cron { .. } => {
                if let Some(next_ts) = job.next_run_at.as_deref() {
                    match advance_next_run_past_now(&job.schedule, next_ts) {
                        Some(ref new_next) if new_next != next_ts => {
                            // Outside grace window — advance and skip.
                            tracing::info!(
                                job_id = %job.id,
                                job_name = %job.name,
                                new_next = %new_next,
                                "cron: stale job fast-forwarded (missed grace window)"
                            );
                            job.next_run_at = Some(new_next.clone());
                            has_stale = true;
                            true
                        }
                        _ => false, // within grace window — fire now
                    }
                } else {
                    false
                }
            }
            Schedule::Once { .. } => false, // one-shots always fire if due
        };

        if !stale {
            due_indices.push(i);
        }
    }

    // Persist stale advances so the next tick picks up the correct next_run_at.
    if has_stale {
        save_store(&mut store)?;
    }

    if due_indices.is_empty() {
        if verbose {
            if has_stale {
                println!("Stale job(s) fast-forwarded. No other jobs due.");
            } else {
                println!("No due cron jobs.");
            }
        }
        return Ok(0);
    }

    // ── Phase 2: Pre-advance recurring jobs BEFORE execution ─────────────────
    //
    // Hermes-agent parity (`advance_next_run`): update `next_run_at` to the
    // *next* future occurrence before calling the agent so that if the process
    // crashes mid-execution the job won't re-fire on restart.  This converts
    // the scheduler to at-most-once semantics for recurring jobs — missing one
    // run is far better than firing unlimited duplicates in a crash loop.
    //
    // One-shot jobs are intentionally excluded: they should retry on restart
    // until they succeed at least once.
    for &idx in &due_indices {
        if advance_pre_exec(&mut store.jobs[idx]) {
            tracing::debug!(
                job_id = %store.jobs[idx].id,
                job_name = %store.jobs[idx].name,
                new_next = ?store.jobs[idx].next_run_at,
                "cron: pre-advanced next_run_at before execution"
            );
        }
    }
    // Persist pre-advances BEFORE executing — this is the crash-safety save.
    save_store(&mut store)?;

    // Clone job snapshots for execution (store may be mutated below).
    let mut ran = 0usize;
    let jobs_to_run: Vec<CronJob> = due_indices.iter().map(|&i| store.jobs[i].clone()).collect();

    for (job, idx) in jobs_to_run.iter().zip(due_indices.iter()) {
        match execute_job(job, args).await {
            Ok(response) => {
                if verbose {
                    println!("Ran {} ({})", job.id_short(), job.name);
                }
                // Save full output regardless of SILENT marker
                let output_path = save_output(job, &response).ok();
                let silent = response.trim_start().starts_with(SILENT_MARKER);
                if verbose && silent {
                    tracing::debug!("Job '{}' reported [SILENT] — no delivery.", job.id);
                }
                // Deliver to external platform (hermes parity)
                if !silent {
                    if let Err(e) = deliver_cron_output(
                        job,
                        &response,
                        output_path.as_deref(),
                        sender.as_deref(),
                    )
                    .await
                    {
                        tracing::warn!(
                            job = %job.id,
                            error = %e,
                            "cron delivery failed — output saved locally"
                        );
                    }
                }
                // Notify the TUI of completion so the user sees output
                // in the chat window, regardless of the deliver= setting.
                //
                // First principles: when running inside the TUI, the user
                // created this cron here — they should always see the full
                // response inline. The deliver= setting only controls whether
                // the result is ALSO forwarded to an external platform
                // (Telegram, Discord, etc.). It must never suppress TUI output.
                if let Some(ref tx) = tui_tx {
                    let tui_msg = if silent {
                        format!("**⏰ Cron job ran:** {}", job.name)
                    } else {
                        // Always show the full response in the TUI chat.
                        format!("**⏰ Cron: {}**\n\n{}", job.name, response)
                    };
                    let _ = tx.send(tui_msg);
                }
                store.jobs[*idx].last_error = None;
                let _keep = mark_job_run(&mut store.jobs[*idx], true, None);
                ran += 1;
            }
            Err(e) => {
                if verbose {
                    eprintln!("Failed {} ({}): {e}", job.id_short(), job.name);
                }
                // Notify TUI of failure
                if let Some(ref tx) = tui_tx {
                    let _ = tx.send(format!("**⏰ Cron job failed:** {} — {}", job.name, e));
                }
                let _ = mark_job_run(&mut store.jobs[*idx], false, Some(&e.to_string()));
            }
        }
    }

    // Remove fully-completed jobs (repeat-exhausted, one-shot done)
    store.jobs.retain(|j| j.state != JobState::Completed);
    save_store(&mut store)?;
    Ok(ran)
}

// ─── create ───────────────────────────────────────────────────────────

fn create_cmd(
    schedule: &str,
    prompt: &str,
    name: Option<&str>,
    skills: &[String],
    repeat: Option<u32>,
    deliver: Option<&str>,
) -> anyhow::Result<()> {
    let text = create_job_text(schedule, prompt, name, skills, repeat, deliver)?;
    println!("{text}");
    Ok(())
}

/// Create a cron job and return a markdown confirmation string.
///
/// Shared by CLI (`create_cmd`) and TUI (`/cron add`).
pub fn create_job_text(
    schedule: &str,
    prompt: &str,
    name: Option<&str>,
    skills: &[String],
    repeat: Option<u32>,
    deliver: Option<&str>,
) -> anyhow::Result<String> {
    if prompt.trim().is_empty() && skills.is_empty() {
        anyhow::bail!("cron create requires either a prompt or at least one --skill");
    }
    if !prompt.trim().is_empty() {
        scan_cron_prompt(prompt).map_err(|e| anyhow::anyhow!(e))?;
    }

    let mut builder = CronJobBuilder::new(schedule, prompt);
    if let Some(n) = name {
        builder = builder.name(n);
    }
    if !skills.is_empty() {
        builder = builder.skills(skills.to_vec());
    }
    if let Some(r) = repeat {
        builder = builder.repeat(r);
    }
    if let Some(d) = deliver {
        builder = builder.deliver(d);
    }

    let job = create_job(builder)?;
    let next_rel = job
        .next_run_at
        .as_deref()
        .map(relative_time)
        .unwrap_or_else(|| "-".into());
    let next_abs = job
        .next_run_at
        .as_deref()
        .map(|s| format!("  *({})*", format_ts(Some(s))))
        .unwrap_or_default();
    let d_icon = deliver_icon(&job.deliver);

    let mut lines = vec![
        format!("✅ **Created** — **{}**  `{}`", job.name, job.id_short()),
        String::new(),
        format!(
            "**Schedule:** `{}`    **First run:** {}{next_abs}",
            job.schedule_display, next_rel
        ),
        format!(
            "**Deliver:** {d_icon} `{}`    **Repeat:** {}",
            job.deliver,
            job.repeat.display()
        ),
    ];
    if !job.skills.is_empty() {
        let codes: Vec<String> = job.skills.iter().map(|s| format!("`{s}`")).collect();
        lines.push(format!("**Skills:** {}", codes.join(" ")));
    }
    if !prompt.is_empty() {
        lines.push(String::new());
        lines.push(format!("> {}", truncate(prompt, 100)));
    }
    Ok(lines.join("\n"))
}

// ─── edit ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn edit_cmd(
    id: &str,
    schedule: Option<&str>,
    prompt: Option<&str>,
    name: Option<&str>,
    skills: &[String], // --skill (replace-all; overrides add/remove/clear when non-empty)
    add_skills: &[String],
    remove_skills: &[String],
    clear_skills: bool,
    deliver: Option<&str>,
) -> anyhow::Result<()> {
    let text = edit_job_text(
        id,
        schedule,
        prompt,
        name,
        skills,
        add_skills,
        remove_skills,
        clear_skills,
        deliver,
    )?;
    println!("{text}");
    Ok(())
}

/// Edit a cron job and return a human-readable summary string.
///
/// Skill precedence: `--skill` (replace-all) > `--clear-skills` > `--add/remove-skill`.
/// Shared by CLI (`edit_cmd`) and TUI (`/cron edit`).
#[allow(clippy::too_many_arguments)]
pub fn edit_job_text(
    id: &str,
    schedule: Option<&str>,
    prompt: Option<&str>,
    name: Option<&str>,
    skills: &[String],
    add_skills: &[String],
    remove_skills: &[String],
    clear_skills: bool,
    deliver: Option<&str>,
) -> anyhow::Result<String> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = &mut store.jobs[idx];

    if let Some(sched) = schedule {
        let parsed = edgecrab_cron::parse_schedule(sched)
            .with_context(|| format!("invalid schedule '{sched}'"))?;
        job.schedule_display = schedule_display(&parsed);
        if job.state != JobState::Paused {
            job.next_run_at = edgecrab_cron::compute_next_run(&parsed, None);
        }
        job.schedule = parsed;
    }
    if let Some(p) = prompt {
        if !p.trim().is_empty() {
            scan_cron_prompt(p).map_err(|e| anyhow::anyhow!(e))?;
        }
        job.prompt = p.to_string();
    }
    if let Some(n) = name {
        job.name = n.to_string();
    }
    if let Some(d) = deliver {
        job.deliver = d.to_string();
    }

    // Skill management: --skill replaces all; else --clear/add/remove apply
    if !skills.is_empty() {
        job.skills = skills.to_vec();
    } else if clear_skills {
        job.skills.clear();
    } else {
        for s in remove_skills {
            job.skills.retain(|existing| existing != s);
        }
        for s in add_skills {
            let s = s.trim().to_string();
            if !s.is_empty() && !job.skills.contains(&s) {
                job.skills.push(s);
            }
        }
    }

    if job.enabled && job.state != JobState::Paused && job.next_run_at.is_none() {
        job.next_run_at = edgecrab_cron::compute_next_run(&job.schedule, None);
    }

    job.updated_at = Utc::now().to_rfc3339();
    let next_rel = job
        .next_run_at
        .as_deref()
        .map(relative_time)
        .unwrap_or_else(|| "-".into());
    let next_abs = job
        .next_run_at
        .as_deref()
        .map(|s| format!("  *({})*", format_ts(Some(s))))
        .unwrap_or_default();
    let summary = format!(
        "✏ **Updated** — **{}**  `{}`\n\n**Next run:** {next_rel}{next_abs}",
        job.id_short(),
        job.name
    );
    save_store(&mut store)?;
    Ok(summary)
}

// ─── pause / resume ───────────────────────────────────────────────────

fn pause_cmd(id: &str) -> anyhow::Result<()> {
    println!("{}", pause_job_text(id)?);
    Ok(())
}

/// Pause a job and return a markdown confirmation. Shared by CLI and TUI.
pub fn pause_job_text(id: &str) -> anyhow::Result<String> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = &mut store.jobs[idx];
    job.enabled = false;
    job.state = JobState::Paused;
    job.next_run_at = None;
    job.paused_at = Some(Utc::now().to_rfc3339());
    job.updated_at = Utc::now().to_rfc3339();
    let msg = format!("⏸ **Paused** — **{}**  `{}`", job.name, job.id_short());
    save_store(&mut store)?;
    Ok(msg)
}

fn resume_cmd(id: &str) -> anyhow::Result<()> {
    println!("{}", resume_job_text(id)?);
    Ok(())
}

/// Resume a paused job and return a markdown confirmation. Shared by CLI and TUI.
pub fn resume_job_text(id: &str) -> anyhow::Result<String> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = &mut store.jobs[idx];
    job.enabled = true;
    job.state = JobState::Scheduled;
    job.paused_at = None;
    job.paused_reason = None;
    job.next_run_at = edgecrab_cron::compute_next_run(&job.schedule, None);
    job.updated_at = Utc::now().to_rfc3339();
    let next_rel = job
        .next_run_at
        .as_deref()
        .map(relative_time)
        .unwrap_or_else(|| "-".into());
    let next_abs = job
        .next_run_at
        .as_deref()
        .map(|s| format!("  *({})*", format_ts(Some(s))))
        .unwrap_or_default();
    let msg = format!(
        "▶ **Resumed** — **{}**  `{}`\n\n**Next run:** {next_rel}{next_abs}",
        job.name,
        job.id_short()
    );
    save_store(&mut store)?;
    Ok(msg)
}

// ─── run (trigger immediately) ────────────────────────────────────────

async fn run_job_cmd(id: &str, args: &CliArgs) -> anyhow::Result<()> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = store.jobs[idx].clone();

    let response = execute_job(&job, args).await?;

    let silent = response.trim_start().starts_with(SILENT_MARKER);
    let _ = save_output(&job, &response);

    mark_job_run(&mut store.jobs[idx], true, None);
    save_store(&mut store)?;

    println!("⚡ Ran  {} [{}]", job.name, job.id_short());
    if silent {
        println!("   Response: [SILENT] — nothing to report.");
    } else {
        println!("   Response: {}", truncate(&response, 200));
    }
    Ok(())
}

/// Advance a job's `next_run_at` to now so the scheduler fires it on the next
/// tick.  Does NOT run the agent immediately — use from TUI or agent `run`
/// action.  Shared by `do_run` (LLM tool) and the TUI `/cron run` handler.
pub fn trigger_job_text(id: &str) -> anyhow::Result<String> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = &mut store.jobs[idx];
    job.enabled = true;
    job.state = JobState::Scheduled;
    job.paused_at = None;
    job.paused_reason = None;
    let name = job.name.clone();
    let short = job.id_short().to_string();
    job.next_run_at = Some(Utc::now().to_rfc3339());
    job.updated_at = Utc::now().to_rfc3339();
    save_store(&mut store)?;
    Ok(format!(
        "⚡ **Queued** — **{name}**  `{short}`\n\nWill fire on the next scheduler tick (~60 seconds)."
    ))
}

// ─── remove ───────────────────────────────────────────────────────────

fn remove_cmd(id: &str) -> anyhow::Result<()> {
    println!("{}", remove_job_text(id)?);
    Ok(())
}

/// Remove a job and return a markdown confirmation. Shared by CLI and TUI.
pub fn remove_job_text(id: &str) -> anyhow::Result<String> {
    let mut store = load_store()?;
    let idx = resolve_job_index(&store.jobs, id)?;
    let job = store.jobs.remove(idx);
    save_store(&mut store)?;
    Ok(format!(
        "🗑 **Removed** — **{}**  `{}`",
        job.name,
        job.id_short()
    ))
}

// ─── list / status ────────────────────────────────────────────────────

fn list_cmd(show_all: bool) -> anyhow::Result<()> {
    print!("{}", list_jobs_text(show_all)?);
    Ok(())
}

/// Format all jobs as a human-readable plain-text string. Used by CLI.
///
/// For TUI, use `list_jobs_md()` which goes through markdown rendering.
pub fn list_jobs_text(show_all: bool) -> anyhow::Result<String> {
    let store = load_store()?;
    let jobs: Vec<&CronJob> = store
        .jobs
        .iter()
        .filter(|j| show_all || (j.enabled && j.state == JobState::Scheduled))
        .collect();

    if jobs.is_empty() {
        let hidden = store.jobs.len();
        if hidden > 0 && !show_all {
            return Ok(format!(
                "No active jobs. ({hidden} paused/completed — run `edgecrab cron list --all` to show all)"
            ));
        }
        return Ok(
            "No scheduled jobs. Create one: edgecrab cron create <schedule> <prompt>".into(),
        );
    }

    let mut out = format!(
        "⏰ Cron Jobs  ·  {} active  ·  {} paused\n{}\n",
        store
            .jobs
            .iter()
            .filter(|j| j.enabled && j.state == JobState::Scheduled)
            .count(),
        store
            .jobs
            .iter()
            .filter(|j| j.state == JobState::Paused)
            .count(),
        "─".repeat(56)
    );

    for job in &jobs {
        let emoji = state_emoji(job);
        let next = if job.state == JobState::Paused {
            "paused".to_string()
        } else {
            job.next_run_at
                .as_deref()
                .map(relative_time)
                .unwrap_or_else(|| "-".to_string())
        };
        out.push_str(&format!(
            "\n{emoji}  {}  [{}]\n    Schedule:  {}    Next: {}\n",
            job.name,
            job.id_short(),
            job.schedule_display,
            next,
        ));
        if !job.skills.is_empty() {
            out.push_str(&format!("    Skills:    {}\n", job.skills.join(", ")));
        }
        out.push_str(&format!(
            "    Deliver:   {}  {}   Runs: {}  ·  {}\n",
            deliver_icon(&job.deliver),
            job.deliver,
            job.run_count,
            job.repeat.display()
        ));
        out.push_str(&format!("    Prompt:    {}\n", truncate(&job.prompt, 80)));
    }
    Ok(out)
}

fn status_cmd() -> anyhow::Result<()> {
    print!("{}", status_text()?);
    Ok(())
}

/// Format cron + gateway status. Shared by CLI and TUI.
pub fn status_text() -> anyhow::Result<String> {
    let status = status_snapshot()?;
    let gateway = gateway_cmd::snapshot()?;
    Ok(format!(
        "Cron jobs:         {}\nActive:            {}\nPaused:            {}\nNext run:          {}\nGateway scheduler: {}",
        status.total_jobs,
        status.active_jobs,
        status.paused_jobs,
        format_ts_i64(status.next_run_at),
        if gateway.running {
            "running"
        } else {
            "stopped"
        }
    ))
}

// ─── TUI Markdown display (OutputRole::Assistant) ─────────────────────

/// Render all cron jobs as markdown — for TUI display.
///
/// Produces headers, code spans, bold, blockquotes that the TUI markdown
/// renderer (`render_markdown`) turns into a fully styled ratatui widget.
pub fn list_jobs_md(show_all: bool) -> anyhow::Result<String> {
    let store = load_store()?;

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

    let jobs: Vec<&CronJob> = store
        .jobs
        .iter()
        .filter(|j| show_all || (j.enabled && j.state == JobState::Scheduled))
        .collect();

    let header = format!("## ⏰ Cron Jobs  ·  **{active}** active  ·  **{paused}** paused");

    if jobs.is_empty() {
        let hint = if total > 0 && !show_all {
            format!(
                "\n*{} paused/completed job(s) hidden — `/cron list --all` to show all.*",
                total
            )
        } else {
            String::new()
        };
        return Ok(format!(
            "{header}\n\nNo active scheduled jobs.{hint}\n\n\
             *Create one:*  `/cron add 30m Check the build status`"
        ));
    }

    let mut out = format!("{header}\n");
    for job in &jobs {
        out.push('\n');
        out.push_str("---\n\n");

        let emoji = state_emoji(job);
        let paused_note = if job.state == JobState::Paused {
            "  *(paused)*"
        } else {
            ""
        };
        out.push_str(&format!(
            "### {emoji}  **{}**  `{}`{paused_note}\n\n",
            job.name,
            job.id_short()
        ));

        // Schedule + next run
        let next = if job.state == JobState::Paused {
            "—".to_string()
        } else {
            job.next_run_at
                .as_deref()
                .map(relative_time)
                .unwrap_or_else(|| "-".to_string())
        };
        let next_abs = if job.state == JobState::Paused {
            String::new()
        } else {
            job.next_run_at
                .as_deref()
                .map(|s| format!("  *({})*", format_ts(Some(s))))
                .unwrap_or_default()
        };
        out.push_str(&format!(
            "**Schedule:** `{}`    **Next:** {}{next_abs}\n",
            job.schedule_display, next
        ));

        // Deliver + skills + model
        let d_icon = deliver_icon(&job.deliver);
        let skills_part = if job.skills.is_empty() {
            String::new()
        } else {
            let codes: Vec<String> = job.skills.iter().map(|s| format!("`{s}`")).collect();
            format!("    ⚙ **Skills:** {}", codes.join(" "))
        };
        let model_part = job
            .model
            .as_deref()
            .map(|m| format!("    🤖 `{m}`"))
            .unwrap_or_default();
        out.push_str(&format!(
            "**Deliver:** {d_icon} `{}`{skills_part}{model_part}\n",
            job.deliver
        ));

        // Runs + repeat
        let repeat_str = job.repeat.display();
        out.push_str(&format!(
            "**Runs:** {}  ·  Repeat: {}\n",
            job.run_count, repeat_str
        ));

        // Prompt preview
        if !job.prompt.is_empty() {
            out.push('\n');
            out.push_str(&format!("> {}\n", truncate(&job.prompt, 120)));
        }

        // Last error
        if let Some(ref err) = job.last_error {
            out.push('\n');
            out.push_str(&format!("> ⚠ Last error: {}\n", truncate(err, 80)));
        }
    }

    out.push_str("\n---\n");
    if !show_all && total > jobs.len() {
        out.push_str(&format!(
            "*{} job(s) hidden. `/cron list --all` to show all.*\n",
            total - jobs.len()
        ));
    } else {
        out.push_str(
            "*`/cron add <schedule> <prompt>` to create  ·  `/cron help` for all commands*\n",
        );
    }
    Ok(out)
}

/// Render cron + gateway status as markdown — for TUI display.
pub fn status_md() -> anyhow::Result<String> {
    let status = status_snapshot()?;
    let gateway = gateway_cmd::snapshot()?;

    let next_str = status
        .next_run_at
        .map(relative_time_from_ts)
        .unwrap_or_else(|| "-".into());
    let next_abs = status
        .next_run_at
        .map(|ts| format!("  *({})*", format_ts_i64(Some(ts))))
        .unwrap_or_default();

    let gw = if gateway.running {
        "✓ **running**".to_string()
    } else {
        "✗ stopped  —  `edgecrab gateway start` to launch".to_string()
    };

    Ok(format!(
        "## ⏰ Cron Scheduler\n\n\
         **Jobs:** {total} total    **Active:** {active}    **Paused:** {paused}\n\
         **Next run:** {next_str}{next_abs}\n\
         **Gateway:** {gw}\n\n\
         ---\n\n{help}",
        total = status.total_jobs,
        active = status.active_jobs,
        paused = status.paused_jobs,
        help = cron_commands_help(),
    ))
}

/// Full markdown help page for the `/cron help` command.
pub fn cron_help_md() -> String {
    "## ⏰ Cron — Command Reference\n\n\
     **Create a job:**\n\
     - `/cron add 30m Check the build`\n\
     - `/cron add every 2h Summarize the news feed`\n\
     - `/cron add 0 9 * * * Daily morning briefing`\n\
     \n\
     **Manage jobs:**\n\
     - `/cron list` — active jobs  ·  `/cron list --all` — all including paused\n\
     - `/cron status` — scheduler + gateway summary\n\
     - `/cron pause <id>` — suspend without deleting\n\
     - `/cron resume <id>` — re-enable and reschedule\n\
     - `/cron run <id>` — trigger on the next scheduler tick (~60s)\n\
     - `/cron remove <id>` — delete permanently\n\
     \n\
     **Schedule formats:**\n\
     - One-shot delay: `30m`  `2h`  `1d`\n\
     - Interval: `every 30m`  `every 2h`  `every 1d`\n\
     - Cron expression: `0 9 * * *`  `0 */6 * * *`  `0 9 * * 1-5`\n\
     - ISO timestamp (one-time): `2026-03-31T09:00:00`\n\
     \n\
     **Delivery targets:**\n\
     \n\
     | Target | Where output goes | Requires |\n\
     |---|---|---|\n\
     | `local` | `~/.edgecrab/cron/output/` *(default)* | — |\n\
     | `origin` | Reply to the chat that created the job | — |\n\
     | `telegram` | Telegram home channel | `TELEGRAM_HOME_CHANNEL` env |\n\
     | `discord` | Discord home channel | `DISCORD_HOME_CHANNEL` env |\n\
     | `slack` | Slack home channel | `SLACK_HOME_CHANNEL` env |\n\
     | `whatsapp` | WhatsApp home channel | `WHATSAPP_HOME_CHANNEL` env |\n\
     | `signal` | Signal home channel | `SIGNAL_HOME_CHANNEL` env |\n\
     | `email` | Email address | `EMAIL_HOME_CHANNEL` env |\n\
     | `sms` | SMS number | `SMS_HOME_CHANNEL` env |\n\
     | `matrix` | Matrix home channel | `MATRIX_HOME_CHANNEL` env |\n\
     | `mattermost` | Mattermost home channel | `MATTERMOST_HOME_CHANNEL` env |\n\
     | `telegram:-100123456` | Specific Telegram chat | — |\n\
     | `discord:987654321` | Specific Discord channel | — |\n\
     \n\
     **Create job with delivery target:**\n\
     - `/cron add --deliver telegram 0 9 * * * Check HN for AI news`\n\
     - `/cron add -d discord every 2h Server health check`\n\
     - `/cron add --deliver origin 30m Quick build check`\n\
     - `/cron add --deliver telegram:-100123456 0 8 * * * Daily report`\n\
     \n\
     *Note: Cron sessions cannot create more cron jobs (recursion guard).*"
        .to_string()
}

fn cron_commands_help() -> &'static str {
    "**Quick reference:**\n\
     - `/cron list` — active jobs  ·  `/cron list --all` — all\n\
     - `/cron add <schedule> <prompt>` — create a job\n\
     - `/cron pause <id>`  ·  `/cron resume <id>`\n\
     - `/cron run <id>` — trigger  ·  `/cron remove <id>` — delete\n\
     - `/cron help` — full command reference\n\
     \n\
     *Schedule examples:*  `30m`  `every 2h`  `0 9 * * *`  `2026-03-31T09:00`"
}

// ─── Agent execution ─────────────────────────────────────────────────

async fn execute_job(job: &CronJob, args: &CliArgs) -> anyhow::Result<String> {
    // Per-job model override or fall back to runtime config
    let model_override = job.model.as_deref().or(args.model.as_deref());

    let runtime = load_runtime(
        args.config.as_deref(),
        model_override,
        args.toolset.as_deref(),
    )?;
    let provider = create_provider(&runtime.config.model.default_model);
    let state_db = open_state_db(&runtime.state_db_path)?;
    let tool_registry = build_tool_registry_with_mcp_discovery(&runtime.config).await;

    // Build cron job prompt: inject SILENT hint + optional skill preamble + user prompt
    let effective_prompt = build_cron_prompt(job);

    let agent = build_agent(
        &runtime,
        provider,
        state_db,
        tool_registry,
        // Platform::Cron → disables manage_cron_jobs via check_fn (recursion guard)
        Platform::Cron,
        true, // quiet mode
        Some(format!("{CRON_SESSION_PREFIX}-{}", job.id)),
    )?;

    agent
        .chat(&effective_prompt)
        .await
        .with_context(|| format!("failed to run cron job '{}'", job.name))
}

/// Build the effective prompt — SILENT hint + skills preamble + user prompt.
fn build_cron_prompt(job: &CronJob) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Prepend SILENT hint so the agent can suppress empty delivery
    parts.push(SILENT_HINT.to_string());

    // Load skills in order (best-effort — missing skills emit a warning)
    for skill_name in &job.skills {
        let skill_path = skill_path(skill_name);
        match std::fs::read_to_string(&skill_path) {
            Ok(content) => {
                parts.push(format!(
                    "[SYSTEM: The user has invoked the \"{skill_name}\" skill. \
                     Full skill content loaded below.]\n\n{content}"
                ));
            }
            Err(_) => {
                parts.push(format!(
                    "[SYSTEM: Skill \"{skill_name}\" was listed for this job but could not be found. \
                     Start your response with a notice: \
                     '⚠️ Skill not found and skipped: {skill_name}']"
                ));
                tracing::warn!(skill = %skill_name, job = %job.id, "skill not found for cron job");
            }
        }
    }

    // Append the job prompt
    if !job.prompt.is_empty() {
        if job.skills.is_empty() {
            parts.push(job.prompt.clone());
        } else {
            parts.push(format!(
                "The user has provided the following instruction alongside the skill invocation: {}",
                job.prompt
            ));
        }
    }

    parts.join("\n\n")
}

/// Resolve the filesystem path for a skill's SKILL.md.
fn skill_path(name: &str) -> std::path::PathBuf {
    let home = std::env::var("EDGECRAB_HOME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|dir| dir.join(".edgecrab")))
        .unwrap_or_default();
    home.join("skills").join(name).join("SKILL.md")
}

// ─── Cron delivery ───────────────────────────────────────────────────

/// Deliver cron job output to the configured external target.
///
/// WHY separate async fn: Delivery errors must NOT abort the tick loop
/// (output is already saved locally). We log warnings and continue.
async fn deliver_cron_output(
    job: &CronJob,
    content: &str,
    saved_output_path: Option<&std::path::Path>,
    sender: Option<&dyn GatewaySender>,
) -> anyhow::Result<()> {
    let Some(sender) = sender else {
        return Ok(()); // No gateway sender — local-only mode
    };

    let deliver: Deliver = job.deliver.parse().unwrap_or_default();
    let Some((platform, recipient)) = resolve_delivery_target(&deliver, job.origin.as_ref()) else {
        return Ok(()); // Deliver::Local or unresolvable origin
    };

    let wrapped = format_delivery_response(&job.name, content, saved_output_path);
    sender
        .send_message(&platform, &recipient, &wrapped)
        .await
        .map_err(|e| anyhow::anyhow!("delivery to {platform}:{recipient} failed: {e}"))
}

/// Resolve the `(platform, chat_id)` delivery tuple from a job's delivery config.
///
/// Returns `None` for `Deliver::Local` (no network delivery) or when
/// `Deliver::Origin` has no origin info.
fn resolve_delivery_target(deliver: &Deliver, origin: Option<&Origin>) -> Option<(String, String)> {
    match deliver {
        Deliver::Local => None,
        Deliver::Origin => origin.map(|o| {
            (
                o.platform.clone(),
                delivery_recipient(&o.chat_id, o.thread_id.as_deref()),
            )
        }),
        Deliver::Platform(name) => {
            if let Some(origin) = origin.filter(|origin| origin.platform.eq_ignore_ascii_case(name))
            {
                return Some((
                    name.clone(),
                    delivery_recipient(&origin.chat_id, origin.thread_id.as_deref()),
                ));
            }
            Some((name.clone(), String::new()))
        }
        Deliver::Explicit(platform, chat_id) => Some((platform.clone(), chat_id.clone())),
    }
}

/// Wrap cron job output in the hermes-style delivery envelope.
fn format_delivery_response(
    job_name: &str,
    content: &str,
    saved_output_path: Option<&std::path::Path>,
) -> String {
    let visible_content = if content.len() > MAX_PLATFORM_OUTPUT {
        let suffix = match saved_output_path {
            Some(path) => format!(
                "\n\n... [truncated, full output saved to {}]",
                path.display()
            ),
            None => "\n\n... [truncated, full output saved locally]".to_string(),
        };
        format!(
            "{}{}",
            edgecrab_core::safe_truncate(content, TRUNCATED_VISIBLE),
            suffix
        )
    } else {
        content.to_string()
    };
    let separator = "─".repeat(job_name.len() + 18);
    format!(
        "Cronjob Response: {job_name}\n{separator}\n\n{content}\n\n\
         Note: The agent cannot see this message — it was sent automatically by the cron scheduler.",
        content = visible_content,
    )
}

fn delivery_recipient(chat_id: &str, thread_id: Option<&str>) -> String {
    match thread_id.filter(|thread_id| !thread_id.is_empty()) {
        Some(thread_id) => format!("{chat_id}:{thread_id}"),
        None => chat_id.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn resolve_delivery_target_keeps_origin_thread_suffix() {
        let origin = Origin {
            platform: "telegram".into(),
            chat_id: "-100123".into(),
            chat_name: None,
            thread_id: Some("17".into()),
        };

        let target = resolve_delivery_target(&Deliver::Origin, Some(&origin)).expect("target");
        assert_eq!(target, ("telegram".into(), "-100123:17".into()));
    }

    #[test]
    fn resolve_platform_delivery_uses_empty_recipient_for_home_channel() {
        let target =
            resolve_delivery_target(&Deliver::Platform("telegram".into()), None).expect("target");
        assert_eq!(target, ("telegram".into(), String::new()));
    }

    #[test]
    fn format_delivery_response_truncates_large_outputs() {
        let content = "x".repeat(MAX_PLATFORM_OUTPUT + 500);
        let rendered = format_delivery_response(
            "Nightly report",
            &content,
            Some(std::path::Path::new("/tmp/full-output.md")),
        );

        assert!(rendered.contains("[truncated, full output saved to /tmp/full-output.md]"));
        assert!(rendered.len() < content.len());
    }
}

// ─── Display helpers ──────────────────────────────────────────────────

/// Human-friendly relative time from an RFC3339 string.
///
/// Examples: "in 6h 32m", "just now", "3d ago", "overdue 5m"
pub fn relative_time(rfc3339: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(rfc3339) else {
        return "-".to_string();
    };
    let diff_secs = dt
        .with_timezone(&Utc)
        .signed_duration_since(Utc::now())
        .num_seconds();
    if diff_secs.abs() < 90 {
        return "just now".to_string();
    }
    let abs = diff_secs.unsigned_abs();
    let mins = abs / 60;
    let hours = abs / 3600;
    let days = abs / 86400;
    let display = if days >= 2 {
        format!("{days}d")
    } else if hours >= 1 {
        let rem = mins % 60;
        if rem > 0 {
            format!("{hours}h {rem}m")
        } else {
            format!("{hours}h")
        }
    } else {
        format!("{mins}m")
    };
    if diff_secs >= 0 {
        format!("in {display}")
    } else {
        format!("overdue {display}")
    }
}

fn relative_time_from_ts(ts_secs: i64) -> String {
    chrono::DateTime::<Utc>::from_timestamp(ts_secs, 0)
        .map(|dt| relative_time(&dt.to_rfc3339()))
        .unwrap_or_else(|| "-".into())
}

fn state_emoji(job: &CronJob) -> &'static str {
    if job.state == JobState::Paused {
        "⏸"
    } else if job.state == JobState::Completed {
        "✅"
    } else if job.enabled {
        "🟢"
    } else {
        "⭕"
    }
}

fn deliver_icon(deliver: &str) -> &'static str {
    let d = deliver.split(':').next().unwrap_or(deliver);
    match d {
        "" | "local" => "💾",
        "origin" => "↩",
        "telegram" => "📱",
        "discord" => "🎮",
        "slack" => "💬",
        "whatsapp" => "📲",
        "signal" => "🔒",
        "email" => "📧",
        "sms" => "📟",
        "matrix" => "🔵",
        "mattermost" => "🔵",
        "dingtalk" => "🔔",
        "homeassistant" => "🏠",
        _ => "✉",
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────

fn truncate(text: &str, limit: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= limit {
        text.to_string()
    } else {
        chars[..limit].iter().collect::<String>() + "…"
    }
}

fn format_ts_i64(ts: Option<i64>) -> String {
    ts.and_then(|v| chrono::DateTime::<Utc>::from_timestamp(v, 0))
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".into())
}
