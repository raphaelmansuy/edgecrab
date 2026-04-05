//! Cron job persistent storage.
//!
//! Jobs are stored in `~/.edgecrab/cron/jobs.json` with:
//!   - Atomic writes (tempfile + rename) — no partial writes
//!   - Owner-only file permissions (0600)
//!   - Output saved to `~/.edgecrab/cron/output/{job_id}/{timestamp}.md`
//!
//! This module is the **single source of truth** for the cron store format,
//! shared by both the CLI (`cron_cmd.rs`) and the LLM tool (`tools/cron.rs`).

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::schedule::{Schedule, compute_next_run, schedule_display};
use crate::time::{edgecrab_home_dir, to_user_timezone};

// ─── Delivery target ─────────────────────────────────────────────────

/// Where cron job output is delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Deliver {
    /// Write output to `~/.edgecrab/cron/output/` only.
    #[default]
    Local,
    /// Reply to the platform/chat where the job was created.
    Origin,
    /// A named platform's home channel (e.g. `"telegram"`, `"discord"`).
    Platform(String),
    /// An explicit platform + chat ID (`"telegram:123456"`).
    Explicit(String, String),
}

impl Deliver {
    pub fn to_string_repr(&self) -> String {
        match self {
            Deliver::Local => "local".into(),
            Deliver::Origin => "origin".into(),
            Deliver::Platform(p) => p.clone(),
            Deliver::Explicit(p, c) => format!("{p}:{c}"),
        }
    }
}

impl std::str::FromStr for Deliver {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "local" | "" => Deliver::Local,
            "origin" => Deliver::Origin,
            other => {
                if let Some((platform, chat)) = other.split_once(':') {
                    Deliver::Explicit(platform.into(), chat.into())
                } else {
                    Deliver::Platform(other.into())
                }
            }
        })
    }
}

/// Origin info — the platform/chat where the job was created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    pub platform: String,
    pub chat_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

// ─── Repeat counter ──────────────────────────────────────────────────

/// Repeat/run-count configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepeatConfig {
    /// `None` = run forever; `Some(n)` = run at most `n` times.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub times: Option<u32>,
    /// How many times this job has already run.
    pub completed: u32,
}

impl RepeatConfig {
    pub fn forever() -> Self {
        Self {
            times: None,
            completed: 0,
        }
    }
    pub fn once() -> Self {
        Self {
            times: Some(1),
            completed: 0,
        }
    }
    pub fn n_times(n: u32) -> Self {
        Self {
            times: Some(n),
            completed: 0,
        }
    }
    pub fn display(&self) -> String {
        match self.times {
            None => "forever".into(),
            Some(1) => {
                if self.completed >= 1 {
                    "1/1".into()
                } else {
                    "once".into()
                }
            }
            Some(n) => format!("{}/{n}", self.completed),
        }
    }
    pub fn exhausted(&self) -> bool {
        self.times.is_some_and(|n| self.completed >= n)
    }
}

// ─── Job state ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    #[default]
    Scheduled,
    Paused,
    Completed,
    Error,
}

impl JobState {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Scheduled => "scheduled",
            JobState::Paused => "paused",
            JobState::Completed => "completed",
            JobState::Error => "error",
        }
    }
}

// ─── CronJob ─────────────────────────────────────────────────────────

/// A single scheduled cron job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub prompt: String,

    // Schedule
    pub schedule: Schedule,
    /// Human-readable schedule (e.g. `"every 2h"`, `"0 9 * * *"`).
    pub schedule_display: String,

    // Skills — ordered list loaded before the prompt runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,

    // Optional per-job model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    // State
    pub state: JobState,
    pub enabled: bool,
    pub repeat: RepeatConfig,

    // Delivery
    pub deliver: String, // stored as string for forward compat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,

    // Timing
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,

    // Results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub run_count: u64,

    // Pause metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paused_reason: Option<String>,
}

impl CronJob {
    /// First 8 characters of the ID for display.
    pub fn id_short(&self) -> &str {
        &self.id[..self.id.len().min(8)]
    }

    /// True if the job should fire at or before `now`.
    pub fn is_due(&self, now: &DateTime<Utc>) -> bool {
        if self.state == JobState::Paused || !self.enabled {
            return false;
        }
        self.next_run_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .is_some_and(|dt| dt.with_timezone(&Utc) <= *now)
    }
}

// ─── CronStore ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronStore {
    pub jobs: Vec<CronJob>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// ─── Builder ─────────────────────────────────────────────────────────

