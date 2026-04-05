use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("edgecrab-cli manifest should live under <repo>/edgecrab/crates/edgecrab-cli")
        .to_path_buf()
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

fn assert_contains(haystack: &str, needle: &str, label: &str) {
    assert!(
        haystack.contains(needle),
        "{label} should contain `{needle}`"
    );
}

fn collect_skill_count(root: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        for entry in fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()))
        {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            if path.is_dir() {
                walk(&path, count);
                continue;
            }

            let is_skill = path.file_name().and_then(|n| n.to_str()) == Some("SKILL.md");
            if !is_skill {
                continue;
            }

            let path_str = path.to_string_lossy();
            if path_str.contains("/skills/") || path_str.contains("/optional-skills/") {
                *count += 1;
            }
        }
    }

    let mut count = 0;
    walk(root, &mut count);
    count
}

fn collect_file_stems(dir: &Path, extension: &str, skip: &[&str]) -> BTreeSet<String> {
    let skip: BTreeSet<&str> = skip.iter().copied().collect();
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some(extension))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .filter(|stem| !skip.contains(stem.as_str()))
        .collect()
}

fn expected_edgecrab_gateway_adapters() -> BTreeSet<String> {
    [
        "api_server",
        "telegram",
        "discord",
        "slack",
        "feishu",
        "wecom",
        "signal",
        "whatsapp",
        "webhook",
        "email",
        "sms",
        "matrix",
        "mattermost",
        "dingtalk",
        "homeassistant",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn expected_hermes_gateway_adapters() -> BTreeSet<String> {
    [
        "api_server",
        "telegram",
        "discord",
        "slack",
        "signal",
        "whatsapp",
        "webhook",
        "email",
        "sms",
        "matrix",
        "mattermost",
        "dingtalk",
        "homeassistant",
        "feishu",
        "wecom",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[test]
fn overview_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_tools_mod = read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/mod.rs"));
    let edgecrab_backends =
        read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/backends/mod.rs"));
    let edgecrab_browser = read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/browser.rs"));
    let hermes_model_tools = read(&root.join("hermes-agent/model_tools.py"));
    let env_doc = read(&root.join("edgecrab/docs/008_environments/001_environments.md"));
    let overview = read(&root.join("edgecrab/docs/gaps/00_overview/001_index.md"));

    let edgecrab_browser_tools = edgecrab_browser.matches("pub struct Browser").count();
    let hermes_browser_tools = read(&root.join("hermes-agent/tools/browser_tool.py"))
        .lines()
        .filter(|line| line.starts_with("def browser_"))
        .count();

    assert_eq!(edgecrab_browser_tools, 14);
    assert_eq!(hermes_browser_tools, 11);
    assert_contains(&edgecrab_backends, "pub mod docker;", "edgecrab backends");
    assert_contains(&edgecrab_backends, "pub mod local;", "edgecrab backends");
    assert_contains(&edgecrab_backends, "pub mod modal;", "edgecrab backends");
    assert_contains(&edgecrab_backends, "pub mod ssh;", "edgecrab backends");
    assert_contains(
        &hermes_model_tools,
        "\"tools.image_generation_tool\"",
        "hermes model tools",
    );
    assert_contains(
        &hermes_model_tools,
        "\"tools.rl_training_tool\"",
        "hermes model tools",
    );
    assert_contains(
        &edgecrab_tools_mod,
        "pub mod browser;",
        "edgecrab tools mod",
    );
    assert_contains(
        &env_doc,
        "EdgeCrab supports 6 backends",
        "edgecrab environments doc drift marker",
    );
    assert_contains(
        &overview,
        "| Browser tool verbs | 14 | 11 |",
        "overview gap doc",
    );
    assert_contains(
        &overview,
        "| Execution backends | 6 active backends | 6 active backends |",
        "overview gap doc",
    );
    assert_contains(
        &overview,
        "direct plus managed Modal transport variants",
        "overview gap doc",
    );
    assert_contains(
        &overview,
        "| Test files under audited test trees | about 209 | about 694 |",
        "overview gap doc",
    );
}

#[test]
fn cli_tui_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_app = read(&root.join("edgecrab/crates/edgecrab-cli/src/app.rs"));
    let hermes_voice = read(&root.join("hermes-agent/tools/voice_mode.py"));
    let doc = read(&root.join("edgecrab/docs/gaps/01_cli_tui/001_cli_tui.md"));

    for needle in [
        "KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES",
        "KeyboardEnhancementFlags::REPORT_EVENT_TYPES",
        "KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES",
        "KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS",
        "KEYBOARD_PROTOCOL_WARMUP",
        "WaitingForApproval",
        "SecretCapture",
        "session_browser",
        "skill_selector",
        "model_selector",
        "ghost_hint",
        "mouse_capture_enabled",
        "ComposeInsert",
        "ComposeNormal",
    ] {
        assert_contains(&edgecrab_app, needle, "edgecrab app");
    }

    assert_contains(
        &hermes_voice,
        "Push-to-talk audio recording and playback for the CLI.",
        "hermes voice mode",
    );
    assert_contains(
        &doc,
        "EdgeCrab exceeds Hermes on terminal control, keyboard normalization, and full-screen overlay composition.",
        "cli/tui gap doc",
    );
    assert_contains(
        &doc,
        "Hermes still leads on one real input primitive: live microphone capture.",
        "cli/tui gap doc",
    );
}

#[test]
fn core_tools_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_tools_mod = read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/mod.rs"));
    let edgecrab_browser = read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/browser.rs"));
    let edgecrab_toolsets = read(&root.join("edgecrab/crates/edgecrab-tools/src/toolsets.rs"));
    let edgecrab_acp_permissions =
        read(&root.join("edgecrab/crates/edgecrab-acp/src/permission.rs"));
    let hermes_browser = read(&root.join("hermes-agent/tools/browser_tool.py"));
    let hermes_model_tools = read(&root.join("hermes-agent/model_tools.py"));
    let doc = read(&root.join("edgecrab/docs/gaps/02_core_tools/001_core_tools.md"));

    let edgecrab_browser_tools = edgecrab_browser.matches("pub struct Browser").count();
    let hermes_browser_tools = hermes_browser
        .lines()
        .filter(|line| line.starts_with("def browser_"))
        .count();
    assert_eq!(edgecrab_browser_tools, 14);
    assert_eq!(hermes_browser_tools, 11);

    for needle in [
        "pub mod execute_code;",
        "pub mod delegate_task;",
        "pub mod session_search;",
        "pub mod browser;",
        "pub mod vision;",
        "pub mod transcribe;",
    ] {
        assert_contains(&edgecrab_tools_mod, needle, "edgecrab tools mod");
    }
    for needle in [
        "\"tools.browser_tool\"",
        "\"tools.image_generation_tool\"",
        "\"tools.rl_training_tool\"",
    ] {
        assert_contains(&hermes_model_tools, needle, "hermes model tools");
    }
    for needle in [
        "\"browser_wait_for\"",
        "\"browser_select\"",
        "\"browser_hover\"",
    ] {
        assert_contains(&edgecrab_toolsets, needle, "edgecrab toolsets");
        assert_contains(
            &edgecrab_acp_permissions,
            needle,
            "edgecrab acp permissions",
        );
    }

    assert_contains(
        &doc,
        "- EdgeCrab ships 14 browser verbs",
        "core tools gap doc",
    );
    assert_contains(
        &doc,
        "- Hermes ships 11 browser verbs",
        "core tools gap doc",
    );
    assert_contains(
        &doc,
        "Hermes exposes `tools/image_generation_tool.py` in the runtime surface.",
        "core tools gap doc",
    );
    assert_contains(
        &doc,
        "Those extra browser verbs are now exposed consistently across EdgeCrab's core and ACP capability surfaces.",
        "core tools gap doc",
    );
}

#[test]
fn execution_backends_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_backends =
        read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/backends/mod.rs"));
    let edgecrab_modal =
        read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/backends/modal.rs"));
    let backend_tests =
        read(&root.join("edgecrab/crates/edgecrab-tools/tests/terminal_backends.rs"));
    let doc =
        read(&root.join("edgecrab/docs/gaps/03_execution_backends/001_execution_backends.md"));
    let env_doc = read(&root.join("edgecrab/docs/008_environments/001_environments.md"));

    for needle in [
        "pub mod docker;",
        "pub mod daytona;",
        "pub mod local;",
        "pub mod modal;",
        "pub mod singularity;",
        "pub mod ssh;",
    ] {
        assert_contains(&edgecrab_backends, needle, "edgecrab backends");
    }
    for path in [
        "hermes-agent/tools/environments/daytona.py",
        "hermes-agent/tools/environments/singularity.py",
    ] {
        assert!(root.join(path).exists(), "expected backend file {path}");
    }
    assert_contains(
        &env_doc,
        "EdgeCrab supports 6 backends",
        "edgecrab environments doc drift marker",
    );
    assert_contains(
        &doc,
        "Hermes ships six core execution worlds",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab ships the same six core worlds",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab now matches Hermes on the six core execution worlds, on direct plus managed Modal transport variants, and on background-process routing.",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "typed Modal transport selection via `auto`, `direct`, and `managed` modes",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "full terminal-tool dispatch into managed Modal via a fake gateway API",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "direct Modal path now also restores task-scoped filesystem snapshots",
        "execution backends gap doc",
    );
    assert_contains(
        &doc,
        "Hermes no longer holds a code-surface lead in execution backends.",
        "execution backends gap doc",
    );
    assert_contains(
        &edgecrab_modal,
        "ModalTransportMode",
        "edgecrab modal backend",
    );
    assert_contains(
        &edgecrab_modal,
        "snapshot_filesystem",
        "edgecrab modal backend",
    );
    assert_contains(
        &backend_tests,
        "e2e_modal_backend_respects_working_directory_via_fake_http_api",
        "terminal backend tests",
    );
    assert_contains(
        &backend_tests,
        "e2e_modal_direct_persists_filesystem_snapshots_across_backend_rebuilds",
        "terminal backend tests",
    );
    assert_contains(
        &backend_tests,
        "e2e_modal_direct_syncs_auth_skills_and_cache_files",
        "terminal backend tests",
    );
    assert_contains(
        &backend_tests,
        "e2e_managed_modal_remote_run_process_waits_and_collects_output",
        "terminal backend tests",
    );
    assert_contains(
        &backend_tests,
        "e2e_daytona_remote_run_process_waits_and_collects_output",
        "terminal backend tests",
    );
}

