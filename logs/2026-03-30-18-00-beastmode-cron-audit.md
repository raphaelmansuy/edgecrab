# Task log — 2026-03-30 cron audit

## Actions
- Audited edgecrab cron against hermes-agent cron; identified 2 critical missing features
- Added `advance_pre_exec()` to `edgecrab-cron/src/store.rs` (at-most-once semantics for recurring jobs on crash)
- Added Phase 1 stale-grace detection to `tick_due_jobs` (prevents burst-fire after downtime)
- Added Phase 2 pre-execution advance save to `tick_due_jobs` (crash-safety save before agent call)
- Replaced inherent `Deliver::from_str` with `impl std::str::FromStr for Deliver` (clippy compliance)
- Fixed 5 pre-existing clippy warnings in cron.rs and app.rs

## Decisions
- Both missing features match hermes-agent exactly: `advance_next_run` (pre-exec) and `get_due_jobs` grace window check
- `advance_pre_exec` is not called for `Schedule::Once` jobs (they should retry on restart until success)
- Grace window scales with schedule period (half interval, clamped 120 s – 7200 s), matching hermes formula

## Next steps
- Optional: add per-job `provider` + `base_url` override fields (hermes has them; low priority)
- Optional: extend `Deliver::Explicit` to carry an optional thread_id for Telegram topic threads

## Lessons
- Pre-advancing `next_run_at` before execution (at-most-once) is a non-obvious but critical correctness property that prevents crash loops
- Stale grace detection prevents "thundering herd" after gateway restarts