/// Builder for `CronJob` — validates inputs before creating.
pub struct CronJobBuilder {
    schedule_str: String,
    prompt: String,
    name: Option<String>,
    skills: Vec<String>,
    repeat: Option<u32>,
    deliver: String,
    origin: Option<Origin>,
    model: Option<String>,
}

impl CronJobBuilder {
    pub fn new(schedule_str: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            schedule_str: schedule_str.into(),
            prompt: prompt.into(),
            name: None,
            skills: Vec::new(),
            repeat: None,
            deliver: String::new(),
            origin: None,
            model: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn skills(mut self, skills: Vec<String>) -> Self {
        self.skills = skills;
        self
    }

    pub fn repeat(mut self, times: u32) -> Self {
        self.repeat = Some(times);
        self
    }

    pub fn deliver(mut self, deliver: impl Into<String>) -> Self {
        self.deliver = deliver.into();
        self
    }

    pub fn origin(mut self, origin: Origin) -> Self {
        self.origin = Some(origin);
        self
    }

    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn build(self) -> anyhow::Result<CronJob> {
        let schedule = crate::schedule::parse_schedule(&self.schedule_str)
            .with_context(|| format!("invalid schedule '{}'", self.schedule_str))?;

        let display = schedule_display(&schedule);
        let now_str = Utc::now().to_rfc3339();
        let next_run_at = compute_next_run(&schedule, None);

        let repeat = match (&schedule, self.repeat) {
            // One-shot by default runs once
            (Schedule::Once { .. }, None) => RepeatConfig::once(),
            (_, Some(0)) => RepeatConfig::forever(),
            (_, Some(n)) => RepeatConfig::n_times(n),
            (_, None) => RepeatConfig::forever(),
        };

        let label = self.name.clone().unwrap_or_else(|| {
            let src = if !self.prompt.is_empty() {
                &self.prompt
            } else if let Some(s) = self.skills.first() {
                s.as_str()
            } else {
                "cron job"
            };
            src.chars().take(50).collect()
        });
        let deliver = if self.deliver.trim().is_empty() {
            if self.origin.is_some() {
                "origin".to_string()
            } else {
                "local".to_string()
            }
        } else {
            self.deliver.trim().to_string()
        };

        Ok(CronJob {
            id: Uuid::new_v4().to_string(),
            name: label,
            prompt: self.prompt,
            schedule,
            schedule_display: display,
            skills: self.skills,
            model: self.model.filter(|s| !s.is_empty()),
            state: JobState::Scheduled,
            enabled: true,
            repeat,
            deliver,
            origin: self.origin,
            created_at: now_str.clone(),
            updated_at: now_str,
            next_run_at,
            last_run_at: None,
            last_status: None,
            last_error: None,
            run_count: 0,
            paused_at: None,
            paused_reason: None,
        })
    }
}

// ─── IO helpers ──────────────────────────────────────────────────────

pub fn cron_dir() -> anyhow::Result<PathBuf> {
    let dir = edgecrab_home_dir()?.join("cron");
    std::fs::create_dir_all(&dir)?;
    set_dir_permissions(&dir);
    Ok(dir)
}

pub fn jobs_file() -> anyhow::Result<PathBuf> {
    Ok(cron_dir()?.join("jobs.json"))
}

pub fn output_dir() -> anyhow::Result<PathBuf> {
    let dir = cron_dir()?.join("output");
    std::fs::create_dir_all(&dir)?;
    set_dir_permissions(&dir);
    Ok(dir)
}

/// Atomic write: write to a tempfile in the same directory, then rename.
fn atomic_write(path: &PathBuf, data: &str) -> anyhow::Result<()> {
    let parent = path.parent().context("jobs file has no parent dir")?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data.as_bytes())?;
    tmp.flush()?;
    // Secure the temp file before rename
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o600));
    }
    tmp.persist(path)
        .with_context(|| format!("failed to atomically replace {}", path.display()))?;
    // Ensure final file is owner-only
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn set_dir_permissions(path: &PathBuf) {
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
    }
}

// ─── Load / Save ─────────────────────────────────────────────────────

pub fn load_store() -> anyhow::Result<CronStore> {
    let path = jobs_file()?;
    if !path.exists() {
        return Ok(CronStore::default());
    }
    let data = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let raw: Value = serde_json::from_str(&data)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    let normalized = normalize_store_value(raw);
    serde_json::from_value(normalized)
        .with_context(|| format!("failed to deserialize {}", path.display()))
}

pub fn save_store(store: &mut CronStore) -> anyhow::Result<()> {
    store.updated_at = Some(Utc::now().to_rfc3339());
    let data = serde_json::to_string_pretty(store)?;
    let path = jobs_file()?;
    atomic_write(&path, &data)
}