#[test]
fn gateway_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_gateway_dir = root.join("edgecrab/crates/edgecrab-gateway/src");
    let hermes_gateway_dir = root.join("hermes-agent/gateway/platforms");
    let edgecrab_setup = read(&root.join("edgecrab/crates/edgecrab-cli/src/gateway_setup.rs"));
    let edgecrab_catalog = read(&root.join("edgecrab/crates/edgecrab-cli/src/gateway_catalog.rs"));
    let doc = read(&root.join("edgecrab/docs/gaps/04_gateway_channels/001_gateway_channels.md"));

    let expected_edgecrab = expected_edgecrab_gateway_adapters();
    let expected_hermes = expected_hermes_gateway_adapters();
    let edgecrab = collect_file_stems(
        &edgecrab_gateway_dir,
        "rs",
        &[
            "attachment_cache",
            "channel_directory",
            "config",
            "delivery",
            "event_processor",
            "hooks",
            "interactions",
            "lib",
            "mirror",
            "pairing",
            "platform",
            "run",
            "sender",
            "session",
            "stream_consumer",
        ],
    );
    let hermes = collect_file_stems(
        &hermes_gateway_dir,
        "py",
        &["__init__", "base", "telegram_network"],
    );

    assert_eq!(
        edgecrab, expected_edgecrab,
        "edgecrab adapter inventory changed"
    );
    assert_eq!(hermes, expected_hermes, "hermes adapter inventory changed");

    for platform in [
        "telegram",
        "discord",
        "slack",
        "feishu",
        "wecom",
        "signal",
        "whatsapp",
        "webhook",
        "email",
        "sms",
        "matrix",
        "mattermost",
        "dingtalk",
        "homeassistant",
        "api_server",
    ] {
        assert_contains(
            &edgecrab_catalog,
            &format!("id: \"{platform}\""),
            "gateway catalog",
        );
    }
    assert_contains(
        &edgecrab_setup,
        "configure_generic_env_platform",
        "gateway setup",
    );
    assert_contains(&edgecrab_setup, "configure_webhook", "gateway setup");
    for helper in ["pub mod platform;", "pub mod delivery;", "pub mod sender;"] {
        assert_contains(
            &read(&root.join("edgecrab/crates/edgecrab-gateway/src/lib.rs")),
            helper,
            "edgecrab gateway lib",
        );
    }

    assert_contains(
        &doc,
        "EdgeCrab exceeds Hermes on gateway onboarding, operator diagnostics, runtime cohesion, and operator-path validation depth.",
        "gateway gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab ships 15 gateway adapters in source.",
        "gateway gap doc",
    );
    assert_contains(
        &doc,
        "Hermes ships 15 gateway adapters in source.",
        "gateway gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab now ships one catalog-driven operator surface across all 15 EdgeCrab gateway adapters.",
        "gateway gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab now matches Hermes on raw adapter breadth and exceeds Hermes on operator completeness and operator-path validation across that same shipped surface.",
        "gateway gap doc",
    );
}

