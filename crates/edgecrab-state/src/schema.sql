-- EdgeCrab session database schema (v6)
-- WAL mode set at connection time, not in schema SQL.

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    user_id TEXT,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    stable_system_prompt TEXT,
    semi_stable_system_prompt TEXT,
    parent_session_id TEXT,
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    last_prompt_tokens INTEGER NOT NULL DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    billing_mode TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL,
    cost_status TEXT,
    cost_source TEXT,
    pricing_version TEXT,
    title TEXT,
    handoff_state TEXT,
    handoff_platform TEXT,
    handoff_error TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id)
);

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_details TEXT,
    codex_reasoning_items TEXT
);

-- FTS5 virtual table for full-text search across all sessions.
-- content= syncs with the messages table via triggers below.
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);

-- FTS5 sync triggers: keep the index in lockstep with messages table.
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content)
    VALUES('delete', old.id, old.content);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content)
    VALUES('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;

-- Indexes for common query patterns.
CREATE INDEX IF NOT EXISTS idx_sessions_source   ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_parent   ON sessions(parent_session_id);
CREATE INDEX IF NOT EXISTS idx_sessions_started  ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session   ON messages(session_id, timestamp);

CREATE INDEX IF NOT EXISTS idx_sessions_title ON sessions(title);

-- Persistent session goals (Ralph loop) — keyed by session_id, survives compression.
CREATE TABLE IF NOT EXISTS session_goals (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    goal_text TEXT NOT NULL,
    created_at REAL NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    turns_used INTEGER NOT NULL DEFAULT 0,
    max_turns INTEGER NOT NULL DEFAULT 20,
    paused_reason TEXT,
    last_verdict TEXT,
    last_reason TEXT,
    consecutive_parse_failures INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS session_subgoals (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES session_goals(session_id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    done INTEGER NOT NULL DEFAULT 0,
    position INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_session_subgoals_session ON session_subgoals(session_id, position);

CREATE TABLE IF NOT EXISTS model_transfers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    from_model TEXT NOT NULL,
    to_model TEXT NOT NULL,
    brief TEXT NOT NULL,
    ts REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_model_transfers_session ON model_transfers(session_id, ts);