// ─── Tick-lock ───────────────────────────────────────────────────────

/// File-based advisory lock preventing two concurrent scheduler ticks.
pub struct TickLock {
    _file: std::fs::File,
}

impl TickLock {
    /// Try to acquire the tick lock (non-blocking).
    ///
    /// Returns `None` if another tick is already running.
    pub fn try_acquire() -> anyhow::Result<Option<Self>> {
        let lock_path = cron_dir()?.join(".tick.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("cannot open lock file {}", lock_path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = file.as_raw_fd();
            let ret = unsafe { libc_flock(fd, 2 | 4) }; // LOCK_EX | LOCK_NB
            if ret != 0 {
                return Ok(None); // another tick is running
            }
        }
        #[cfg(not(unix))]
        {
            // Windows: best-effort using file creation (no fcntl)
            // If the file already exists and is locked, we proceed anyway
            // (single-process model on Windows is fine for now).
        }

        Ok(Some(Self { _file: file }))
    }
}

#[cfg(unix)]
unsafe fn libc_flock(fd: std::os::unix::io::RawFd, op: i32) -> i32 {
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    unsafe { flock(fd, op) }
}

// ─── CRUD helpers ────────────────────────────────────────────────────

/// Create a new job in the store.
pub fn create_job(builder: CronJobBuilder) -> anyhow::Result<CronJob> {
    let job = builder.build()?;
    let mut store = load_store()?;
    store.jobs.push(job.clone());
    save_store(&mut store)?;
    Ok(job)
}

/// Find a job index by exact ID or unambiguous prefix.
pub fn resolve_job_index(jobs: &[CronJob], id: &str) -> anyhow::Result<usize> {
    let matches: Vec<usize> = jobs
        .iter()
        .enumerate()
        .filter_map(|(i, j)| (j.id == id || j.id.starts_with(id)).then_some(i))
        .collect();
    match matches.as_slice() {
        [] => bail!("no cron job matching '{id}'"),
        [i] => Ok(*i),
        _ => bail!("ambiguous cron job prefix '{id}' — be more specific"),
    }
}

/// Format an RFC 3339 timestamp for CLI display (`YYYY-MM-DD HH:MM` local time).
pub fn format_ts(ts: Option<&str>) -> String {
    ts.and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| {
            to_user_timezone(dt.with_timezone(&Utc))
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "-".into())
}

/// Pre-advance `next_run_at` for a **recurring** job BEFORE execution.
///
/// WHY (hermes-agent parity): If the host process crashes during job execution,
/// the scheduler would re-fire the same tick on restart because `next_run_at`
/// still points to the previous slot.  By advancing it to the *next* future
/// occurrence **before** calling the agent, we convert the scheduler to
/// at-most-once semantics for recurring jobs — missing one run is far better
/// than firing a burst on every restart.
///
/// One-shot jobs (`Schedule::Once`) are intentionally **not** advanced: they
/// should retry on restart until they succeed at least once.
///
/// Returns `true` if `next_run_at` was changed (i.e. the job is recurring and
/// the timestamp actually moved forward).
pub fn advance_pre_exec(job: &mut CronJob) -> bool {
    match &job.schedule {
        Schedule::Interval { .. } | Schedule::Cron { .. } => {
            let from = Utc::now().to_rfc3339();
            let new_next = compute_next_run(&job.schedule.clone(), Some(&from));
            if new_next != job.next_run_at {
                job.next_run_at = new_next;
                true
            } else {
                false
            }
        }
        Schedule::Once { .. } => false,
    }
}

/// Mark a job as having been executed: update counters, compute next run.
/// Returns `true` if the job should be kept, `false` if it exhausted its repeat limit.
pub fn mark_job_run(job: &mut CronJob, success: bool, error: Option<&str>) -> bool {
    let now = Utc::now().to_rfc3339();
    job.last_run_at = Some(now.clone());
    job.run_count += 1;
    job.repeat.completed += 1;
    job.last_status = Some(if success { "ok" } else { "error" }.into());
    job.last_error = error.map(str::to_string);
    job.updated_at = now.clone();

    if job.repeat.exhausted() {
        job.enabled = false;
        job.state = JobState::Completed;
        job.next_run_at = None;
        return false; // caller may remove the job
    }

    // Compute next run
    job.next_run_at = compute_next_run(&job.schedule, Some(&now));
    if job.next_run_at.is_none() {
        // One-shot fully consumed
        job.enabled = false;
        job.state = JobState::Completed;
        return false;
    }
    job.state = JobState::Scheduled;
    true
}

