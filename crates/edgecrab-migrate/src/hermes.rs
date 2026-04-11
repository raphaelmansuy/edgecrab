//! # hermes-agent → EdgeCrab migrator
//!
//! WHY migration: Users have existing hermes-agent configs, memories,
//! skills, and sessions in ~/.hermes/. This migrator copies and converts
//! them to ~/.edgecrab/ format so they don't lose their setup.
//!
//! ```text
//!   HermesMigrator
//!     ├── migrate_config()    — YAML config (compatible, minor key renames)
//!     ├── migrate_state()     — Import Hermes SQLite sessions/messages
//!     ├── migrate_memory()    — Copy memory files (MEMORY.md, USER.md)
//!     ├── migrate_skills()    — Copy skills/ directory tree
//!     ├── migrate_env()       — Copy .env with key aliasing
//!     └── migrate_all()       — Run all migrations, produce report
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::{Connection, OpenFlags, params};

use crate::common::{copy_dir_recursive, ensure_dir};
use crate::report::{MigrationItem, MigrationReport, MigrationStatus};

/// Migrates hermes-agent (~/.hermes/) → EdgeCrab (~/.edgecrab/).
pub struct HermesMigrator {
    hermes_home: PathBuf,
    edgecrab_home: PathBuf,
}

impl HermesMigrator {
    pub fn new(hermes_home: PathBuf, edgecrab_home: PathBuf) -> Self {
        Self {
            hermes_home,
            edgecrab_home,
        }
    }

    /// Run all migration steps and return a report.
    pub fn migrate_all(&self) -> anyhow::Result<MigrationReport> {
        let mut report = MigrationReport::new("hermes-agent → EdgeCrab");

        report.add(self.migrate_config());
        report.add(self.migrate_state());
        report.add(self.migrate_memory());
        report.add(self.migrate_skills());
        report.add(self.migrate_env());

        Ok(report)
    }

