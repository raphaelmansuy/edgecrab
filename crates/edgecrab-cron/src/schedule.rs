//! Schedule parsing and next-run computation.
//!
//! Supports four formats (matching hermes-agent parity):
//!   - One-shot duration: `30m`, `2h`, `1d`
//!   - Recurring interval: `every 30m`, `every 2h`
//!   - Standard cron expression: `0 9 * * *`
//!   - ISO 8601 timestamp: `2026-03-15T09:00:00`
//!
//! ## Grace period for missed jobs
//!
//! Recurring jobs that were missed (e.g. machine was offline) catch up via
//! a grace window: half the period, clamped between 120 s and 2 hours.
//! If still within the grace window, the job fires immediately; if outside,
//! `advance_next_run` fast-forwards to the *next* future slot.

use std::str::FromStr;

use anyhow::{Context, bail};
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::time::{naive_local_to_utc, now_in_user_timezone, to_user_timezone};

// ─── Types ────────────────────────────────────────────────────────────

/// Parsed schedule — the canonical form stored per-job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    /// Run exactly once at a fixed timestamp.
    Once {
        /// ISO 8601 string (timezone-aware) of the planned run.
        run_at: String,
    },
    /// Repeat at a fixed interval.
    Interval {
        /// Period in whole minutes (>0).
        minutes: u64,
    },
    /// Standard cron expression (5-field).
    Cron {
        /// The raw 5-field cron expression, e.g. `"0 9 * * *"`.
        expr: String,
    },
}

/// One-shot grace window: a one-shot job whose `run_at` is no older than this
/// still fires on the next tick (handles clock drift / slow startup).
const ONESHOT_GRACE_SECS: i64 = 120;

/// Lower bound on the catch-up grace window for recurring jobs.
const MIN_GRACE_SECS: i64 = 120;
/// Upper bound on the catch-up grace window.
const MAX_GRACE_SECS: i64 = 7_200;

fn normalize_cron_expr(expr: &str) -> anyhow::Result<String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {expr} *")),
        6 => Ok(format!("0 {expr}")),
        _ => bail!("invalid cron expression '{expr}': expected 5 fields or 6 fields with year"),
    }
}

// ─── Duration parsing ─────────────────────────────────────────────────

/// Parse a human duration string (`"30m"`, `"2h"`, `"1d"`) into minutes.
pub fn parse_duration_minutes(s: &str) -> anyhow::Result<u64> {
    let s = s.trim();
    let re =
        Regex::new(r"(?i)^(\d+)\s*(m|min|mins|minute|minutes|h|hr|hrs|hour|hours|d|day|days)$")
            .unwrap();
    let caps = re
        .captures(s)
        .with_context(|| format!("invalid duration '{s}': use 30m / 2h / 1d"))?;
    let value: u64 = caps[1].parse().unwrap();
    let unit = caps[2].to_ascii_lowercase();
    let multiplier: u64 = match unit.chars().next().unwrap() {
        'm' => 1,
        'h' => 60,
        'd' => 1_440,
        _ => unreachable!(),
    };
    Ok(value * multiplier)
}

// ─── Schedule parsing ─────────────────────────────────────────────────

