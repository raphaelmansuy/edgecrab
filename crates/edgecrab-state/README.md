# edgecrab-state

> **Why this crate?** An agent without memory forgets everything the moment you close your  
> terminal. `edgecrab-state` gives EdgeCrab a durable, searchable brain: SQLite WAL for  
> sessions, FTS5 for instant full-text search across your entire conversation history, plus  
> a config manager, memory store, and skill store — all behind a clean async API.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Store | Purpose | Key type |
|-------|---------|---------|
| `SessionDb` | Save / resume / search conversations | `Session`, `SessionMessage` |
| `ConfigManager` | Read & write `~/.edgecrab/config.yaml` | `AppConfig` |
| `MemoryStore` | Persistent agent memories (MEMORY.md) | `MemoryEntry` |
| `SkillStore` | Install / list / remove skills | `Skill` |

## Add to your crate

```toml
[dependencies]
edgecrab-state = { path = "../edgecrab-state" }
```

## Quick start

```rust
use edgecrab_state::SessionDb;

// Open (or create) the session database
let db = SessionDb::open("~/.edgecrab/sessions.db").await?;

// Save a session after a conversation
db.save_session(&session).await?;

// Resume a previous session by ID
let messages = db.get_messages(&session_id).await?;

// Full-text search across all sessions (FTS5)
let hits = db.search_sessions("refactor the auth module").await?;
for hit in hits {
    println!("{} — {}", hit.session_id, hit.snippet);
}
```

## Storage layout

```
~/.edgecrab/
├── sessions.db        # SQLite WAL + FTS5 (sessions, messages)
├── config.yaml        # User config (managed by ConfigManager)
├── memories/
│   ├── MEMORY.md      # Agent's long-term episodic memory
│   └── USER.md        # User profile facts
└── skills/            # Installed skill markdown files
    └── my-skill.md
```

## Design notes

- **WAL mode** keeps reads non-blocking while writes are in progress.
- **FTS5** index is updated on every `save_session` call — no manual sync needed.
- **`~/.edgecrab/`** path is overridable via the `EDGECRAB_HOME` env var for profiles and tests.
- All async functions are `tokio`-compatible.

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