#[test]
fn memory_skills_state_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_root = root.join("edgecrab");
    let hermes_root = root.join("hermes-agent");
    let edgecrab_migrate = read(&root.join("edgecrab/crates/edgecrab-migrate/src/hermes.rs"));
    let edgecrab_state = read(&root.join("edgecrab/crates/edgecrab-state/src/session_db.rs"));
    let edgecrab_honcho = read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/honcho.rs"));
    let edgecrab_skills_hub =
        read(&root.join("edgecrab/crates/edgecrab-tools/src/tools/skills_hub.rs"));
    let doc =
        read(&root.join("edgecrab/docs/gaps/05_memory_skills_state/001_memory_skills_state.md"));

    assert_eq!(collect_skill_count(&edgecrab_root), 111);
    assert_eq!(collect_skill_count(&hermes_root), 116);

    for needle in [
        "migrate_all",
        "migrate_state",
        "migrate_memory",
        "migrate_skills",
        "migrate_env",
    ] {
        assert_contains(&edgecrab_migrate, needle, "edgecrab hermes migrator");
    }
    for needle in ["FTS5", "escape_fts5_query", "fn search(", "messages_fts"] {
        assert_contains(&edgecrab_state, needle, "edgecrab session db");
    }
    for needle in [
        "honcho_conclude",
        "honcho_search",
        "honcho_list",
        "honcho_remove",
        "honcho_profile",
        "honcho_context",
    ] {
        assert_contains(&edgecrab_honcho, needle, "edgecrab honcho tools");
    }
    for needle in [
        "read_lock",
        "read_taps",
        "search_optional_skills",
        "install_skill",
    ] {
        assert_contains(&edgecrab_skills_hub, needle, "edgecrab skills hub");
    }

    assert_contains(
        &doc,
        "EdgeCrab exceeds Hermes on migration direction, typed state boundaries, and local auditability.",
        "memory/skills/state gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab: 111 skill definitions",
        "memory/skills/state gap doc",
    );
    assert_contains(
        &doc,
        "Hermes: 116 skill definitions",
        "memory/skills/state gap doc",
    );
}

