---
title: Sessions & Memory
description: How EdgeCrab sessions, conversation history, persistent memory, and full-text search work. Plus compression, naming, and session management commands.
sidebar:
  order: 3
---

EdgeCrab stores every conversation in a WAL-mode SQLite database with FTS5 full-text search. Alongside sessions, a memory system persists facts across all future conversations.

---

## Sessions

### What is a Session?

A session is a complete conversation: the sequence of user messages, agent replies, and tool calls. Everything is stored in `~/.edgecrab/state.db` automatically.

### Starting a New Session

```bash
edgecrab                        # Unnamed session — auto ID
edgecrab --session my-project   # Named session
```

Inside the TUI:

```
/new                            # Fresh unnamed session
/session new my-project         # Fresh named session
```

### Resuming a Session

```bash
edgecrab --session my-project   # Resume by name
```

Inside the TUI:

```
/session list                   # Show all sessions
/session load my-project        # Load a session by name
```

### Session List

```bash
/session list
```

```
  ID   Name             Messages   Last Active
  ───  ───────────────  ────────   ──────────────────
  1    my-project       47         2026-04-02 14:32
  2    refactor-auth    12         2026-04-01 09:14
  3    (unnamed)         8         2026-03-31 22:05
```

---

## Context Window Management

EdgeCrab automatically manages context length. When a conversation approaches the `max_context_tokens` limit (default: 100,000), it:

1. **Summarizes** older messages with the LLM into a compact summary block
2. **Keeps** the summary + all recent messages within the window
3. **Stores** the full uncompressed history in SQLite — nothing is ever deleted

You can tune the threshold in `~/.edgecrab/config.yaml`:

```yaml
session:
  max_context_tokens: 100000
  compression_threshold: 80000  # Start compressing when this is reached
```

---

## Memory System

Memory is separate from session history. It is a set of plain-text Markdown files in `~/.edgecrab/memories/` that are injected at the start of **every** session.

### What Gets Remembered?

EdgeCrab identifies facts worth persisting during sessions:
- Project preferences and conventions
- Recurring patterns the user cares about
- Explicit notes you ask it to remember

### Writing a Memory Manually

```bash
echo "# Project: edgecrab\n\nAlways use Result<T, Box<dyn Error>> for error handling.\nTarget: Rust 1.86, edition 2024." > ~/.edgecrab/memories/project-edgecrab.md
```

Or ask EdgeCrab directly:

```
Please remember: this project uses 4-space indentation and snake_case for all identifiers.
```

### Listing Memory Files

```
/memory
```

```
  Files in ~/.edgecrab/memories/
  ─────────────────────────────────────────────
  project-edgecrab.md     (2.1 KB)
  coding-preferences.md   (0.8 KB)
  deployment-notes.md     (1.4 KB)
```

### Forgetting a Memory

Delete the file directly:

```bash
rm ~/.edgecrab/memories/deployment-notes.md
```

---

## Full-Text Search

EdgeCrab uses SQLite FTS5 to index all session history. Search from the command line:

```bash
edgecrab search "SQLite WAL mode"
edgecrab search "SSRF protection" --session my-project
edgecrab search "trait bounds" --limit 20
```

Results show the session name, message timestamp, and a highlighted snippet.

---

## Storage Locations

| Path | Contents |
|------|----------|
| `~/.edgecrab/state.db` | All session history (SQLite WAL) |
| `~/.edgecrab/memories/` | Persistent memory Markdown files |
| `~/.edgecrab/skills/` | Reusable skill Markdown files |
| `~/.edgecrab/config.yaml` | Configuration |
| `~/.edgecrab/.env` | API keys (chmod 600) |

### Database Size

The SQLite database grows with every session. For most users, it stays under 50 MB indefinitely. If you want to prune old sessions:

```bash
edgecrab sessions prune --older-than 90d   # Delete sessions older than 90 days
edgecrab sessions prune --keep-last 50     # Keep only the 50 most recent sessions
```

---

## Backup and Restore

```bash
# Backup everything
cp -r ~/.edgecrab ~/.edgecrab.bak-$(date +%Y%m%d)

# Restore
cp -r ~/.edgecrab.bak-20260403 ~/.edgecrab
```

The SQLite database is safe to copy while EdgeCrab is not running (WAL mode guarantees consistency).

---

## Pro Tips

**Name your sessions from the start.** `edgecrab --session "auth-refactor-2026"` makes it trivial to resume and find sessions later. Unnamed sessions get auto-IDs that are hard to remember.

**Export before pruning.** Before running `sessions prune`, export the sessions you want to keep as Markdown for easy reference:
```bash
edgecrab sessions list | grep "important-project" | awk '{print $1}' | \
  xargs -I{} edgecrab sessions export {} --format markdown > archived-sessions.md
```

**Use search as your second brain.** Solved a bug 6 months ago? `edgecrab sessions browse --query "tokio spawn blocking panic"` retrieves the exact context, solution, and code changes.

**Compress context at the threshold.** If sessions get very long (100+ messages), enable compression to keep costs manageable:
```yaml
compression:
  enabled: true
  threshold_tokens: 80000
```

---

## Frequently Asked Questions

**Q: I accidentally deleted a session. Can I recover it?**

No — session deletion is permanent. The FTS5 index is also updated. Always export important sessions before pruning. Consider routine backups with a cron job:
```bash
# Add to crontab: weekly backup at 2 AM Sunday
0 2 * * 0  cp -r ~/.edgecrab ~/.edgecrab.bak-$(date +\%Y\%m\%d)
```

**Q: Sessions list shows duplicate names. Is that a bug?**

No — names are not unique. Multiple sessions can share the same name. The ID is the unique identifier. Use `edgecrab sessions browse --query` or `edgecrab sessions list` with the ID to differentiate.

**Q: I want to continue exactly where I left off in a closed session.**

```bash
edgecrab -C                          # resumes the last CLI session
edgecrab -r "auth-refactor"          # fuzzy-match by title
edgecrab --session abc123            # exact session ID
```

**Q: How do I find a session from 3 weeks ago about a specific bug?**

```bash
edgecrab sessions browse --query "segfault in connection pool"
edgecrab sessions list --limit 100   # all recent sessions
```
FTS5 search is significantly faster than browsing the list manually.

**Q: Can I view session history in a browser?**

Export as Markdown and open in any Markdown viewer:
```bash
edgecrab sessions export my-session --format markdown > session.md
# Then open session.md in your editor or use `glow session.md`
```

---

## See Also

- [SQLite State & Search](/features/state/) — Database schema, FTS5 internals, backup
- [Memory](/features/memory/) — Persistent memory (different from session history)
- [CLI Commands](/reference/cli-commands/) — Full `edgecrab sessions` reference
- [Configuration](/user-guide/configuration/) — Session and compression config
