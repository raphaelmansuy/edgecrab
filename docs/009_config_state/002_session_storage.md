# 009.002 — Session Storage

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 009.001 Config & State](./001_config_state.md) | [→ 003.001 Agent Struct](../003_agent_core/001_agent_struct.md) | [→ 008.001 Environments](../008_environments/001_environments.md)
> **Source**: `edgecrab-state/src/session_db.rs`, `edgecrab-state/src/schema.sql`
> **Parity**: mirrors hermes-agent session lifecycle — `session_lifecycle.py`, `session_search.py`

---

## 1. Why SQLite WAL + FTS5

Multiple EdgeCrab processes share one `~/.edgecrab/state.db`:

```
Gateway HTTP listener ─┐
                        ├──▶  state.db  (WAL mode)
CLI interactive loop  ─┤
                        │
Worktree sub-agents  ──┘
```

| Problem | Solution |
|---------|----------|
| Concurrent reads and writes without lock contention | WAL (Write-Ahead Log) mode — readers never block writers |
| Writer convoy (all retries fire at the same 20ms instant) | Jitter: random 20–150ms sleep before each retry (max 15 retries) |
| Full-text search across message history | FTS5 virtual table with BM25 ranking |
| Silent schema drift across releases | `schema_version` table; code constant `SCHEMA_VERSION = 6` |

WAL pragma configuration (applied on every open):

```sql
PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
```

`synchronous=NORMAL` tolerates a power-loss in the middle of a WAL frame (last transaction might be lost) while being dramatically faster than `FULL`.

---

## 2. Database Schema

### 2.1 `sessions` table

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id                   TEXT PRIMARY KEY,
    source               TEXT NOT NULL,          -- platform: "cli", "telegram", "discord", …
    user_id              TEXT,
    model                TEXT,
    system_prompt        TEXT,
    parent_session_id    TEXT,
    started_at           REAL NOT NULL,           -- Unix timestamp (f64 seconds)
    ended_at             REAL,
    end_reason           TEXT,                    -- "user_exit" | "context_limit" | "tool_error"
    message_count        INTEGER DEFAULT 0,
    tool_call_count      INTEGER DEFAULT 0,
    input_tokens         INTEGER DEFAULT 0,
    output_tokens        INTEGER DEFAULT 0,
    cache_read_tokens    INTEGER DEFAULT 0,
    cache_write_tokens   INTEGER DEFAULT 0,
    reasoning_tokens     INTEGER DEFAULT 0,       -- extended thinking
    estimated_cost_usd   REAL,
    title                TEXT UNIQUE              -- human-readable label; NULL = untitled
);
```

### 2.2 `messages` table

```sql
CREATE TABLE IF NOT EXISTS messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL,   -- "system" | "user" | "assistant" | "tool"
    content         TEXT,
    tool_call_id    TEXT,
    tool_calls      TEXT,            -- JSON-encoded Vec<ToolCall>
    tool_name       TEXT,
    timestamp       REAL NOT NULL,
    finish_reason   TEXT,
    reasoning       TEXT             -- extended thinking block
);
```

### 2.3 FTS5 virtual table

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    session_id UNINDEXED,
    role UNINDEXED,
    content='messages',
    content_rowid='id',
    tokenize='porter unicode61'
);
```

Sync triggers keep the FTS index consistent with `messages`:

```sql
-- INSERT trigger
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

-- DELETE trigger  
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;
```

---

## 3. `SessionDb` API Reference

### 3.1 Opening the database

```rust
use edgecrab_state::SessionDb;
use std::path::Path;

let db = SessionDb::open(Path::new("~/.edgecrab/state.db"))?;

// Testing only (in-memory, no file I/O)
let db = SessionDb::open_in_memory()?;
```

### 3.2 Session lifecycle

```rust
// Create session
db.save_session(&SessionRecord {
    id: uuid_v4(),
    source: "cli".to_string(),
    model: Some("claude-opus-4-5".to_string()),
    started_at: unix_now_f64(),
    ..Default::default()
})?;

// Append a message
db.save_message(&session_id, &message, unix_now_f64())?;

// Mark session ended
db.end_session(&session_id, "user_exit")?;

// Reopen for /resume
db.reopen_session(&session_id)?;
```

### 3.3 Retrieval

```rust
// By ID
let record: Option<SessionRecord> = db.get_session("abc-123")?;

// List most recent 20
let list: Vec<SessionSummary> = db.list_sessions(20)?;

// Rich list with message preview
let rich: Vec<SessionRichSummary> = db.list_sessions_rich(Some("telegram"), 50)?;

// All messages
let messages: Vec<Message> = db.get_messages("abc-123")?;

// By title
let rec = db.get_session_by_title("my project")?;
```

### 3.4 ID/title resolution

`resolve_session()` always drives `/resume` and `/export`:

```
resolve_session("abc")
  │
  ├─ 1. Exact ID match          → immediate return
  ├─ 2. Unique prefix match     → return if exactly 1 result
  └─ 3. Title lineage match     → "my project #3" beats "my project #2"
```

```rust
let full_id: Option<String> = db.resolve_session("abc-1")?;
```

### 3.5 Full-text search

```rust
let results: Vec<SearchResult> = db.search_sessions("context compression", Some(10))?;
// results[0].session_id, .snippet, .score (BM25, lower = more relevant)
```