/// Parse a schedule string into a `Schedule`.
///
/// Accepted formats:
/// ```text
/// "30m"                  → Once in 30 minutes
/// "2h"                   → Once in 2 hours
/// "1d"                   → Once in 1 day
/// "every 30m"            → Interval of 30min
/// "every 2h"             → Interval of 2h
/// "0 9 * * *"            → Cron expression (daily at 09:00)
/// "2026-03-15T09:00:00"  → ISO 8601 one-shot
/// ```
pub fn parse_schedule(input: &str) -> anyhow::Result<Schedule> {
    let s = input.trim();
    let lower = s.to_ascii_lowercase();

    // --- "every X" → Interval ---
    if let Some(rest) = lower.strip_prefix("every ") {
        let minutes = parse_duration_minutes(rest)
            .with_context(|| format!("invalid interval schedule '{s}'"))?;
        return Ok(Schedule::Interval { minutes });
    }

    // --- 5-field cron expression (or 6 fields with year) ---
    let fields: Vec<&str> = s.split_whitespace().collect();
    if matches!(fields.len(), 5 | 6) {
        let normalized = normalize_cron_expr(s)?;
        cron::Schedule::from_str(&normalized)
            .with_context(|| format!("invalid cron expression '{s}'"))?;
        return Ok(Schedule::Cron {
            expr: s.to_string(),
        });
    }

    // --- ISO 8601 timestamp ---
    if s.contains('T') || s.len() >= 10 && s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        // Try fully qualified ISO with timezone first
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Ok(Schedule::Once {
                run_at: dt.with_timezone(&Utc).to_rfc3339(),
            });
        }
        // Naive ISO (no timezone) — interpret as configured timezone or local time
        let formats = [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
            "%Y-%m-%d",
        ];
        for fmt in formats {
            if let Ok(ndt) = NaiveDateTime::parse_from_str(s, fmt) {
                return Ok(Schedule::Once {
                    run_at: naive_local_to_utc(ndt)
                        .with_context(|| format!("invalid local timestamp '{s}'"))?
                        .to_rfc3339(),
                });
            }
        }
        // Bare date
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            let ndt = d.and_hms_opt(0, 0, 0).unwrap();
            return Ok(Schedule::Once {
                run_at: naive_local_to_utc(ndt)
                    .with_context(|| format!("invalid local date '{s}'"))?
                    .to_rfc3339(),
            });
        }
        bail!("invalid timestamp '{s}': use ISO 8601, e.g. 2026-03-15T09:00:00");
    }

    // --- Bare duration → one-shot in X minutes ---
    if let Ok(minutes) = parse_duration_minutes(s) {
        let run_at = Utc::now() + Duration::minutes(minutes as i64);
        return Ok(Schedule::Once {
            run_at: run_at.to_rfc3339(),
        });
    }

    bail!(
        "unrecognized schedule '{s}'.\n\
         Use:\n  \
           30m / 2h / 1d        (one-shot delay)\n  \
           every 30m / every 2h (recurring interval)\n  \
           0 9 * * *            (cron expression)\n  \
           2026-03-15T09:00:00  (ISO 8601 timestamp)"
    )
}

/// Human-readable display string for a schedule.
pub fn schedule_display(s: &Schedule) -> String {
    match s {
        Schedule::Once { run_at } => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(run_at) {
                format!(
                    "once at {}",
                    to_user_timezone(dt.with_timezone(&Utc)).format("%Y-%m-%d %H:%M")
                )
            } else {
                format!("once at {run_at}")
            }
        }
        Schedule::Interval { minutes } => {
            if *minutes % 1_440 == 0 {
                format!("every {}d", minutes / 1_440)
            } else if *minutes % 60 == 0 {
                format!("every {}h", minutes / 60)
            } else {
                format!("every {minutes}m")
            }
        }
        Schedule::Cron { expr } => expr.clone(),
    }
}

// ─── Next-run computation ─────────────────────────────────────────────

/// Compute the next run time for a job.
///
/// - `last_run_at`: ISO 8601 string of the last run (if any).
/// - Returns `None` when the job should not run again (one-shot after first run).
pub fn compute_next_run(schedule: &Schedule, last_run_at: Option<&str>) -> Option<String> {
    let now = Utc::now();

    match schedule {
        Schedule::Once { run_at } => {
            // Already ran → never again.
            if last_run_at.is_some() {
                return None;
            }
            // Within grace window → still fire.
            let run_at_dt = parse_rfc3339(run_at)?;
            let cutoff = now - Duration::seconds(ONESHOT_GRACE_SECS);
            if run_at_dt >= cutoff {
                Some(run_at.clone())
            } else {
                None
            }
        }

        Schedule::Interval { minutes } => {
            let base = if let Some(last) = last_run_at.and_then(parse_rfc3339) {
                last
            } else {
                now
            };
            let next = base + Duration::minutes(*minutes as i64);
            Some(next.to_rfc3339())
        }

        Schedule::Cron { expr } => {
            let normalized = normalize_cron_expr(expr).ok()?;
            let sched = cron::Schedule::from_str(&normalized).ok()?;
            let now_local = now_in_user_timezone();
            sched
                .after(&now_local)
                .next()
                .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
        }
    }
}

/// For recurring jobs, advance `next_run_at` past `now` using grace semantics.
///
/// If the job's next run is already in the future, it is left unchanged.
/// If it is within the grace window (half the period), it fires immediately.
/// Otherwise it fast-forwards to the next future slot.
pub fn advance_next_run_past_now(schedule: &Schedule, next_run_at: &str) -> Option<String> {
    let now = Utc::now();
    let next = parse_rfc3339(next_run_at)?;

    if next > now {
        return Some(next_run_at.to_string());
    }

    let grace = compute_grace_seconds(schedule);
    let cutoff = now - Duration::seconds(grace);
    if next >= cutoff {
        // Still within grace — fire this tick.
        return Some(next_run_at.to_string());
    }

    // Outside grace → advance to next future slot.
    compute_next_run(schedule, Some(&now.to_rfc3339()))
}

