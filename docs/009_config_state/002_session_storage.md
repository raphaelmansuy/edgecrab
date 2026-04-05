# 🦀 Session Storage

> **WHY**: Conversation history, cost tracking, and search across past sessions all require a reliable, queryable store. EdgeCrab uses embedded SQLite — no external database process, no network dependency, no configuration beyond a file path.

**Source**: `crates/edgecrab-state/src/session_db.rs`, `crates/edgecrab-state/src/schema.sql`

---

## Architecture Overview

```
┌──────────────────────────────────────────────────────────────┐
│                      Agent / Gateway                          │
│   save_session()   get_messages()   list_sessions_rich()      │
└──────────────────────────┬───────────────────────────────────┘
                           │  SessionDb API
                           ▼
┌──────────────────────────────────────────────────────────────┐
│                      SessionDb                                │
│                                                               │
│  ┌─────────────┐   ┌──────────────┐   ┌───────────────────┐  │
│  │  sessions   │   │   messages   │   │  messages_fts     │  │
│  │  table      │◄──│   table      │──►│  (FTS5 virtual)   │  │
│  │             │   │              │   │  auto-synced       │  │
│  └─────────────┘   └──────────────┘   │  via triggers      │  │
│                                        └───────────────────┘  │
│                                                               │
│  WAL mode  •  FK enforcement  •  jitter-retry writes         │
└──────────────────────────────────────────────────────────────┘
                           │
                     state.db  (SQLite file)
```

---

## SQLite Tuning

EdgeCrab opens SQLite with three settings that matter at scale:

| Setting | Value | Why |
|---|---|---|
| Journal mode | `WAL` | Non-blocking reads during writes; essential for concurrent CLI + gateway usage on the same file |
| Foreign keys | `ON` | Prevents orphaned message rows if sessions are pruned |
| Schema version | `6` | Verified at open; migrations run automatically |

**Write-convoy prevention**: every write path uses exponential backoff with jitter in `[20, 150)` ms so concurrent writers (CLI, gateway, cron) don't pile up into lock convoys. `WRITE_MAX_RETRIES = 15`.

```
Writer 1 ──► locked ──► wait 73ms ──► retry ──► success
Writer 2 ──► locked ──► wait 41ms ──► retry ──► success (different jitter)
```

---

## Schema: Sessions Table

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT PK | UUID |
| `source` | TEXT | Platform: `cli`, `telegram`, `discord`… |
| `user_id` | TEXT | Platform-specific user identifier |
| `model` | TEXT | Model used for the session |
| `system_prompt` | TEXT | Full system prompt at session start |
| `parent_id` | TEXT | Lineage — parent session id if branched |
| `root_id` | TEXT | Lineage — root of the session tree |
| `prompt_tokens` | INTEGER | Accumulated input tokens |
| `completion_tokens` | INTEGER | Accumulated output tokens |
| `estimated_cost_usd` | REAL | Running cost estimate |
| `title` | TEXT | Auto-generated or user-set title |
| `created_at` | TEXT | ISO8601 UTC |
| `updated_at` | TEXT | ISO8601 UTC |

---

## Schema: Messages Table

Message rows store the conversation transcript in the canonical `Message` format from `edgecrab-types`. Each row is a complete serialised `Message` — role, content, optional tool calls, optional reasoning — so replaying a session is a simple ordered scan.

FTS5 keeps `messages_fts` in sync via database triggers. The trigger fires on `INSERT` and `UPDATE`, so search is always current with zero application-level bookkeeping.

---

## Public API

```rust
// Persist a completed session with its messages
SessionDb::save_session(&self, session: &Session) -> Result<()>

// List all sessions, newest first
SessionDb::list_sessions(&self) -> Result<Vec<SessionSummary>>

// List sessions by the source platform
SessionDb::list_sessions_by_source(&self, source: &str) -> Result<Vec<SessionSummary>>

// List sessions with token and cost metadata
SessionDb::list_sessions_rich(&self) -> Result<Vec<RichSessionSummary>>

// Delete sessions older than the given age
SessionDb::prune_sessions(&self, older_than: Duration) -> Result<usize>

// Export a single session as newline-delimited JSON
SessionDb::export_session_jsonl(&self, id: &str) -> Result<String>

// Export all sessions as newline-delimited JSON
SessionDb::export_all_jsonl(&self) -> Result<String>

// Retrieve full message history for a session
SessionDb::get_messages(&self, session_id: &str) -> Result<Vec<Message>>
```

---

## Write Flow

```
Agent loop completes turn
        │
        ▼
serialize Message to JSON
        │
        ▼
INSERT INTO messages  ──► trigger fires ──► FTS5 index updated
        │
        ▼
UPDATE sessions (tokens, cost, updated_at)
        │
        ▼
COMMIT (WAL — readers unblocked immediately)
```

---

## Storage File Location

| Context | Path |
|---|---|
| Default | `~/.edgecrab/state.db` |
| Custom home | `$EDGECRAB_HOME/state.db` |
| Profile | `~/.edgecrab/profiles/<name>/state.db` |

Each profile has its own isolated `state.db` — sessions from the `work` profile never appear in the `personal` profile's history.

---

## Slash Commands Backed by SessionDb

| Command | Operation |
|---|---|
| `/history` | `list_sessions_rich` |
| `/search <query>` | FTS5 full-text search on `messages_fts` |
| `/export` | `export_session_jsonl` |
| `/prune` | `prune_sessions` |
| `/cost` | Reads `estimated_cost_usd` from `sessions` |

---

## Tips

- **FTS5 search is fast on large histories** — SQLite FTS5 uses a B-tree index, not a full scan. `/search` across 10 000 messages is sub-millisecond.
- **WAL mode means you can query `state.db` externally** (e.g., with `sqlite3` CLI or DBeaver) while EdgeCrab is running — reads will never block.
- **Export before pruning** — `export_all_jsonl` writes every session to stdout; pipe it to a file as a backup before running `/prune`.

---

## FAQ

**Q: Can I share one `state.db` across multiple users on the same machine?**
A: Technically yes (WAL handles concurrent access), but sessions are not access-controlled at the SQLite level. Use separate profiles instead.

**Q: Does EdgeCrab run schema migrations automatically?**
A: Yes. The schema version is checked at open; if it's below `6`, migration SQL runs automatically before any other queries.

**Q: How do I search old conversations from the command line?**
A: `edgecrab /search "query"` — backed by FTS5, supports phrase search and prefix wildcards.

---

## Cross-References

- Config file paths → [`009_config_state/001_config_state.md`](001_config_state.md)
- Message data model → [`010_data_models/001_data_models.md`](../010_data_models/001_data_models.md)
- Concurrency model (WAL + jitter) → [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- Platform source values → [`006_gateway/001_gateway_architecture.md`](../006_gateway/001_gateway_architecture.md)