/// Save cron job output to `~/.edgecrab/cron/output/{job_id}/{timestamp}.md`.
pub fn save_output(job: &CronJob, content: &str) -> anyhow::Result<PathBuf> {
    let dir = output_dir()?.join(&job.id);
    std::fs::create_dir_all(&dir)?;
    set_dir_permissions(&dir);
    let ts = Utc::now().format("%Y%m%d_%H%M%S");
    let path = dir.join(format!("{ts}.md"));
    let ran_at = Utc::now().to_rfc3339();
    let doc = format!(
        "# Cron job output: {name}\n\n\
         **Job ID:** {id}  \n\
         **Schedule:** {schedule}  \n\
         **Ran at:** {ran_at}  \n\
         **Deliver:** {deliver}  \n\n\
         ## Prompt\n\n{prompt}\n\n\
         ## Output\n\n{content}\n",
        name = job.name,
        id = job.id,
        schedule = job.schedule_display,
        deliver = job.deliver,
        prompt = job.prompt,
    );
    atomic_write(&path, &doc)?;
    Ok(path)
}

fn normalize_store_value(mut raw: Value) -> Value {
    let Some(root) = raw.as_object_mut() else {
        return json!({ "jobs": [] });
    };

    let jobs = root.entry("jobs").or_insert_with(|| json!([]));
    if let Some(entries) = jobs.as_array_mut() {
        for entry in entries {
            normalize_job_value(entry);
        }
    }

    raw
}

