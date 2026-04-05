# 012.001 — Migration Guide

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 009 Config](../009_config_state/001_config_state.md) | [→ 001 Overview](../001_overview/001_project_summary.md)
> **Source**: `edgecrab-migrate/` — verified against real implementation

## 1. Migration Paths

EdgeCrab provides a migration utility (`edgecrab migrate`) that imports configs,
memories, skills, and sessions from compatible agent installations:

```
+----------------------+     +-------------------+
|  Nous Hermes agent   |     |     OpenClaw       |
|  ~/.hermes/          |     |  ~/.claw/ or       |
|                      |     |  ~/.openclaw/      |
+----------+-----------+     +---------+----------+
           |                           |
           v                           v
   +-----------------------------------------------+
   |            edgecrab-migrate                    |
   |   (`edgecrab migrate` CLI subcommand)          |
   |   Converts: config, memory, skills, sessions   |
   +-----------------------------------------------+
```

## 2. Nous Hermes Agent → EdgeCrab Migration

### 2.1 Config Migration

```yaml
# hermes-agent: ~/.hermes/config.yaml
model:
  default: "anthropic/claude-sonnet-4-20250514"
  base_url: "https://openrouter.ai/api/v1"
  max_iterations: 90

# EdgeCrab: ~/.edgecrab/config.yaml (same format, auto-migrated)
model:
  default: "anthropic/claude-sonnet-4-20250514"
  max_iterations: 90
  # base_url removed — edgequake-llm auto-detects from model name
```

### 2.2 Migration CLI

```bash
# Auto-migrate everything
edgecrab migrate --from hermes

# Selective migration
edgecrab migrate --from hermes --only config
edgecrab migrate --from hermes --only sessions
edgecrab migrate --from hermes --only memory
edgecrab migrate --from hermes --only skills
```

### 2.3 Migration Steps

```rust
// edgecrab-migrate/src/hermes.rs

pub struct HermesMigrator {
    hermes_home: PathBuf,    // ~/.hermes/
    edgecrab_home: PathBuf,  // ~/.edgecrab/
}

impl HermesMigrator {
    pub fn migrate_all(&self) -> Result<MigrationReport> {
        let mut report = MigrationReport::new();

        // 1. Config
        report.add(self.migrate_config()?);

        // 2. Memory files (MEMORY.md, USER.md) — direct copy
        report.add(self.migrate_memory()?);

        // 3. Skills — copy directory tree
        report.add(self.migrate_skills()?);

        // 4. Sessions DB — SQLite schema migration
        report.add(self.migrate_sessions()?);

        // 5. API keys (.env) — copy with key rename if needed
        report.add(self.migrate_env()?);

        // 6. Skins — convert theme format
        report.add(self.migrate_skins()?);

        Ok(report)
    }

    fn migrate_sessions(&self) -> Result<MigrationItem> {
        // hermes state.db → edgecrab state.db
        // Schema is compatible — copy + add any new columns
        let src = self.hermes_home.join("state.db");
        let dst = self.edgecrab_home.join("state.db");
        if src.exists() {
            std::fs::copy(&src, &dst)?;
            // Run ALTER TABLE for new columns
            let conn = rusqlite::Connection::open(&dst)?;
            conn.execute_batch("
                ALTER TABLE sessions ADD COLUMN IF NOT EXISTS platform TEXT;
                ALTER TABLE sessions ADD COLUMN IF NOT EXISTS total_cost REAL DEFAULT 0.0;
            ")?;
        }
        Ok(MigrationItem::success("sessions"))
    }
}
```

## 3. OpenClaw → EdgeCrab Migration

### 3.1 Key Differences

| Aspect | OpenClaw | EdgeCrab |
|--------|----------|----------|
| Config dir | `~/.openclaw/` | `~/.edgecrab/` |
| Config format | JSON | YAML |
| Session storage | JSON files | SQLite |
| Skills format | Different SKILL.md layout | hermes-compatible |
| API key env vars | `OPENCLAW_API_KEY` | `OPENROUTER_API_KEY` (+ aliases) |
| CLI binary | `openclaw` | `edgecrab` |

### 3.2 OpenClaw Migration

```rust
// edgecrab-migrate/src/openclaw.rs

pub struct OpenClawMigrator {
    openclaw_home: PathBuf,
    edgecrab_home: PathBuf,
}

impl OpenClawMigrator {
    pub fn migrate_config(&self) -> Result<MigrationItem> {
        // Convert JSON config → YAML
        let src = self.openclaw_home.join("config.json");
        if src.exists() {
            let json: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(&src)?
            )?;
            let config = AppConfig::from_openclaw_json(&json)?;
            let yaml = serde_yaml::to_string(&config)?;
            std::fs::write(self.edgecrab_home.join("config.yaml"), yaml)?;
        }
        Ok(MigrationItem::success("config"))
    }

    pub fn migrate_sessions(&self) -> Result<MigrationItem> {
        // Convert JSON session files → SQLite
        let sessions_dir = self.openclaw_home.join("sessions");
        if sessions_dir.is_dir() {
            let db = SessionDb::open(&self.edgecrab_home.join("state.db"))?;
            for entry in std::fs::read_dir(&sessions_dir)? {
                let path = entry?.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    let session: serde_json::Value = serde_json::from_str(
                        &std::fs::read_to_string(&path)?
                    )?;
                    db.import_openclaw_session(&session)?;
                }
            }
        }
        Ok(MigrationItem::success("sessions"))
    }
}
```

