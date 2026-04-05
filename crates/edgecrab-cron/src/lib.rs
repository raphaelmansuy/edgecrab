//! # edgecrab-cron
//!
//! Shared cron scheduling library for EdgeCrab.
//!
//! Provides the single source of truth for:
//!   - Schedule parsing (one-shot delays, intervals, cron expressions, ISO timestamps)
//!   - Job storage (atomic writes, file-based tick lock, output persistence)
//!   - Prompt security scanning (injection/exfiltration/invisible-unicode detection)
//!
//! Both `edgecrab-cli` (CLI cron commands) and `edgecrab-tools` (LLM-callable
//! `manage_cron_jobs` tool) consume this crate, ensuring DRY store semantics.
//!
//! ## Feature parity with hermes-agent
//!
//! | Feature                     | hermes-agent      | edgecrab (this crate) |
//! |-----------------------------|-------------------|-----------------------|
//! | One-shot delays             | ✓ 30m/2h          | ✓                     |
//! | Intervals                   | ✓ every 30m       | ✓                     |
//! | Cron expressions            | ✓ 0 9 * * *       | ✓                     |
//! | ISO timestamps              | ✓                 | ✓                     |
//! | Skills per job              | ✓ multi-skill     | ✓                     |
//! | Repeat counters             | ✓ repeat=5        | ✓                     |
//! | Delivery targets            | ✓ local/origin/…  | ✓ Deliver enum + GatewaySender|
//! | Recursion guard             | ✓ disable cronjob | ✓ Platform::Cron gate  |
//! | File lock (tick)            | ✓ .tick.lock      | ✓                     |
//! | Atomic writes               | ✓ tmp+rename      | ✓                     |
//! | Output persistence          | ✓ output/{id}/    | ✓                     |
//! | Silent marker               | ✓ [SILENT]        | ✓ (injected in prompt) |
//! | Grace period (missed jobs)  | ✓                 | ✓                     |
//! | Job states (sched/paused/…) | ✓                 | ✓                     |
//! | Per-job model override      | ✓                 | ✓                     |
//! | Prompt injection scanning   | ✓                 | ✓                     |

pub mod scan;
pub mod schedule;
pub mod store;
mod time;

pub use scan::scan_cron_prompt;
pub use schedule::{
    Schedule, advance_next_run_past_now, compute_next_run, parse_duration_minutes, parse_schedule,
    schedule_display,
};
pub use store::{
    CronJob, CronJobBuilder, CronStore, Deliver, JobState, Origin, RepeatConfig, TickLock,
    advance_pre_exec, create_job, cron_dir, format_ts, jobs_file, load_store, mark_job_run,
    output_dir, resolve_job_index, save_output, save_store,
};

/// Sentinel marker — when injected at the start of a cron agent's response,
/// output is saved locally but not delivered to the user (nothing to report).
pub const SILENT_MARKER: &str = "[SILENT]";

/// System hint prepended to every cron job prompt so that cron agents can
/// suppress delivery when they have nothing to report.
pub const SILENT_HINT: &str = "\
[SYSTEM: If you have nothing new or noteworthy to report, respond with exactly \
\"[SILENT]\" (optionally followed by a brief internal note). This suppresses \
delivery to the user while still saving output locally. Only use [SILENT] when \
there are genuinely no changes worth reporting.]\n\n";