fn normalize_job_value(job: &mut Value) {
    let Some(obj) = job.as_object_mut() else {
        return;
    };

    let schedule = obj
        .get("schedule")
        .cloned()
        .and_then(|value| serde_json::from_value::<Schedule>(value).ok());

    if !obj.contains_key("skills") {
        let skills = match obj.get("skill").and_then(Value::as_str).map(str::trim) {
            Some(skill) if !skill.is_empty() => json!([skill]),
            _ => json!([]),
        };
        obj.insert("skills".into(), skills);
    } else if let Some(raw_skills) = obj.get("skills").cloned() {
        let normalized = match raw_skills {
            Value::String(skill) => {
                let trimmed = skill.trim();
                if trimmed.is_empty() {
                    json!([])
                } else {
                    json!([trimmed])
                }
            }
            Value::Array(items) => {
                let mut unique: Vec<String> = Vec::new();
                for item in items {
                    let text = match item {
                        Value::String(text) => text.trim().to_string(),
                        other => other.to_string().trim_matches('"').trim().to_string(),
                    };
                    if !text.is_empty() && !unique.contains(&text) {
                        unique.push(text);
                    }
                }
                json!(unique)
            }
            _ => json!([]),
        };
        obj.insert("skills".into(), normalized);
    }

    if !obj.contains_key("schedule_display") {
        if let Some(schedule) = &schedule {
            obj.insert(
                "schedule_display".into(),
                Value::String(schedule_display(schedule)),
            );
        }
    }

    if !obj.contains_key("repeat") {
        let repeat = match schedule {
            Some(Schedule::Once { .. }) => json!({ "times": 1, "completed": 0 }),
            _ => json!({ "times": Value::Null, "completed": 0 }),
        };
        obj.insert("repeat".into(), repeat);
    }

    if !obj.contains_key("deliver") {
        let deliver = if obj.get("origin").is_some_and(|origin| !origin.is_null()) {
            "origin"
        } else {
            "local"
        };
        obj.insert("deliver".into(), Value::String(deliver.to_string()));
    }

    if !obj.contains_key("enabled") {
        obj.insert("enabled".into(), Value::Bool(true));
    }

    if !obj.contains_key("state") {
        let state = if obj.get("enabled").and_then(Value::as_bool) == Some(false) {
            "paused"
        } else if obj.get("next_run_at").is_none() || obj.get("next_run_at") == Some(&Value::Null) {
            "completed"
        } else {
            "scheduled"
        };
        obj.insert("state".into(), Value::String(state.to_string()));
    }

    if !obj.contains_key("run_count") {
        let completed = obj
            .get("repeat")
            .and_then(|repeat| repeat.get("completed"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        obj.insert("run_count".into(), json!(completed));
    }

    if !obj.contains_key("updated_at") {
        if let Some(created_at) = obj.get("created_at").cloned() {
            obj.insert("updated_at".into(), created_at);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_interval_job() -> CronJob {
        CronJobBuilder::new("every 30m", "check status")
            .name("test-job")
            .build()
            .unwrap()
    }

    #[test]
    fn builder_creates_valid_job() {
        let job = make_interval_job();
        assert_eq!(job.name, "test-job");
        assert_eq!(job.prompt, "check status");
        assert!(job.enabled);
        assert_eq!(job.state, JobState::Scheduled);
        assert!(job.next_run_at.is_some());
    }

    #[test]
    fn builder_oneshot_sets_repeat_once() {
        let job = CronJobBuilder::new("30m", "one time task").build().unwrap();
        assert_eq!(job.repeat.times, Some(1));
    }

    #[test]
    fn builder_interval_sets_repeat_forever() {
        let job = CronJobBuilder::new("every 30m", "recurring task")
            .build()
            .unwrap();
        assert_eq!(job.repeat.times, None);
    }

    #[test]
    fn mark_job_run_increments_counters() {
        let mut job = make_interval_job();
        let keep = mark_job_run(&mut job, true, None);
        assert!(keep);
        assert_eq!(job.run_count, 1);
        assert_eq!(job.repeat.completed, 1);
        assert_eq!(job.last_status.as_deref(), Some("ok"));
    }

    #[test]
    fn mark_job_run_exhausts_repeat() {
        let mut job = CronJobBuilder::new("every 5m", "test")
            .repeat(2)
            .build()
            .unwrap();

        mark_job_run(&mut job, true, None);
        assert!(!job.repeat.exhausted());
        let keep = mark_job_run(&mut job, true, None);
        assert!(!keep);
        assert!(job.repeat.exhausted());
        assert_eq!(job.state, JobState::Completed);
    }

    #[test]
    fn is_due_future_not_due() {
        let mut job = make_interval_job();
        let future = (Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
        job.next_run_at = Some(future);
        assert!(!job.is_due(&Utc::now()));
    }

    #[test]
    fn is_due_past_is_due() {
        let mut job = make_interval_job();
        let past = (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        job.next_run_at = Some(past);
        assert!(job.is_due(&Utc::now()));
    }

    #[test]
    fn is_due_paused_not_due() {
        let mut job = make_interval_job();
        let past = (Utc::now() - chrono::Duration::seconds(10)).to_rfc3339();
        job.next_run_at = Some(past);
        job.state = JobState::Paused;
        assert!(!job.is_due(&Utc::now()));
    }

    #[test]
    fn id_short_is_8_chars() {
        let job = make_interval_job();
        assert_eq!(job.id_short().len(), 8);
    }

    #[test]
    fn repeat_display() {
        assert_eq!(RepeatConfig::forever().display(), "forever");
        assert_eq!(RepeatConfig::once().display(), "once");
        let mut r = RepeatConfig::n_times(5);
        r.completed = 2;
        assert_eq!(r.display(), "2/5");
    }

    #[test]
    fn deliver_roundtrip() {
        assert_eq!(
            "local".parse::<Deliver>().unwrap().to_string_repr(),
            "local"
        );
        assert_eq!(
            "origin".parse::<Deliver>().unwrap().to_string_repr(),
            "origin"
        );
        assert_eq!(
            "telegram".parse::<Deliver>().unwrap().to_string_repr(),
            "telegram"
        );
        assert_eq!(
            "telegram:123".parse::<Deliver>().unwrap().to_string_repr(),
            "telegram:123"
        );
    }

    #[test]
    fn builder_defaults_delivery_to_origin_when_origin_present() {
        let job = CronJobBuilder::new("every 30m", "check")
            .origin(Origin {
                platform: "telegram".into(),
                chat_id: "123".into(),
                chat_name: None,
                thread_id: None,
            })
            .build()
            .unwrap();
        assert_eq!(job.deliver, "origin");
    }

    #[test]
    fn normalize_store_recovers_legacy_single_skill_jobs() {
        let raw = json!({
            "jobs": [{
                "id": "job-1",
                "name": "legacy",
                "prompt": "check status",
                "skill": "blogwatcher",
                "schedule": { "kind": "interval", "minutes": 30 },
                "created_at": "2026-01-01T00:00:00Z",
                "next_run_at": "2026-01-01T00:30:00Z"
            }]
        });

        let normalized: CronStore = serde_json::from_value(normalize_store_value(raw)).unwrap();
        assert_eq!(normalized.jobs.len(), 1);
        assert_eq!(normalized.jobs[0].skills, vec!["blogwatcher"]);
        assert_eq!(normalized.jobs[0].deliver, "local");
        assert_eq!(normalized.jobs[0].state, JobState::Scheduled);
        assert_eq!(normalized.jobs[0].run_count, 0);
    }
}
