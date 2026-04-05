---
title: Memory
description: Persistent memory system — file-based memory in ~/.edgecrab/memories/ plus Honcho cross-session user modeling. Grounded in crates/edgecrab-tools/src/tools/memory.rs and config.rs.
sidebar:
  order: 6
---

EdgeCrab has two complementary memory layers: file-based session memory and Honcho user modeling.

---

## File-Based Memory

Memory files live in `~/.edgecrab/memories/` (or profile-specific if using profiles). They are plain Markdown files that persist between sessions.

### Auto-Flush

When `memory.auto_flush: true` (the default), EdgeCrab automatically saves all new memory at the end of each session.

```yaml
memory:
  enabled: true
  auto_flush: true
```

Disable for a single session:

```bash
edgecrab --skip-memory "don't remember this session"
```

### Memory Tools

The agent uses these tools to manage memory during a session:

| Tool | Description |
|------|-------------|
| `memory_read` | Read a memory file by name |
| `memory_write` | Write or update a memory file |

Example agent interaction:

```
❯ Remember that this project uses PostgreSQL 16 with the pgvector extension
```

The agent writes a memory entry. Next session, it reads and injects `memories/*.md` into the system prompt.

### Session Injection

At the start of each session, EdgeCrab injects all memory files under `Persistent Memory` in the system prompt. This is exactly the same injection that Hermes Agent does.

---

## Honcho User Modeling

Honcho provides cross-session, semantic user modeling. It stores facts about the user's working patterns, preferences, and context — then retrieves the most relevant facts at the start of each session.

### Configuration

```yaml
honcho:
  enabled: true                  # master switch
  cloud_sync: false              # sync to Honcho cloud (requires HONCHO_API_KEY)
  api_key_env: "HONCHO_API_KEY"
  api_url: "https://api.honcho.dev/v1"
  max_context_entries: 10        # facts injected per session
  write_frequency: 0             # auto-conclude every N messages (0 = manual)
```

Enable cloud sync:

```bash
export HONCHO_API_KEY=sk-honcho-xxx
# Setting the key auto-enables cloud_sync
```

### Honcho Tools

| Tool | Description |
|------|-------------|
| `honcho_conclude` | Commit a structured memory entry (agent calls at session end) |
| `honcho_search` | Semantic search across Honcho user model |
| `honcho_list` | List stored Honcho entries |
| `honcho_remove` | Delete a specific entry |
| `honcho_profile` | Update user profile summary |
| `honcho_context` | Retrieve top-K relevant entries for the current task |

### How It Works

1. **During session:** Honcho context is injected into the system prompt via `honcho_context`
2. **End of session:** Agent calls `honcho_conclude` with a summary of what it learned
3. **Next session:** Top-K relevant entries are retrieved and injected

All Honcho storage is local by default (`cloud_sync: false`). With cloud sync enabled, facts are stored and retrieved from the Honcho API.

---

## Disabling Memory

```bash
# Skip memory for one session
edgecrab --skip-memory "ephemeral task"

# Disable globally
# config.yaml
memory:
  enabled: false

# Disable Honcho
honcho:
  enabled: false
```

---

## The `/memory` Slash Command

From the TUI:

```
/memory           # show all memory files and their sizes
```

This opens a summary view of `~/.edgecrab/memories/` with file names, sizes, and last-modified times.

---

## Memory File Structure

Memory files are plain Markdown. The agent reads all of them at session start. Keep them focused and structured:

```markdown
# ~/.edgecrab/memories/projects.md

## Active Projects
- edgecrab: Rust workspace at ~/Github/edgecrab. Uses tokio, axum, ratatui.
- my-api: Python FastAPI at ~/work/my-api. PostgreSQL backend.

## Work Patterns
- Prefer to run tests before marking tasks complete
- Use conventional commits (feat:, fix:, docs:, etc.)
- Always check for existing tests before writing new ones
```

---

## Pro Tips

**Seed your memory on day one.** Don't wait for the agent to auto-write it. Create `~/.edgecrab/memories/me.md` with your name, timezone, preferred languages, and coding style. The agent will use this immediately.

**Review memory after long sessions.** Run `cat ~/.edgecrab/memories/*.md` to see what the agent stored. Edit or delete stale facts — the agent doesn't prune memory automatically.

**Use profile memories for context switching.** When switching between work and personal projects, use profiles. Each profile has its own `memories/` directory, keeping contexts clean.

**Keep individual files focused.** `preferences.md`, `projects.md`, `snippets.md` are easier to maintain than one giant `memory.md`.

---

## Frequently Asked Questions

**Q: The agent keeps forgetting things I told it. Why?**

Check that `memory.auto_flush: true` is set in `config.yaml`. Also verify the agent is writing the memory (watch for `memory_write` tool calls in the TUI). If you told it verbally but didn't ask it to "remember", it may not have saved it.

**Q: Can I edit memory files manually?**

Yes — they're plain Markdown files. Open `~/.edgecrab/memories/` in your editor. Changes take effect at the next session start.

**Q: Memory keeps growing. How do I manage it?**

Periodically run:
```bash
edgecrab "review our memory files, remove anything outdated, and consolidate duplicates"
```
Or manually edit `~/.edgecrab/memories/`. There's no automatic pruning.

**Q: What's the difference between memory and AGENTS.md?**

| | Memory (`memories/*.md`) | Context Files (`AGENTS.md`) |
|--|--------------------------|----------------------------|
| **Scope** | Personal facts about you | Project instructions |
| **Who writes it** | Agent auto-writes | You write |
| **Portability** | Follows you everywhere | Belongs to the project repo |
| **When applied** | Every session | Session started in that project dir |

**Q: I accidentally deleted an important memory. How do I recover it?**

If you keep `~/.edgecrab/` under version control or have a backup, restore from there. Otherwise, the session history in `state.db` is preserved — search it with `edgecrab sessions search "keyword"` and reconstruct what was lost.

---

## See Also

- [Context Files](/features/context-files/) — SOUL.md, AGENTS.md, and project context
- [Sessions](/user-guide/sessions/) — Full session history and search
- [Profiles](/user-guide/profiles/) — Per-profile memory isolation
- [Configuration](/user-guide/configuration/) — `memory.*` and `honcho.*` config keys
