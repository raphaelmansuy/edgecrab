# edgecrab-cron

> **Why this crate?** EdgeCrab needs to run tasks while you sleep — daily digests, automated  
> checks, scheduled reminders delivered to any of its 15 messaging gateways. `edgecrab-cron`  
> provides the shared scheduling primitive: cron-expression parsing, job persistence, and  
> prompt-injection scanning for schedules sourced from user input.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Module | Purpose |
|--------|---------|
| `schedule` | Parse and evaluate cron expressions (5-field + optional seconds, TZ-aware) |
| `store` | Persist / load / delete cron jobs in `~/.edgecrab/cron/` |
| `scanner` | Scan user-supplied schedule strings for prompt-injection patterns |

## Add to your crate

```toml
[dependencies]
edgecrab-cron = { path = "../edgecrab-cron" }
```

## Usage

```rust
use edgecrab_cron::{CronJob, CronStore};

// Define a job
let job = CronJob {
    id: uuid::Uuid::new_v4(),
    name: "daily-digest".into(),
    schedule: "0 8 * * *".into(),      // 08:00 every day
    timezone: "America/New_York".into(),
    prompt: "Summarise my GitHub notifications".into(),
    platform: "telegram".into(),
    ..Default::default()
};

// Persist to disk
let store = CronStore::open()?;
store.save(&job)?;

// List next 5 fire times
let next = job.next_fires(5)?;
for t in next { println!("{t}"); }
```

## Cron expression format

```
┌──────── minute  (0–59)
│ ┌────── hour    (0–23)
│ │ ┌──── day-of-month (1–31)
│ │ │ ┌─── month (1–12 or JAN–DEC)
│ │ │ │ ┌── day-of-week (0–7, 0=Sun)
* * * * *
```

Standard shortcuts (`@daily`, `@hourly`, `@weekly`, `@monthly`) are supported.

## Job storage layout

```
~/.edgecrab/cron/
└── <uuid>.yaml    # one file per job
```

Override base path via the `EDGECRAB_HOME` environment variable.

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