#[test]
fn security_distribution_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_security = read(&root.join("edgecrab/crates/edgecrab-security/src/lib.rs"));
    let edgecrab_readme = read(&root.join("edgecrab/README.md"));
    let edgecrab_whatsapp = read(&root.join("edgecrab/crates/edgecrab-cli/src/whatsapp_cmd.rs"));
    let hermes_approval = read(&root.join("hermes-agent/tools/approval.py"));
    let hermes_tirith = read(&root.join("hermes-agent/tools/tirith_security.py"));
    let doc = read(
        &root.join("edgecrab/docs/gaps/06_security_distribution/001_security_distribution.md"),
    );

    for needle in [
        "pub mod approval;",
        "pub mod command_scan;",
        "pub mod injection;",
        "pub mod normalize;",
        "pub mod path_jail;",
        "pub mod redact;",
        "pub mod url_safety;",
    ] {
        assert_contains(&edgecrab_security, needle, "edgecrab security crate");
    }
    for needle in [
        "npm install -g edgecrab-cli",
        "pip install edgecrab-cli",
        "cargo install edgecrab-cli",
        "single static binary",
    ] {
        assert_contains(&edgecrab_readme, needle, "edgecrab readme");
    }
    assert_contains(
        &edgecrab_whatsapp,
        "Node.js and npm are required for WhatsApp support",
        "edgecrab whatsapp support",
    );
    assert_contains(
        &hermes_approval,
        "Dangerous command approval",
        "hermes approval module",
    );
    assert_contains(
        &hermes_tirith,
        "Tirith pre-exec security scanning wrapper.",
        "hermes tirith security",
    );
    assert_contains(
        &doc,
        "the core CLI is binary-first, while a few optional integrations still pull external runtimes.",
        "security/distribution gap doc",
    );
    assert_contains(
        &doc,
        "The WhatsApp bridge still requires Node.js and npm",
        "security/distribution gap doc",
    );
}