/// Grace window in seconds for a recurring schedule.
fn compute_grace_seconds(schedule: &Schedule) -> i64 {
    let period_secs: i64 = match schedule {
        Schedule::Interval { minutes } => (*minutes as i64) * 60,
        Schedule::Cron { expr } => normalize_cron_expr(expr)
            .ok()
            .and_then(|normalized| cron::Schedule::from_str(&normalized).ok())
            .and_then(|s| {
                let now_local = now_in_user_timezone();
                let first = s.after(&now_local).next()?;
                let second = s.after(&first).next()?;
                Some((second - first).num_seconds())
            })
            .unwrap_or(MIN_GRACE_SECS),
        Schedule::Once { .. } => return MIN_GRACE_SECS,
    };
    (period_secs / 2).clamp(MIN_GRACE_SECS, MAX_GRACE_SECS)
}

// ─── Helpers ──────────────────────────────────────────────────────────

fn parse_rfc3339(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_minutes_valid() {
        assert_eq!(parse_duration_minutes("30m").unwrap(), 30);
        assert_eq!(parse_duration_minutes("2h").unwrap(), 120);
        assert_eq!(parse_duration_minutes("1d").unwrap(), 1440);
        assert_eq!(parse_duration_minutes("90 minutes").unwrap(), 90);
    }

    #[test]
    fn parse_duration_minutes_invalid() {
        assert!(parse_duration_minutes("30x").is_err());
        assert!(parse_duration_minutes("abc").is_err());
    }

    #[test]
    fn parse_schedule_interval() {
        assert_eq!(
            parse_schedule("every 30m").unwrap(),
            Schedule::Interval { minutes: 30 }
        );
        assert_eq!(
            parse_schedule("every 2h").unwrap(),
            Schedule::Interval { minutes: 120 }
        );
        assert_eq!(
            parse_schedule("every 1d").unwrap(),
            Schedule::Interval { minutes: 1440 }
        );
    }

    #[test]
    fn parse_schedule_cron() {
        let s = parse_schedule("0 9 * * *").unwrap();
        assert!(matches!(s, Schedule::Cron { .. }));
        if let Schedule::Cron { expr } = s {
            assert_eq!(expr, "0 9 * * *");
        }
    }

    #[test]
    fn parse_schedule_cron_with_year() {
        let s = parse_schedule("0 9 * * * 2028").unwrap();
        assert!(matches!(s, Schedule::Cron { .. }));
        if let Schedule::Cron { expr } = s {
            assert_eq!(expr, "0 9 * * * 2028");
        }
    }

    #[test]
    fn parse_schedule_one_shot_duration() {
        // Should produce a Once variant
        let s = parse_schedule("30m").unwrap();
        assert!(matches!(s, Schedule::Once { .. }));
    }

    #[test]
    fn parse_schedule_iso_timestamp() {
        let s = parse_schedule("2028-12-25T09:00:00").unwrap();
        assert!(matches!(s, Schedule::Once { .. }));
    }

    #[test]
    fn parse_schedule_invalid() {
        assert!(parse_schedule("foobar").is_err());
        assert!(parse_schedule("5 fields but bad * * * * $").is_err());
    }

    #[test]
    fn compute_next_run_interval_no_last() {
        let s = Schedule::Interval { minutes: 60 };
        let next = compute_next_run(&s, None).unwrap();
        let next_dt = DateTime::parse_from_rfc3339(&next).unwrap();
        let now = Utc::now();
        let diff = (next_dt.with_timezone(&Utc) - now).num_minutes();
        assert!((59..=61).contains(&diff), "expected ~60 min, got {diff}");
    }

    #[test]
    fn compute_next_run_once_no_last() {
        let run_at = (Utc::now() + Duration::minutes(5)).to_rfc3339();
        let s = Schedule::Once {
            run_at: run_at.clone(),
        };
        // Should return the run_at unchanged when not yet run
        assert_eq!(compute_next_run(&s, None).unwrap(), run_at);
    }

    #[test]
    fn compute_next_run_once_after_last() {
        // Once a one-shot has run, it must return None
        let run_at = (Utc::now() + Duration::minutes(5)).to_rfc3339();
        let s = Schedule::Once { run_at };
        assert!(compute_next_run(&s, Some("2026-01-01T00:00:00Z")).is_none());
    }

    #[test]
    fn schedule_display_interval() {
        assert_eq!(
            schedule_display(&Schedule::Interval { minutes: 30 }),
            "every 30m"
        );
        assert_eq!(
            schedule_display(&Schedule::Interval { minutes: 120 }),
            "every 2h"
        );
        assert_eq!(
            schedule_display(&Schedule::Interval { minutes: 1440 }),
            "every 1d"
        );
    }
}