FTS5 special characters (`"`, `*`, `-`, `+`, etc.) are sanitised before query construction to prevent injection.

### 3.6 Title hygiene

| Rule | Detail |
|------|--------|
| Max length | 100 chars (`MAX_TITLE_LENGTH`) |
| Strip | ASCII control characters, zero-width chars, directional overrides |
| Collapse | consecutive whitespace → single space |
| Uniqueness | enforced in database (UNIQUE constraint + check before write) |
| Lineage | duplicate titles become "title #2", "title #3", … |

```rust
let clean: Option<String> = SessionDb::sanitize_title("  My\x00Session  ")?;
// → Some("My Session")

let next = db.next_title_in_lineage("refactor-auth")?;
// → "refactor-auth #2"
```

### 3.7 Statistics & reporting

```rust
// Aggregate stats
let stats: SessionStats = db.session_statistics()?;
// stats.total_sessions, stats.total_messages, stats.by_source, stats.db_size_bytes

// Historical insights (30/60/90 day report)
let report: InsightsReport = db.historical_insights(30)?;
// report.overview.total_sessions, .estimated_total_cost_usd
// report.models, report.top_tools, report.daily_activity
```

### 3.8 Export & prune

```rust
// Export one session (session record + all messages)
let export: Option<SessionExport> = db.export_session_jsonl("abc-123")?;

// Export all sessions for backup
let all: Vec<SessionExport> = db.export_all_jsonl(None)?;

// Prune ended sessions older than 30 days
let deleted: usize = db.prune_sessions(30, None)?;
```

---

## 4. Write Contention Handling

Every write goes through `execute_write()`:

```rust
fn execute_write<F>(&self, f: F) -> Result<(), AgentError>
where F: Fn(&Connection) -> Result<(), rusqlite::Error>
{
    let mut rng = rand::thread_rng();
    for attempt in 0..WRITE_MAX_RETRIES {
        match self.conn.lock().and_then(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            f(&conn)?;
            conn.execute_batch("COMMIT")?;
            Ok(())
        }) {
            Ok(()) => { self.maybe_checkpoint(); return Ok(()); }
            Err(_) if attempt < WRITE_MAX_RETRIES - 1 => {
                let ms = rng.gen_range(WRITE_RETRY_MIN_MS..=WRITE_RETRY_MAX_MS);
                std::thread::sleep(Duration::from_millis(ms));
            }
            Err(e) => return Err(AgentError::Database(e.to_string())),
        }
    }
}
```

Key decisions:
- `BEGIN IMMEDIATE` acquires the WAL write lock at transaction start (vs. `BEGIN DEFERRED` which only acquires on first write — causes TOCTOU)
- Random 20–150 ms jitter eliminates deterministic thundering-herd retry storms
- Passive WAL checkpoint fires every 50 writes (`CHECKPOINT_EVERY_N_WRITES = 50`) to prevent WAL file growing unboundedly

---

## 5. Session Lifecycle in the Agent Loop

```text
Agent::run()
  │
  ├─▶ SessionDb::save_session()          ← session begins
  │
  │   per turn:
  ├─▶ SessionDb::save_message(user_msg)
  ├─▶ SessionDb::save_message(assistant_msg)
  ├─▶ SessionDb::save_message(tool_result)  ← 0..N tool calls
  ├─▶ SessionDb::update_token_counts()
  │
  └─▶ SessionDb::end_session("user_exit")  ← session ends
```

When context compression fires, `replace_messages()` atomically replaces all messages in one transaction rather than delete + re-insert N times:

```rust
db.replace_messages(&session_id, &compressed_messages, unix_now_f64())?;
```

---

## 6. Schema Migrations

`SCHEMA_VERSION = 6`. The version is stored in a `schema_version` table:

```sql
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER);
```

Current migration strategy: bump the version and re-run `schema.sql` (CREATE IF NOT EXISTS). Destructive migrations (column renames, drops) are deferred and handled by future migration scripts inside `init_schema()`.

---

## 7. Using `session_search` Tool

The `session_search` tool (in `edgecrab-tools`) exposes FTS5 search to the LLM:

```
Tool call: session_search { "query": "context compression bug", "limit": 5 }
→ Returns: JSON array of { session_id, snippet, score }
```

The tool escapes all FTS5 special syntax before passing the query to SQLite, so the LLM cannot accidentally corrupt the FTS index with malformed expressions.

---

## 8. Testing

```bash
# Unit tests (in-memory database, no file system)
cargo test -p edgecrab-state

# Specific test modules
cargo test -p edgecrab-state session_db::tests
```

Key test patterns:

```rust
#[tokio::test]
async fn test_session_roundtrip() {
    let db = SessionDb::open_in_memory().unwrap();
    let id = "test-123";
    db.save_session(&SessionRecord { id: id.to_string(), source: "test".to_string(), started_at: 0.0, ..Default::default() }).unwrap();
    let found = db.get_session(id).unwrap();
    assert!(found.is_some());
    db.end_session(id, "done").unwrap();
    let after = db.get_session(id).unwrap().unwrap();
    assert_eq!(after.end_reason.as_deref(), Some("done"));
}

#[test]
fn test_title_lineage() {
    let db = SessionDb::open_in_memory().unwrap();
    // save "my project" session
    let next = db.next_title_in_lineage("my project").unwrap();
    assert_eq!(next, "my project #2");
}
```
