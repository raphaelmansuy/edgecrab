use edgecrab_migrate::hermes::HermesMigrator;
use edgecrab_migrate::report::MigrationStatus;
use rusqlite::{Connection, params};

fn init_hermes_state_db(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).expect("open source db");
    conn.execute_batch(include_str!("../../edgecrab-state/src/schema.sql"))
        .expect("init source schema");
    conn.execute("INSERT INTO schema_version (version) VALUES (6)", [])
        .expect("schema version");
    conn
}

fn setup_hermes_home(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).expect("create hermes home");
    std::fs::write(
        dir.join("config.yaml"),
        "model:\n  default: claude\n  base_url: https://openrouter.ai/api/v1\n",
    )
    .expect("write config");
    std::fs::write(dir.join(".env"), "OPENROUTER_API_KEY=sk-test\n").expect("write env");

    let memories = dir.join("memories");
    std::fs::create_dir_all(&memories).expect("create memories");
    std::fs::write(memories.join("MEMORY.md"), "remember migration lineage").expect("write memory");

    let skill = dir.join("skills").join("ops").join("state-audit");
    std::fs::create_dir_all(&skill).expect("create skill");
    std::fs::write(skill.join("SKILL.md"), "# State Audit").expect("write skill");
}

#[test]
fn migrate_all_preserves_files_and_state_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let hermes = tmp.path().join("hermes");
    let edgecrab = tmp.path().join("edgecrab");
    setup_hermes_home(&hermes);

    let source = init_hermes_state_db(&hermes.join("state.db"));
    source
        .execute(
            "INSERT INTO sessions (id, source, started_at, title)
             VALUES (?1, ?2, ?3, ?4)",
            params!["parent", "cli", 1_700_000_100.0_f64, "Parent"],
        )
        .expect("insert parent");
    source
        .execute(
            "INSERT INTO sessions (id, source, parent_session_id, started_at, title)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["child", "cli", "parent", 1_700_000_000.0_f64, "Child"],
        )
        .expect("insert child");
    source
        .execute(
            "INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_name, timestamp, reasoning
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "child",
                "tool",
                "audit complete",
                "call_1",
                "session_search",
                1_700_000_001.0_f64,
                "verified"
            ],
        )
        .expect("insert message");

    let migrator = HermesMigrator::new(hermes.clone(), edgecrab.clone());
    let report = migrator.migrate_all().expect("migrate all");

    assert_eq!(report.failed_count(), 0);
    assert_eq!(report.success_count(), 5);

    let config = std::fs::read_to_string(edgecrab.join("config.yaml")).expect("read config");
    assert!(config.contains("default: claude"));
    assert!(!config.contains("base_url"));
    assert!(edgecrab.join(".env").exists());
    assert!(edgecrab.join("memories").join("MEMORY.md").exists());
    assert!(edgecrab.join("skills/ops/state-audit/SKILL.md").exists());

    let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("open target");
    let child = db
        .get_session("child")
        .expect("get child")
        .expect("child exists");
    assert_eq!(child.parent_session_id.as_deref(), Some("parent"));

    let messages = db.get_messages("child").expect("child messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].name.as_deref(), Some("session_search"));
    assert_eq!(messages[0].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(messages[0].reasoning.as_deref(), Some("verified"));
}

#[test]
fn migrate_state_is_duplicate_safe_on_re_run_end_to_end() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let hermes = tmp.path().join("hermes");
    let edgecrab = tmp.path().join("edgecrab");
    setup_hermes_home(&hermes);

    let source = init_hermes_state_db(&hermes.join("state.db"));
    source
        .execute(
            "INSERT INTO sessions (id, source, started_at, title)
             VALUES (?1, ?2, ?3, ?4)",
            params!["shared", "cli", 1_700_000_000.0_f64, "Shared"],
        )
        .expect("insert shared");

    let migrator = HermesMigrator::new(hermes.clone(), edgecrab.clone());
    let first = migrator.migrate_state();
    assert_eq!(first.status, MigrationStatus::Success);

    let second = migrator.migrate_state();
    assert_eq!(second.status, MigrationStatus::Skipped);
    assert!(second.detail.contains("already exist"));

    let db = edgecrab_state::SessionDb::open(&edgecrab.join("state.db")).expect("open target");
    let session = db
        .get_session("shared")
        .expect("get shared")
        .expect("shared exists");
    assert_eq!(session.title.as_deref(), Some("Shared"));
}