    /// Migrate config.yaml — mostly compatible, just copy.
    pub fn migrate_config(&self) -> MigrationItem {
        let src = self.hermes_home.join("config.yaml");
        let dst = self.edgecrab_home.join("config.yaml");

        if !src.exists() {
            return MigrationItem::skipped("config", "no config.yaml found in hermes home");
        }

        if dst.exists() {
            return MigrationItem::skipped("config", "config.yaml already exists in edgecrab home");
        }

        match std::fs::read_to_string(&src) {
            Ok(content) => {
                // Remove base_url if it's the OpenRouter default (edgequake-llm auto-detects)
                let migrated = content
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("base_url:"))
                    .collect::<Vec<_>>()
                    .join("\n");

                if let Err(e) = ensure_dir(&self.edgecrab_home) {
                    return MigrationItem::failed("config", &format!("failed to create dir: {e}"));
                }
                match std::fs::write(&dst, migrated) {
                    Ok(()) => MigrationItem::success("config"),
                    Err(e) => MigrationItem::failed("config", &format!("write failed: {e}")),
                }
            }
            Err(e) => MigrationItem::failed("config", &format!("read failed: {e}")),
        }
    }

    /// Migrate Hermes SQLite session state into EdgeCrab's state.db.
    ///
    /// WHY import instead of file-copy:
    /// - preserves an existing EdgeCrab state.db instead of clobbering it
    /// - avoids depending on source-side WAL/shm sidecars being in sync
    /// - lets us skip duplicate session IDs deterministically
    pub fn migrate_state(&self) -> MigrationItem {
        let src = self.hermes_home.join("state.db");
        let dst = self.edgecrab_home.join("state.db");

        if !src.exists() {
            return MigrationItem::skipped("state", "no state.db found in hermes home");
        }

        if let Err(e) = ensure_dir(&self.edgecrab_home) {
            return MigrationItem::failed("state", &format!("dir create failed: {e}"));
        }

        // Ensure the target schema exists before importing into it.
        if let Err(e) = edgecrab_state::SessionDb::open(&dst) {
            return MigrationItem::failed("state", &format!("target init failed: {e}"));
        }

        match self.import_state_db(&src, &dst) {
            Ok(StateImportSummary {
                total_sessions,
                imported_sessions,
                skipped_sessions,
                imported_messages,
            }) => {
                if total_sessions == 0 {
                    MigrationItem::skipped("state", "state.db exists but contains no sessions")
                } else if imported_sessions == 0 {
                    MigrationItem::skipped(
                        "state",
                        &format!(
                            "all {total_sessions} Hermes session(s) already exist in edgecrab state.db"
                        ),
                    )
                } else {
                    let detail = if skipped_sessions > 0 {
                        format!(
                            "imported {imported_sessions}/{total_sessions} session(s) and \
                             {imported_messages} message(s); skipped {skipped_sessions} existing session(s)"
                        )
                    } else {
                        format!(
                            "imported {imported_sessions} session(s) and {imported_messages} message(s)"
                        )
                    };
                    MigrationItem::new("state", MigrationStatus::Success, &detail)
                }
            }
            Err(e) => MigrationItem::failed("state", &e.to_string()),
        }
    }

    /// Migrate memory files (MEMORY.md, USER.md, memories/).
    pub fn migrate_memory(&self) -> MigrationItem {
        let src_dir = self.hermes_home.join("memories");
        let dst_dir = self.edgecrab_home.join("memories");

        if !src_dir.exists() {
            // Try legacy single-file MEMORY.md
            let legacy = self.hermes_home.join("MEMORY.md");
            if legacy.exists() {
                if let Err(e) = ensure_dir(&dst_dir) {
                    return MigrationItem::failed("memory", &format!("dir create failed: {e}"));
                }
                let dst = dst_dir.join("MEMORY.md");
                if dst.exists() {
                    return MigrationItem::skipped("memory", "MEMORY.md already exists");
                }
                return match std::fs::copy(&legacy, &dst) {
                    Ok(_) => MigrationItem::success("memory"),
                    Err(e) => MigrationItem::failed("memory", &format!("copy failed: {e}")),
                };
            }
            return MigrationItem::skipped("memory", "no memories directory found");
        }

        if dst_dir.exists() {
            return MigrationItem::skipped("memory", "memories directory already exists");
        }

        match copy_dir_recursive(&src_dir, &dst_dir) {
            Ok(count) => MigrationItem::new(
                "memory",
                MigrationStatus::Success,
                &format!("copied {count} files"),
            ),
            Err(e) => MigrationItem::failed("memory", &format!("copy failed: {e}")),
        }
    }

    /// Migrate skills/ directory tree.
    pub fn migrate_skills(&self) -> MigrationItem {
        let src_dir = self.hermes_home.join("skills");
        let dst_dir = self.edgecrab_home.join("skills");

        if !src_dir.exists() {
            return MigrationItem::skipped("skills", "no skills directory found");
        }

        if dst_dir.exists() {
            return MigrationItem::skipped("skills", "skills directory already exists");
        }

        match copy_dir_recursive(&src_dir, &dst_dir) {
            Ok(count) => MigrationItem::new(
                "skills",
                MigrationStatus::Success,
                &format!("copied {count} skill files"),
            ),
            Err(e) => MigrationItem::failed("skills", &format!("copy failed: {e}")),
        }
    }

    /// Migrate .env file (API keys).
    pub fn migrate_env(&self) -> MigrationItem {
        let src = self.hermes_home.join(".env");
        let dst = self.edgecrab_home.join(".env");

        if !src.exists() {
            return MigrationItem::skipped("env", "no .env file found");
        }

        if dst.exists() {
            return MigrationItem::skipped("env", ".env already exists in edgecrab home");
        }

        if let Err(e) = ensure_dir(&self.edgecrab_home) {
            return MigrationItem::failed("env", &format!("dir create failed: {e}"));
        }

        match std::fs::copy(&src, &dst) {
            Ok(_) => MigrationItem::success("env"),
            Err(e) => MigrationItem::failed("env", &format!("copy failed: {e}")),
        }
    }

    fn import_state_db(&self, src: &Path, dst: &Path) -> anyhow::Result<StateImportSummary> {
        let src_conn = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .with_context(|| format!("cannot open Hermes state DB {}", src.display()))?;
        let mut dst_conn = Connection::open(dst)
            .with_context(|| format!("cannot open EdgeCrab state DB {}", dst.display()))?;
        dst_conn
            .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")
            .context("cannot configure target state DB")?;

        let total_sessions: usize = src_conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| {
                row.get::<_, i64>(0)
            })
            .context("cannot count Hermes sessions")? as usize;
        if total_sessions == 0 {
            return Ok(StateImportSummary::default());
        }

        let existing_sessions: HashSet<String> = {
            let mut stmt = dst_conn
                .prepare("SELECT id FROM sessions")
                .context("cannot query existing EdgeCrab sessions")?;
            stmt.query_map([], |row| row.get::<_, String>(0))
                .context("cannot iterate existing EdgeCrab sessions")?
                .filter_map(|row| row.ok())
                .collect()
        };

        let mut src_sessions = src_conn
            .prepare(
                "SELECT
                    id, source, user_id, model, model_config, system_prompt, parent_session_id,
                    started_at, ended_at, end_reason, message_count, tool_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    reasoning_tokens, billing_provider, billing_base_url, billing_mode,
                    estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                    pricing_version, title
                 FROM sessions
                 ORDER BY started_at ASC, id ASC",
            )
            .context("cannot read Hermes sessions")?;

        let source_sessions: Vec<SourceSessionRow> = src_sessions
            .query_map([], |row| {
                Ok(SourceSessionRow {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    user_id: row.get(2)?,
                    model: row.get(3)?,
                    model_config: row.get(4)?,
                    system_prompt: row.get(5)?,
                    parent_session_id: row.get(6)?,
                    started_at: row.get(7)?,
                    ended_at: row.get(8)?,
                    end_reason: row.get(9)?,
                    message_count: row.get(10)?,
                    tool_call_count: row.get(11)?,
                    input_tokens: row.get(12)?,
                    output_tokens: row.get(13)?,
                    cache_read_tokens: row.get(14)?,
                    cache_write_tokens: row.get(15)?,
                    reasoning_tokens: row.get(16)?,
                    billing_provider: row.get(17)?,
                    billing_base_url: row.get(18)?,
                    billing_mode: row.get(19)?,
                    estimated_cost_usd: row.get(20)?,
                    actual_cost_usd: row.get(21)?,
                    cost_status: row.get(22)?,
                    cost_source: row.get(23)?,
                    pricing_version: row.get(24)?,
                    title: row.get(25)?,
                })
            })
            .context("cannot iterate Hermes sessions")?
            .collect::<Result<Vec<_>, _>>()
            .context("cannot decode Hermes sessions")?;

        let mut pending_sessions = Vec::new();
        let mut skipped_sessions = 0usize;
        for session in source_sessions {
            if existing_sessions.contains(&session.id) {
                skipped_sessions += 1;
            } else {
                pending_sessions.push(session);
            }
        }

        let tx = dst_conn
            .transaction()
            .context("cannot begin target transaction")?;
        let mut insert_session = tx
            .prepare(
                "INSERT INTO sessions (
                    id, source, user_id, model, model_config, system_prompt, parent_session_id,
                    started_at, ended_at, end_reason, message_count, tool_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    reasoning_tokens, billing_provider, billing_base_url, billing_mode,
                    estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                    pricing_version, title
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
                 )",
            )
            .context("cannot prepare target session insert")?;
        let mut insert_message = tx
            .prepare(
                "INSERT INTO messages (
                    session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason, reasoning,
                    reasoning_details, codex_reasoning_items
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )
            .context("cannot prepare target message insert")?;

        let mut imported_sessions = 0usize;
        let mut imported_messages = 0usize;
        let mut available_parents = existing_sessions;

        while !pending_sessions.is_empty() {
            let mut next_pending = Vec::new();
            let mut progressed = false;

            for session in pending_sessions {
                let parent_ready = session
                    .parent_session_id
                    .as_ref()
                    .is_none_or(|parent_id| available_parents.contains(parent_id));

                if !parent_ready {
                    next_pending.push(session);
                    continue;
                }

                insert_session
                    .execute(params![
                        &session.id,
                        &session.source,
                        session.user_id.as_deref(),
                        session.model.as_deref(),
                        session.model_config.as_deref(),
                        session.system_prompt.as_deref(),
                        session.parent_session_id.as_deref(),
                        session.started_at,
                        session.ended_at,
                        session.end_reason.as_deref(),
                        session.message_count,
                        session.tool_call_count,
                        session.input_tokens,
                        session.output_tokens,
                        session.cache_read_tokens,
                        session.cache_write_tokens,
                        session.reasoning_tokens,
                        session.billing_provider.as_deref(),
                        session.billing_base_url.as_deref(),
                        session.billing_mode.as_deref(),
                        session.estimated_cost_usd,
                        session.actual_cost_usd,
                        session.cost_status.as_deref(),
                        session.cost_source.as_deref(),
                        session.pricing_version.as_deref(),
                        session.title.as_deref(),
                    ])
                    .with_context(|| format!("cannot insert session {}", session.id))?;

                let mut src_messages = src_conn
                    .prepare(
                        "SELECT
                            role, content, tool_call_id, tool_calls, tool_name, timestamp,
                            token_count, finish_reason, reasoning, reasoning_details,
                            codex_reasoning_items
                         FROM messages
                         WHERE session_id = ?1
                         ORDER BY timestamp ASC, id ASC",
                    )
                    .with_context(|| format!("cannot prepare messages query for {}", session.id))?;

                let message_rows = src_messages
                    .query_map(params![session.id.as_str()], |row| {
                        Ok(SourceMessageRow {
                            role: row.get(0)?,
                            content: row.get(1)?,
                            tool_call_id: row.get(2)?,
                            tool_calls: row.get(3)?,
                            tool_name: row.get(4)?,
                            timestamp: row.get(5)?,
                            token_count: row.get(6)?,
                            finish_reason: row.get(7)?,
                            reasoning: row.get(8)?,
                            reasoning_details: row.get(9)?,
                            codex_reasoning_items: row.get(10)?,
                        })
                    })
                    .with_context(|| format!("cannot read messages for {}", session.id))?;

                for message in message_rows {
                    let message = message
                        .with_context(|| format!("cannot decode message for {}", session.id))?;
                    insert_message
                        .execute(params![
                            &session.id,
                            &message.role,
                            message.content.as_deref(),
                            message.tool_call_id.as_deref(),
                            message.tool_calls.as_deref(),
                            message.tool_name.as_deref(),
                            message.timestamp,
                            message.token_count,
                            message.finish_reason.as_deref(),
                            message.reasoning.as_deref(),
                            message.reasoning_details.as_deref(),
                            message.codex_reasoning_items.as_deref(),
                        ])
                        .with_context(|| format!("cannot insert message for {}", session.id))?;
                    imported_messages += 1;
                }

                available_parents.insert(session.id.clone());
                imported_sessions += 1;
                progressed = true;
            }

            if !progressed {
                let unresolved = next_pending
                    .iter()
                    .map(|session| {
                        format!(
                            "{} -> {}",
                            session.id,
                            session.parent_session_id.as_deref().unwrap_or("<none>")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!(
                    "cannot import Hermes sessions: unresolved parent_session_id chain(s): {unresolved}"
                );
            }

            pending_sessions = next_pending;
        }

        drop(insert_message);
        drop(insert_session);
        tx.commit().context("cannot commit target state import")?;

        Ok(StateImportSummary {
            total_sessions,
            imported_sessions,
            skipped_sessions,
            imported_messages,
        })
    }
}

#[derive(Debug, Default)]
struct StateImportSummary {
    total_sessions: usize,
    imported_sessions: usize,
    skipped_sessions: usize,
    imported_messages: usize,
}

#[derive(Debug)]
struct SourceSessionRow {
    id: String,
    source: String,
    user_id: Option<String>,
    model: Option<String>,
    model_config: Option<String>,
    system_prompt: Option<String>,
    parent_session_id: Option<String>,
    started_at: f64,
    ended_at: Option<f64>,
    end_reason: Option<String>,
    message_count: i64,
    tool_call_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
    cache_write_tokens: i64,
    reasoning_tokens: i64,
    billing_provider: Option<String>,
    billing_base_url: Option<String>,
    billing_mode: Option<String>,
    estimated_cost_usd: Option<f64>,
    actual_cost_usd: Option<f64>,
    cost_status: Option<String>,
    cost_source: Option<String>,
    pricing_version: Option<String>,
    title: Option<String>,
}

#[derive(Debug)]
struct SourceMessageRow {
    role: String,
    content: Option<String>,
    tool_call_id: Option<String>,
    tool_calls: Option<String>,
    tool_name: Option<String>,
    timestamp: f64,
    token_count: Option<i64>,
    finish_reason: Option<String>,
    reasoning: Option<String>,
    reasoning_details: Option<String>,
    codex_reasoning_items: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_hermes_home(dir: &Path) {
        std::fs::create_dir_all(dir).expect("create dir");
        std::fs::write(
            dir.join("config.yaml"),
            "model:\n  default: claude\n  base_url: https://openrouter.ai/api/v1\n",
        )
        .expect("write config");
        std::fs::write(dir.join(".env"), "OPENROUTER_API_KEY=sk-test\n").expect("write env");

        let memories = dir.join("memories");
        std::fs::create_dir_all(&memories).expect("create memories");
        std::fs::write(memories.join("MEMORY.md"), "§ Remember: user likes Rust").expect("write");

        let skills = dir.join("skills").join("test-skill");
        std::fs::create_dir_all(&skills).expect("create skills");
        std::fs::write(skills.join("SKILL.md"), "# Test Skill").expect("write skill");
    }

    #[test]
    fn migrate_all_fresh() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        setup_hermes_home(&hermes);

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let report = migrator.migrate_all().expect("migrate");

        assert_eq!(report.success_count(), 4);
        assert_eq!(report.skipped_count(), 1);
        assert_eq!(report.failed_count(), 0);

        // Config — base_url removed
        let config = std::fs::read_to_string(edgecrab.join("config.yaml")).expect("read");
        assert!(config.contains("default: claude"));
        assert!(!config.contains("base_url"));

        // Env
        assert!(edgecrab.join(".env").exists());

        // Memories
        assert!(edgecrab.join("memories").join("MEMORY.md").exists());

        // Skills
        assert!(
            edgecrab
                .join("skills")
                .join("test-skill")
                .join("SKILL.md")
                .exists()
        );
    }

    #[test]
    fn migrate_state_imports_sessions_and_messages() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        std::fs::create_dir_all(&hermes).expect("create hermes dir");

        let source = Connection::open(hermes.join("state.db")).expect("open source db");
        source
            .execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
            .expect("init source schema");
        source
            .execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .expect("schema version");
        source
            .execute(
                "INSERT INTO sessions (
                    id, source, user_id, model, model_config, system_prompt, parent_session_id,
                    started_at, ended_at, end_reason, message_count, tool_call_count,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    reasoning_tokens, billing_provider, billing_base_url, billing_mode,
                    estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                    pricing_version, title
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26
                 )",
                params![
                    "sess-hermes-1",
                    "cli",
                    "user-1",
                    "anthropic/claude-sonnet-4",
                    Some(r#"{"temperature":0.2}"#),
                    "system prompt",
                    Option::<String>::None,
                    1_700_000_000.0_f64,
                    1_700_000_300.0_f64,
                    "user_exit",
                    3_i64,
                    1_i64,
                    120_i64,
                    55_i64,
                    10_i64,
                    5_i64,
                    2_i64,
                    Some("openrouter"),
                    Some("https://openrouter.ai/api/v1"),
                    Some("estimated"),
                    Some(0.42_f64),
                    Some(0.40_f64),
                    Some("final"),
                    Some("provider"),
                    Some("2026-04"),
                    Some("Migrated Session"),
                ],
            )
            .expect("insert source session");
        source
            .execute(
                "INSERT INTO messages (
                    session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason, reasoning, reasoning_details,
                    codex_reasoning_items
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    "sess-hermes-1",
                    "user",
                    "remember the migration gap",
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<String>::None,
                    1_700_000_001.0_f64,
                    Some(12_i64),
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<String>::None,
                    Option::<String>::None,
                ],
            )
            .expect("insert source user message");
        source
            .execute(
                "INSERT INTO messages (
                    session_id, role, content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason, reasoning, reasoning_details,
                    codex_reasoning_items
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    "sess-hermes-1",
                    "tool",
                    "search complete",
                    Some("call_1"),
                    Option::<String>::None,
                    Some("session_search"),
                    1_700_000_002.0_f64,
                    Some(3_i64),
                    Some("stop"),
                    Some("chain"),
                    Some(r#"{"steps":[1]}"#),
                    Some(r#"[{"kind":"reasoning"}]"#),
                ],
            )
            .expect("insert source tool message");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let item = migrator.migrate_state();
        assert_eq!(item.status, MigrationStatus::Success);
        assert!(item.detail.contains("1 session"));
        assert!(item.detail.contains("2 message"));

        let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("open target");
        let session = db
            .get_session("sess-hermes-1")
            .expect("get")
            .expect("found");
        assert_eq!(session.title.as_deref(), Some("Migrated Session"));
        assert_eq!(session.message_count, 3);
        assert_eq!(session.input_tokens, 120);
        assert_eq!(session.output_tokens, 55);

        let messages = db.get_messages("sess-hermes-1").expect("messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text_content(), "remember the migration gap");
        assert_eq!(messages[1].role.as_str(), "tool");
        assert_eq!(messages[1].name.as_deref(), Some("session_search"));
        assert_eq!(messages[1].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages[1].reasoning.as_deref(), Some("chain"));

        let search = db.search("migration", 10).expect("search");
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].session_id, "sess-hermes-1");
    }

    #[test]
    fn migrate_state_skips_existing_session_ids() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        std::fs::create_dir_all(&hermes).expect("create hermes dir");
        std::fs::create_dir_all(&edgecrab).expect("create edgecrab dir");

        let source = Connection::open(hermes.join("state.db")).expect("open source db");
        source
            .execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
            .expect("init source schema");
        source
            .execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .expect("schema version");
        source
            .execute(
                "INSERT INTO sessions (id, source, started_at) VALUES (?1, ?2, ?3)",
                params!["shared-id", "cli", 1_700_000_000.0_f64],
            )
            .expect("insert source session");

        let target = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("target");
        target
            .save_session(&edgecrab_state::SessionRecord {
                id: "shared-id".into(),
                source: "cli".into(),
                user_id: None,
                model: Some("existing/model".into()),
                system_prompt: None,
                parent_session_id: None,
                started_at: 1_800_000_000.0_f64,
                ended_at: None,
                end_reason: None,
                message_count: 0,
                tool_call_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: None,
                title: Some("Existing".into()),
            })
            .expect("save existing session");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let item = migrator.migrate_state();
        assert_eq!(item.status, MigrationStatus::Skipped);
        assert!(item.detail.contains("already exist"));

        let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("reopen");
        let session = db.get_session("shared-id").expect("get").expect("found");
        assert_eq!(session.title.as_deref(), Some("Existing"));
        assert_eq!(session.started_at, 1_800_000_000.0_f64);
    }

    #[test]
    fn migrate_state_imports_parent_chains_even_when_source_order_is_unlucky() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        std::fs::create_dir_all(&hermes).expect("create hermes dir");

        let source = Connection::open(hermes.join("state.db")).expect("open source db");
        source
            .execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
            .expect("init source schema");
        source
            .execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .expect("schema version");

        // Insert parent first to satisfy source DB integrity, but give the child an
        // earlier timestamp to prove import does not rely on timestamp order.
        source
            .execute(
                "INSERT INTO sessions (id, source, started_at, title)
                 VALUES (?1, ?2, ?3, ?4)",
                params!["parent-session", "cli", 1_700_000_100.0_f64, Some("Parent"),],
            )
            .expect("insert parent session");
        source
            .execute(
                "INSERT INTO sessions (id, source, parent_session_id, started_at, title)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    "child-session",
                    "cli",
                    Some("parent-session"),
                    1_700_000_000.0_f64,
                    Some("Child"),
                ],
            )
            .expect("insert child session");
        source
            .execute(
                "INSERT INTO messages (session_id, role, content, timestamp)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "child-session",
                    "user",
                    "child content",
                    1_700_000_001.0_f64
                ],
            )
            .expect("insert child message");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let item = migrator.migrate_state();
        assert_eq!(item.status, MigrationStatus::Success);
        assert!(item.detail.contains("2 session"));

        let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("open target");
        let child = db
            .get_session("child-session")
            .expect("get child")
            .expect("child exists");
        assert_eq!(child.parent_session_id.as_deref(), Some("parent-session"));
        let messages = db
            .get_messages("child-session")
            .expect("get child messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text_content(), "child content");
    }

    #[test]
    fn migrate_state_allows_children_of_existing_target_sessions() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        std::fs::create_dir_all(&hermes).expect("create hermes dir");
        std::fs::create_dir_all(&edgecrab).expect("create edgecrab dir");

        let source = Connection::open(hermes.join("state.db")).expect("open source db");
        source
            .execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
            .expect("init source schema");
        source
            .execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .expect("schema version");
        source
            .execute(
                "INSERT INTO sessions (id, source, started_at)
                 VALUES (?1, ?2, ?3)",
                params!["existing-parent", "cli", 1_699_999_000.0_f64],
            )
            .expect("insert source parent");
        source
            .execute(
                "INSERT INTO sessions (id, source, parent_session_id, started_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "new-child",
                    "cli",
                    Some("existing-parent"),
                    1_700_000_000.0_f64,
                ],
            )
            .expect("insert source child");

        let target = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("target");
        target
            .save_session(&edgecrab_state::SessionRecord {
                id: "existing-parent".into(),
                source: "cli".into(),
                user_id: None,
                model: None,
                system_prompt: None,
                parent_session_id: None,
                started_at: 1_699_999_000.0_f64,
                ended_at: None,
                end_reason: None,
                message_count: 0,
                tool_call_count: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: None,
                title: Some("Existing Parent".into()),
            })
            .expect("save existing parent");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let item = migrator.migrate_state();
        assert_eq!(item.status, MigrationStatus::Success);

        let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("reopen");
        let child = db
            .get_session("new-child")
            .expect("get child")
            .expect("child exists");
        assert_eq!(child.parent_session_id.as_deref(), Some("existing-parent"));
    }

    #[test]
    fn migrate_state_fails_safely_on_orphan_parent_chain() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        std::fs::create_dir_all(&hermes).expect("create hermes dir");

        let source = Connection::open(hermes.join("state.db")).expect("open source db");
        source
            .execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
            .expect("init source schema");
        source
            .execute("INSERT INTO schema_version (version) VALUES (6)", [])
            .expect("schema version");
        source
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .expect("disable source foreign keys for corrupted fixture");
        source
            .execute(
                "INSERT INTO sessions (id, source, parent_session_id, started_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "orphan-child",
                    "cli",
                    Some("missing-parent"),
                    1_700_000_000.0_f64,
                ],
            )
            .expect("insert orphan child");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let item = migrator.migrate_state();
        assert_eq!(item.status, MigrationStatus::Failed);
        assert!(item.detail.contains("unresolved parent_session_id chain"));

        let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("open target");
        assert!(
            db.get_session("orphan-child")
                .expect("get session")
                .is_none(),
            "transaction should roll back on invalid parent chains"
        );
    }

    #[test]
    fn migrate_skips_existing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes");
        let edgecrab = tmp.path().join("edgecrab");
        setup_hermes_home(&hermes);

        // Create existing edgecrab files
        std::fs::create_dir_all(&edgecrab).expect("create");
        std::fs::write(edgecrab.join("config.yaml"), "existing: true\n").expect("write");

        let migrator = HermesMigrator::new(hermes, edgecrab.clone());
        let report = migrator.migrate_all().expect("migrate");

        // Config should be skipped (already exists)
        assert!(report.skipped_count() >= 1);

        // Existing config untouched
        let content = std::fs::read_to_string(edgecrab.join("config.yaml")).expect("read");
        assert_eq!(content, "existing: true\n");
    }

    #[test]
    fn migrate_empty_hermes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let hermes = tmp.path().join("hermes_empty");
        let edgecrab = tmp.path().join("edgecrab_empty");
        std::fs::create_dir_all(&hermes).expect("create");

        let migrator = HermesMigrator::new(hermes, edgecrab);
        let report = migrator.migrate_all().expect("migrate");

        // Everything skipped — nothing to migrate
        assert_eq!(report.success_count(), 0);
        assert_eq!(report.skipped_count(), 5);
    }
}
