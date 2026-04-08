# edgecrab-migrate

> **Why this crate?** Switching agents shouldn't mean starting from zero. If you've used  
> hermes-agent (or its predecessor OpenClaw) you've already built up config, API keys,  
> conversation history, memories, and skills. `edgecrab-migrate` imports all of that into  
> EdgeCrab in a single non-destructive command — nothing is deleted from the source, and  
> a dry-run mode shows you exactly what will change before anything is written.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What gets migrated

| Asset | Source path | Destination |
|-------|-------------|-------------|
| Config | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| API keys | `~/.hermes/.env` | `~/.edgecrab/.env` |
| Memories | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| Skills | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| Sessions | `~/.hermes/sessions.db` | `~/.edgecrab/sessions.db` (converted) |

## Run the migration

```bash
# Preview what will happen — no files are written
edgecrab migrate --dry-run

# Live migration
edgecrab migrate

# Migrate into a specific EdgeCrab profile
edgecrab migrate --profile work
```

Example output:

```
[dry-run] config.yaml        → ~/.edgecrab/config.yaml        (new)
[dry-run] .env               → ~/.edgecrab/.env               (merge: 3 keys)
[dry-run] memories/MEMORY.md → ~/.edgecrab/memories/MEMORY.md (new)
[dry-run] skills/git.md      → ~/.edgecrab/skills/git.md      (new)
[dry-run] sessions.db        → ~/.edgecrab/sessions.db        (42 sessions, 1 287 messages)
Run without --dry-run to apply.
```

## Safety guarantees

- **Source is never modified or deleted.** hermes-agent continues to work after migration.
- **Destination files are never silently overwritten.** Conflicts are reported; existing values are kept unless `--overwrite` is passed.
- **Session import is idempotent** — re-running skips sessions already present by UUID.

## Embed in your binary

```toml
[dependencies]
edgecrab-migrate = { path = "../edgecrab-migrate" }
```

```rust
use edgecrab_migrate::{MigrationPlan, MigrateOptions};

let plan = MigrationPlan::from_hermes(MigrateOptions {
    dry_run: false,
    overwrite: false,
    target_profile: None,
})?;

plan.execute().await?;
println!("Migrated {} sessions, {} skills.", plan.sessions, plan.skills);
```

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
