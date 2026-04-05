---
title: SQLite State & Search
description: How EdgeCrab stores session history in WAL-mode SQLite with FTS5 full-text search. Database schema, search commands, backup, and performance characteristics.
sidebar:
  order: 5
---

EdgeCrab persists all conversation history in a WAL-mode SQLite database with FTS5 full-text search. Every message, tool call, and tool result is indexed and searchable — no conversation is ever lost.

---

## Why SQLite?

- **Zero infrastructure**: no Postgres, Redis, or external services required
- **WAL mode**: concurrent reads never block writes; crash-safe
- **FTS5**: built-in full-text search with ranking — no Elasticsearch needed
- **Portable**: a single `.db` file you can copy, backup, or inspect with any SQLite viewer

---

## Database Location

```
~/.edgecrab/state.db         # Main session + message store
~/.edgecrab/state.db-wal     # WAL write-ahead log (auto-managed)
~/.edgecrab/state.db-shm     # Shared memory header (auto-managed)
```

---

## Schema Overview

```sql
-- Sessions: one row per conversation
CREATE TABLE sessions (
    id         INTEGER PRIMARY KEY,
    name       TEXT,
    created_at INTEGER NOT NULL,   -- Unix timestamp
    updated_at INTEGER NOT NULL,
    metadata   TEXT                -- JSON
);

-- Messages: one row per turn
CREATE TABLE messages (
    id         INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id),
    role       TEXT NOT NULL,      -- 'user' | 'assistant' | 'tool'
    content    TEXT NOT NULL,
    tool_name  TEXT,               -- set for role='tool'
    tool_args  TEXT,               -- JSON, set for role='assistant' tool calls
    created_at INTEGER NOT NULL
);

-- FTS5 virtual table: indexes all message content
CREATE VIRTUAL TABLE messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);
```

---

## Full-Text Search

Search across all conversations from the command line:

```bash
# Search all sessions
edgecrab search "SQLite WAL"

# Search within a specific session
edgecrab search "SQLite WAL" --session my-project

# Limit results
edgecrab search "tokio async" --limit 20

# Show surrounding context (default: 0)
edgecrab search "SSRF" --context 2
```

Output:

```
  3 results for "SQLite WAL"
  ──────────────────────────────────────────────────────────────
  [1] session: my-project  ·  2026-04-02 14:31
      ...EdgeCrab uses WAL-mode SQLite with FTS5 full-text search.
      Every session is stored in ~/.edgecrab/state.db...

  [2] session: refactor-auth  ·  2026-04-01 09:12
      ...The state.db file uses SQLite WAL mode so concurrent
      reads never block writes...
```

---

## Session Management Commands

```bash
edgecrab sessions list                    # List all sessions
edgecrab sessions list --limit 20         # Show most recent 20
edgecrab sessions show my-project         # Show messages in a session
edgecrab sessions delete my-project       # Delete a session (irreversible)
edgecrab sessions prune --older-than 90d  # Delete sessions older than 90 days
edgecrab sessions prune --keep-last 50    # Keep only the 50 most recent
edgecrab sessions export my-project > session.json   # Export to JSON
```

---

## Integrity and Crash Safety

On startup, EdgeCrab runs:

```sql
PRAGMA integrity_check;
PRAGMA wal_checkpoint(PASSIVE);
```

If the integrity check fails (extremely rare, usually indicates hardware issues), EdgeCrab reports the error and exits rather than silently operating on corrupt data.

---

## Performance

FTS5 makes search fast even at scale:

| Sessions | Messages | Search time |
|----------|----------|-------------|
| 100 | 10,000 | < 5 ms |
| 1,000 | 100,000 | < 20 ms |
| 10,000 | 1,000,000 | < 80 ms |

Write performance:
- Each message is written in a single `INSERT` (~0.1 ms)
- FTS5 index is updated synchronously within the same transaction
- WAL mode means writes never block the TUI

---

## Viewing the Database

You can inspect the database directly with any SQLite client:

```bash
# CLI
sqlite3 ~/.edgecrab/state.db
sqlite> SELECT name, created_at FROM sessions ORDER BY updated_at DESC LIMIT 10;
sqlite> SELECT role, content FROM messages WHERE session_id = 1;

# GUI: DB Browser for SQLite, TablePlus, DBeaver, etc.
open ~/.edgecrab/state.db   # macOS — opens with default SQLite viewer
```

---

## Backup

```bash
# Snapshot backup (safe while EdgeCrab is running thanks to WAL)
sqlite3 ~/.edgecrab/state.db ".backup /path/to/backup.db"

# Or simply:
cp -r ~/.edgecrab ~/.edgecrab.bak-$(date +%Y%m%d)
```

---

## Disabling Persistence (ephemeral mode)

For sessions where you don't want anything stored:

```bash
edgecrab --no-persist
```

All messages are kept in memory for the duration of the session and discarded on exit.

---

## Pro Tips

**Search before asking.** If you've worked on a topic before, `edgecrab sessions search "authentication jwt"` returns the exact conversation where you solved it. Faster than re-asking the agent.

**Export important sessions.** Before pruning old sessions, export the ones worth keeping:
```bash
edgecrab sessions export architecture-decision > docs/decisions/2026-04-01.json
edgecrab sessions export architecture-decision --format markdown > docs/decisions/2026-04-01.md
```

**Run integrity check after hardware events.** After a crash or power loss, run `edgecrab doctor` — it includes a SQLite integrity check.

**The WAL files are normal.** `state.db-wal` and `state.db-shm` appearing alongside `state.db` is normal SQLite WAL operation. Don't delete them while EdgeCrab is running.

---

## Frequently Asked Questions

**Q: My state.db is getting large. How do I reduce it?**

```bash
edgecrab sessions prune --older-than 90d   # delete old sessions
sqlite3 ~/.edgecrab/state.db "VACUUM;"     # reclaim disk space
```
`VACUUM` rewrites the database file and reclaims space from deleted rows. It's safe but may take a few seconds on large databases.

**Q: Can I move the database to a different location?**

Not via config currently, but you can symlink it:
```bash
mv ~/.edgecrab/state.db /Volumes/fast-ssd/edgecrab.db
ln -s /Volumes/fast-ssd/edgecrab.db ~/.edgecrab/state.db
```

**Q: I want to search messages by date range. Is that possible?**

Not yet via the CLI, but directly via SQLite:
```bash
sqlite3 ~/.edgecrab/state.db \
  "SELECT role, content FROM messages WHERE created_at > strftime('%s','2026-01-01');"
```

**Q: Can multiple EdgeCrab instances share the same database?**

Yes, via WAL mode — readers never block writers. However, two instances writing concurrently may contend for locks. Use profiles to give each instance its own database for clean isolation.

---

## See Also

- [Sessions](/user-guide/sessions/) — Managing sessions from the user-guide perspective
- [CLI Commands](/reference/cli-commands/) — Full `edgecrab sessions` and `edgecrab search` reference
- [Configuration](/user-guide/configuration/) — Session and compression config keys