#[test]
fn research_training_gap_doc_is_backed_by_source() {
    let root = repo_root();
    let edgecrab_trajectory = read(&root.join("edgecrab/crates/edgecrab-types/src/trajectory.rs"));
    let edgecrab_conversation =
        read(&root.join("edgecrab/crates/edgecrab-core/src/conversation.rs"));
    let doc = read(&root.join("edgecrab/docs/gaps/07_research_training/001_research_training.md"));

    for path in [
        "hermes-agent/batch_runner.py",
        "hermes-agent/mini_swe_runner.py",
        "hermes-agent/trajectory_compressor.py",
        "hermes-agent/tools/rl_training_tool.py",
        "hermes-agent/tinker-atropos",
    ] {
        assert!(
            root.join(path).exists(),
            "expected Hermes research asset to exist: {path}"
        );
    }

    for needle in [
        "Trajectory types for session recording and RL training.",
        "pub struct Trajectory",
        "pub struct TrajectoryMetadata",
        "pub fn save_trajectory",
    ] {
        assert_contains(&edgecrab_trajectory, needle, "edgecrab trajectory types");
    }
    for needle in [
        "if config.save_trajectories {",
        "trajectory_samples.jsonl",
        "failed_trajectories.jsonl",
        "build_trajectory(",
        "save_trajectory(&trajectory_path, &trajectory)",
    ] {
        assert_contains(&edgecrab_conversation, needle, "edgecrab conversation");
    }

    assert_contains(
        &doc,
        "Hermes still leads overall on shipped research and training tooling.",
        "research/training gap doc",
    );
    assert_contains(
        &doc,
        "EdgeCrab exceeds Hermes on one narrower layer: typed trajectory substrate and in-runtime capture wiring.",
        "research/training gap doc",
    );
}