## 4. Backward Compatibility

### 4.1 CLI Aliases

```bash
# Support hermes command as alias during transition
alias hermes='edgecrab'
alias openclaw='edgecrab'

# Or via symlink
ln -s $(which edgecrab) /usr/local/bin/hermes
```

### 4.2 Environment Variable Compat

```rust
/// Check for legacy env vars and map to new ones
pub fn resolve_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok()
        .or_else(|| std::env::var("OPENCLAW_API_KEY").ok())
        .or_else(|| std::env::var("HERMES_API_KEY").ok())
}
```

### 4.3 Config Dir Auto-Detection

```rust
pub fn edgecrab_home() -> PathBuf {
    // 1. Explicit env var
    if let Ok(dir) = std::env::var("EDGECRAB_HOME") {
        return PathBuf::from(dir);
    }
    // 2. Default location
    let default = dirs::home_dir().unwrap().join(".edgecrab");
    if default.exists() { return default; }
    // 3. Fallback to hermes dir (migration not yet done)
    let hermes = dirs::home_dir().unwrap().join(".hermes");
    if hermes.exists() {
        eprintln!("⚠️  Using ~/.hermes/ — run `edgecrab migrate --from hermes` to migrate");
        return hermes;
    }
    // 4. Create default
    std::fs::create_dir_all(&default).ok();
    default
}
```

## 5. Migration Report

```rust
pub struct MigrationReport {
    pub items: Vec<MigrationItem>,
    pub warnings: Vec<String>,
}

pub struct MigrationItem {
    pub name: String,
    pub status: MigrationStatus,
    pub files_migrated: u32,
    pub details: String,
}

pub enum MigrationStatus {
    Success,
    Skipped(String),
    Failed(String),
}

impl MigrationReport {
    pub fn print_summary(&self) {
        println!("Migration Summary:");
        for item in &self.items {
            let icon = match &item.status {
                MigrationStatus::Success => "✅",
                MigrationStatus::Skipped(_) => "⏭️",
                MigrationStatus::Failed(_) => "❌",
            };
            println!("  {} {} — {}", icon, item.name, item.details);
        }
    }
}
```

## 6. Database Schema Versioning

hermes-agent uses `SCHEMA_VERSION = 6` with migration support:

```rust
/// Apply pending schema migrations
pub fn migrate_schema(conn: &Connection) -> Result<()> {
    let current = get_schema_version(conn)?;
    for version in (current + 1)..=SCHEMA_VERSION {
        match version {
            2 => conn.execute_batch("ALTER TABLE sessions ADD COLUMN cache_read_tokens INTEGER DEFAULT 0;")?,
            3 => conn.execute_batch("ALTER TABLE sessions ADD COLUMN cache_write_tokens INTEGER DEFAULT 0;")?,
            4 => conn.execute_batch("ALTER TABLE messages ADD COLUMN reasoning_details TEXT;")?,
            5 => conn.execute_batch("ALTER TABLE sessions ADD COLUMN billing_provider TEXT;")?,
            6 => conn.execute_batch("ALTER TABLE messages ADD COLUMN codex_reasoning_items TEXT;")?,
            _ => {}
        }
        set_schema_version(conn, version)?;
    }
    Ok(())
}
```

Note: hermes stores its database as `state.db` (not `sessions.db`). EdgeCrab uses the
same filename for direct compatibility.

## 7. `edgecrab doctor` — Post-Migration Health Check

```bash
edgecrab doctor
```

Validates the entire installation after migration:

```rust
pub struct DoctorChecks;

impl DoctorChecks {
    pub fn run_all() -> Vec<DoctorResult> {
        vec![
            Self::check_config_valid(),
            Self::check_db_schema(),
            Self::check_api_keys(),
            Self::check_skills_integrity(),
            Self::check_memory_files(),
            Self::check_mcp_servers(),
            Self::check_binary_version(),
        ]
    }
}
```

## 8. Migration Edge Cases

| Scenario | Handling |
|----------|----------|
| Hermes JSONL sessions (pre-SQLite) | Convert to SQLite rows |
| Duplicate memory entries | Deduplicate on import |
| Custom model configs | Map to `custom_models.yaml` |
| Skills with Python scripts | Warn — scripts need Rust port or WASM |
| MCP server configs | Direct copy (JSON-compatible) |
| Hermes `.env` with legacy key names | Map `HERMES_API_KEY` → `OPENROUTER_API_KEY` |
| Concurrent hermes/edgecrab usage | Both share `state.db` via WAL mode |
