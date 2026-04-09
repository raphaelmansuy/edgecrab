# Implementation Plan — EdgeCrab Plugin System

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Owner:** edgecrab-plugins crate  
**Cross-refs:** [000_overview], [001_adr_architecture], [002_adr_transport],
               [003_manifest], [004_plugin_types], [005_lifecycle],
               [006_security], [007_registry], [008_host_api],
               [009_discovery_hub], [010_cli_commands], [011_config_schema],
               [012_crate_structure], [013_plugins_skills_relation],
               [014_edge_cases], [015_hermes_compatibility], [017_plugin_tui]

**EdgeCrab source files (CODE IS LAW):**
- `crates/edgecrab-cli/src/plugins.rs` — `PluginManager`, `Plugin`, `PluginSource`, `PluginHook`, `PluginManifest`
- `crates/edgecrab-cli/src/plugins_cmd.rs` — `PluginAction`, existing `list_plugins()` / `install_plugin()`
- `crates/edgecrab-cli/src/app.rs` — TUI `App` struct, key-event dispatch, render loop, `tool_manager` pattern
- `crates/edgecrab-cli/src/commands.rs` — `CommandRegistry`, `CommandResult` enum
- `crates/edgecrab-cli/src/fuzzy_selector.rs` — `FuzzySelector<T>`, `FuzzyItem` trait (no changes needed)
- `crates/edgecrab-core/src/config.rs` — `AppConfig`, config persistence (`AppConfig::save()`)
- `crates/edgecrab-tools/src/registry.rs` — `ToolRegistry`, `ToolSchema`, `ToolHandler`
- `crates/edgecrab-tools/src/toolsets.rs` — `CORE_TOOLS`, toolset definitions

**hermes-agent source files (CODE IS LAW — reference implementation):**
- `hermes-agent/hermes_cli/skills_config.py` — skill enable/disable flow, `_is_skill_disabled()`, `save_disabled_skills()` (reference for Phase 1 SkillPlugin + Phase 1.5 TUI)
- `hermes-agent/hermes_cli/curses_ui.py` — `curses_checklist()`, `_numbered_fallback()` (reference for Phase 1.5 overlay)
- `hermes-agent/hermes_cli/tools_config.py` — toolset toggle, token estimation (reference for Phase 2)
- `hermes-agent/hermes_cli/skill_commands.py` — skill slash command dispatch (reference for Phase 3)
- `hermes-agent/tools/mcp_tool.py` — MCP client JSON-RPC (reference for Phase 2 ToolServerPlugin)
- `hermes-agent/tools/delegate_tool.py` — subagent delegation (reference for Phase 4)

---

## 1. Overview

This document breaks the plugin system into five deliverable phases. Each phase is
independently releasable and builds on the previous one. Phases 1 through 3 implement
the three plugin kinds; Phases 4 and 5 add the hub, security scanner, and tests.

```
Phase 0 — Scaffold      (0.5 days)   Crate + trait stubs + CI wiring
Phase 1 — SkillPlugin  (3 days)     Hermes-compatible SKILL.md system
Phase 1.5 — TUI Toggle (1.5 days)   Plugin activation/deactivation overlay (see [017_plugin_tui])
Phase 2 — ToolServer   (3 days)     JSON-RPC subprocess plugin
Phase 3 — Script       (2 days)     Rhai inline script plugin
Phase 4 — Hub + Guard  (3 days)     Discovery, security scanner, CLI
Phase 5 — Tests + CI   (2 days)     Property tests, fuzzing, integration
                        ---------
Total estimated:         ~15 days
```

---

## 2. Phase 0 — Scaffold

**Goal:** Create the `edgecrab-plugins` crate with all stubs in place, wired into CI.

### 2.1 Module Layout

```
crates/edgecrab-plugins/
├── Cargo.toml
└── src/
    ├── lib.rs              pub use re-exports + feature flags
    ├── error.rs            PluginError enum (thiserror)
    ├── types.rs            TrustLevel, PluginKind, PluginStatus, SkillReadinessStatus
    ├── manifest.rs         PluginManifest + parse_plugin_manifest()
    ├── registry.rs         PluginRegistry trait + Arc<dyn Plugin>
    ├── host_api.rs         HostApi trait (file access, model call, etc.)
    ├── skill/
    │   ├── mod.rs
    │   ├── manifest.rs     SkillManifest + parse_skill_manifest()
    │   ├── loader.rs       scan_skills_dir()
    │   ├── platform.rs     skill_matches_platform()
    │   ├── readiness.rs    SkillReadinessStatus resolution
    │   ├── inject.rs       translate_hermes_paths() + build_prompt_fragment()
    │   └── sync.rs         bundled_skills_sync()
    ├── tool_server/
    │   ├── mod.rs
    │   ├── process.rs      spawn + stdio JSON-RPC 2.0 transport
    │   ├── client.rs       ToolServerClient (tool_call / tool_list)
    │   └── config.rs       ToolServerConfig
    ├── script/
    │   ├── mod.rs
    │   ├── engine.rs       Rhai engine setup + safe API bindings
    │   └── runner.rs       ScriptPlugin::run_hook()
    ├── hub/
    │   ├── mod.rs
    │   ├── auth.rs         GitHubAuth (4-priority token resolution)
    │   ├── sources.rs      SkillSource trait + GitHub + ClaWhub + Marketplace impls
    │   ├── index.rs        tap management + index cache (TTL 3600s)
    │   ├── download.rs     _download_directory (Git Trees API + Contents fallback)
    │   ├── quarantine.rs   quarantine dir lifecycle
    │   └── lock.rs         lock.json read/write
    └── guard/
        ├── mod.rs
        ├── patterns.rs     ALL Hermes THREAT_PATTERNS + edgecrab_env_access
        ├── scanner.rs      scan_skill_bundle() → ScanResult
        ├── verdict.rs      should_allow_install(trust, scan) → VerdictResult
        └── limits.rs       structural size/count limits
```

### 2.2 Cargo.toml Features

```toml
[features]
default = ["skill", "tool-server"]
skill         = []     # SkillPlugin (Hermes compat)
tool-server   = []     # ToolServerPlugin (subprocess JSON-RPC)
script        = ["rhai"]  # ScriptPlugin
hub           = ["reqwest", "base64", "jsonwebtoken"]
guard         = ["fancy-regex"]
full          = ["skill", "tool-server", "script", "hub", "guard"]
```

### 2.3 CI Wire-up

Add `edgecrab-plugins` to workspace `Cargo.toml`. Clippy and test jobs in
`.github/workflows/ci.yml` must cover it.

---

## 3. Phase 1 — SkillPlugin

**Goal:** Full Hermes-compatible SKILL.md loading, platform filtering, env-var wizard,
and system-prompt injection.

### 3.1 Deliverables

| Module | Deliverable | Source ref |
|---|---|---|
| `skill/manifest.rs` | `parse_skill_manifest()` with all fields, aliases, `_ENV_VAR_NAME_RE` | D-6, D-8, §3 |
| `skill/platform.rs` | `skill_matches_platform()` with `PLATFORM_MAP` | [015] §3.2 |
| `skill/readiness.rs` | `SkillReadinessStatus` enum + resolution logic + `REMOTE_ENV_BACKENDS` | D-9, D-10 |
| `skill/loader.rs` | `scan_skills_dir()` with depth ≤ 2, excluded dirs, external dirs | [015] §4 |
| `skill/inject.rs` | `translate_hermes_paths()` + `build_prompt_fragment()` | [015] §8.1 |
| `skill/sync.rs` | `bundled_skills_sync()` + manifest read/write (`.bundled_manifest`) | [013] §13 |

### 3.2 Key Algorithms

#### SKILL.md Parse

```rust
// 1. Split on --- delimiter
// 2. Parse YAML frontmatter (fallback: line-by-line key:value split)
// 3. Validate: name regex, length limits, env var name regex
// 4. Require non-empty body after strip (D-7)
// 5. Normalise prerequisites.env_vars → required_environment_variables
// 6. Accept env_var alias for name in required_environment_variables (D-6)
// 7. Accept url alias for provider_url in collect_secrets (D-5)
// 8. Apply setup.help fallback for entries with no per-entry help (D-11)
```

#### Platform Matching

```rust
const PLATFORM_MAP: &[(&str, &str)] = &[
    ("macos",   "darwin"),
    ("linux",   "linux"),
    ("windows", "win32"),
];

fn skill_matches_platform(platforms: &[String]) -> bool {
    if platforms.is_empty() { return true; }
    let os = std::env::consts::OS; // "macos", "linux", "windows"
    let hermes_os = PLATFORM_MAP.iter()
        .find(|(k, _)| *k == os)
        .map(|(_, v)| *v)
        .unwrap_or(os);
    platforms.iter().any(|p| {
        let normalized = PLATFORM_MAP.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(p))
            .map(|(_, v)| *v)
            .unwrap_or(p.as_str());
        hermes_os.starts_with(normalized)
    })
}
```

#### Bundled Sync (`.bundled_manifest`)

```
Format: one line per skill = "skill_name:md5hex"
4 cases on sync:
  NEW (in bundled, absent from user dir)       → copy + record hash
  UNCHANGED (hash matches manifest)            → overwrite + update hash
  CUSTOMIZED (hash differs from manifest)      → SKIP (preserve user edits)
  DELETED_BY_USER (in manifest, absent on disk)→ SKIP (respect deletion)
  REMOVED_FROM_BUNDLED (in manifest, not bundled)→ remove from manifest
```

### 3.3 Tests Required

- [ ] Load every `.md` in `hermes-agent/skills/` fixture dir without error → COMPAT-1
- [ ] Skill with only frontmatter → `Err(EmptyBody)` → COMPAT-0
- [ ] Platform matching: all 3+1 PLATFORM_MAP entries
- [ ] `env_var` alias parsed in required_environment_variables
- [ ] `url` alias parsed in collect_secrets
- [ ] `setup.help` fallback applied when entry has no per-entry help
- [ ] `SkillReadinessStatus::Unsupported` when `HERMES_ENVIRONMENT=docker` + missing vars
- [ ] `translate_hermes_paths()` replaces `~/.hermes/` with `~/.edgecrab/`
- [ ] Bundled sync: 5-case matrix verified
- [ ] `scan_skills_dir()`: nested layout detected at depth 2, not depth 3

---

## 4. Phase 1.5 — Plugin Toggle TUI

**Duration:** 1.5 days  
**Spec:** [017_plugin_tui] — read that document fully before implementing.

> **First principles:** Users MUST be able to enable/disable plugins without restarting
> the agent and without editing YAML by hand. The toggle overlay follows the same ratatui
> patterns as the existing `tool_manager` — no new widget infrastructure required (DRY).

### Deliverables

| File | Change |
|---|---|
| `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW: `PluginToggleEntry`, `PluginCheckState`, `PluginScope`, token estimation |
| `crates/edgecrab-cli/src/app.rs` | ADD: `plugin_toggle` FuzzySelector field + `plugin_toggle_scope` + helper methods |
| `crates/edgecrab-cli/src/commands.rs` | ADD: `CommandResult::ShowPluginToggle { name, platform }` + handler |
| `crates/edgecrab-core/src/config.rs` | ADD: `AppConfig::is_plugin_enabled(name, platform)` read helper |

### Phase 1.5 — Step-by-Step

**Step 1** (2h): Create `crates/edgecrab-cli/src/plugin_toggle.rs`
- hermes reference: `hermes-agent/hermes_cli/curses_ui.py` `curses_checklist()`, `hermes-agent/hermes_cli/skills_config.py` `_is_skill_disabled()`
- Define `PluginCheckState` (On/Off, glyph `[x]`/`[ ]`, toggle())
- Define `PluginScope` (Global | Platform(String))
- Define `PluginToggleEntry` implementing `FuzzyItem` from `crates/edgecrab-cli/src/fuzzy_selector.rs` (primary=display_name, secondary=description, tag=kind)
- Implement `plugin_toggle_status_line(entries)` → `"Est. plugin context: ~Xk tokens"` (mirrors `status_fn` in `hermes-agent/hermes_cli/curses_ui.py`)
- Implement `estimate_plugin_tokens(entry)` per kind (skill=body/4, tool-server=schema-sum, script=0) (mirrors `hermes-agent/hermes_cli/tools_config.py` token counter)
- Implement `plugin_toggle_text_fallback(entries)` for non-TTY (mirrors `_numbered_fallback()` in `hermes-agent/hermes_cli/curses_ui.py`)

**Step 2** (2h): Add `App` state fields and builder to `crates/edgecrab-cli/src/app.rs`
- hermes reference: `hermes-agent/hermes_cli/skills_config.py` `_select_platform()` (scope cycling), `_is_skill_disabled()` (disable algorithm)
- Add `plugin_toggle: FuzzySelector<PluginToggleEntry>` to `App` struct (alongside `tool_manager` field, ~line 2959)
- Add `plugin_toggle_scope: PluginScope` to `App` struct
- Add `plugin_toggle_status_note: Option<String>` to `App` struct
- Implement `build_plugin_toggle_entries(scope) -> Vec<PluginToggleEntry>` reading config + `PluginManager::discover()` from `crates/edgecrab-cli/src/plugins.rs`
- Implement `open_plugin_toggle(scope)`: set scope, build entries, set active=true (mirrors `open_tool_manager()` ~line 10088 in `app.rs`)
- Implement `refresh_plugin_toggle_entries() -> bool`
- Implement `cycle_plugin_toggle_scope()` (Tab key handler)

**Step 3** (2h): Key event handling in `crates/edgecrab-cli/src/app.rs`
- In `App::handle_key_event()`: add `if self.plugin_toggle.active { … }` block adjacent to the `tool_manager` block (~line 7026 in `app.rs`)
- Map: ↑/k, ↓/j, PgUp, PgDn → `FuzzySelector` nav methods from `crates/edgecrab-cli/src/fuzzy_selector.rs`
- Map: SPACE → `toggle_plugin_selected()` (flip check_state)
- Map: ENTER → `confirm_plugin_toggle()` (persist + close)
- Map: Esc → discard snapshot, `plugin_toggle.active = false`
- Map: Tab → `cycle_plugin_toggle_scope()`
- Map: char/Backspace → `plugin_toggle.push_char()` / `pop_char()`

**Step 4** (2h): Render in `crates/edgecrab-cli/src/app.rs`
- Implement `render_plugin_toggle(frame, area)` (5-zone vertical layout, see [017_plugin_tui] §9)
- Register in render dispatch alongside `if self.tool_manager.active { self.render_tool_manager(frame, frame.area()); }` (~line 18254)
- Reuse `render_two_column_footer()` from `tool_manager` for help bar
- Reuse `theme.overlay_selected` / `theme.overlay_title` from `crates/edgecrab-cli/src/theme.rs` — no hardcoded colors

**Step 5** (2h): Persist + CommandResult wiring
- hermes reference: `hermes-agent/hermes_cli/skills_config.py` `save_disabled_skills()` (config write pattern)
- Implement `persist_plugin_toggle_to_config(scope, disabled_names)` in `crates/edgecrab-cli/src/app.rs` writing `plugins.disabled` / `plugins.platform_disabled.*` in `crates/edgecrab-core/src/config.rs` (see [011_config_schema] §3 for canonical field names)
- Add `CommandResult::ShowPluginToggle { name, platform }` to `crates/edgecrab-cli/src/commands.rs`
- Wire in `App::handle_command_result()` in `crates/edgecrab-cli/src/app.rs`: open overlay OR non-interactive toggle
- Add `AppConfig::is_plugin_enabled(name, platform)` read helper in `crates/edgecrab-core/src/config.rs` (mirrors `_is_skill_disabled()` in `hermes-agent/hermes_cli/skills_config.py`)

**Step 6** (2h): Credential wizard + tests
- Wire `CredentialPromptState` from `crates/edgecrab-cli/src/app.rs` for newly-enabled ToolServer plugins missing env vars (see [017_plugin_tui] §5)
- hermes reference: `hermes-agent/hermes_cli/tools_config.py` `_configure_toolset()` (credential prompting pattern)
- Write unit tests for: `effective_check_state`, `cycle_plugin_toggle_scope`, `plugin_toggle_status_line`, `persist_plugin_toggle_to_config`, `plugin_toggle_text_fallback`

### Acceptance Criteria

All 15 criteria in [017_plugin_tui] §12 must pass (TUI-01 through TUI-15).

### Release Milestone

Phase 1.5 ships in the same milestone as Phase 1 (SkillPlugin) — `v0.2.0`.
The toggle overlay without Phase 1 is technically complete but shows zero plugins,
which is useful for testing the overlay mechanics before skills are loaded.

---

## 5. Phase 2 — ToolServerPlugin

**Goal:** Spawn subprocess tools over stdin/stdout JSON-RPC 2.0; integrate with HostApi
`tool_call` dispatch.

### 4.1 Deliverables

| Module | Deliverable |
|---|---|
| `tool_server/process.rs` | Spawn command, manage stdio transport, detect crashes |
| `tool_server/client.rs` | `ToolServerClient::tool_list()`, `tool_call()` |
| `tool_server/config.rs` | `ToolServerConfig` parsed from `plugin.toml` |

### 4.2 JSON-RPC 2.0 Wire Protocol

```jsonc
// Initialize (sent first)
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol_version":"0.1"}}
// Tool list
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
// Tool call
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"my_tool","arguments":{...}}}
```

Transport: newline-delimited JSON on stdin/stdout, stderr → tracing at debug level.

### 4.3 Lifecycle

```
plugin enable → spawn process → send initialize → register tools in HostApi
per LLM request → route tool_call via HostApi to ToolServerClient
idle timeout (configurable, default 300s) → send shutdown → kill
plugin disable / agent exit → send shutdown → SIGTERM after 5s → SIGKILL
```

### 4.4 Tests Required

- [ ] Spawn a minimal echo-server (Rust child process) and call `tools/list`
- [ ] `tool_call` returns correct result
- [ ] Process crash detected: plugin moves to `Error` status
- [ ] Idle timeout: process killed after N seconds with no calls
- [ ] `plugin.toml` parsed with all fields (command, args, env, cwd)

---

## 5. Phase 3 — ScriptPlugin

**Goal:** Inline Rhai script plugins that extend agent behaviour via lifecycle hooks.

### 5.1 Deliverables

| Module | Deliverable |
|---|---|
| `script/engine.rs` | Rhai `Engine` with safe API bindings, no `unsafe` Rust |
| `script/runner.rs` | `ScriptPlugin::run_hook(hook_name, context)` |

### 5.2 Rhai Safety Constraints

```
Allowed Rhai built-ins: string, array, map, math, logic, print (→ tracing)
Blocked: file I/O, network, process spawning, eval()
Module resolution: disabled (no require / import from filesystem)
Max operations: 100_000 per hook call (Rhai fuel limit)
Max call stack depth: 64
```

### 5.3 Exposed Host API (Rhai bindings)

```rust
// All Rhai functions are synchronous wrappers around blocked async calls
fn log(level: &str, msg: &str)
fn get_env(key: &str) -> String      // reads from agent context, not std::env
fn emit_message(msg: &str)           // adds message to agent's pending queue
```

### 5.4 Tests Required

- [ ] Hook fires on correct lifecycle event
- [ ] Blocked operations (`eval`, file access) produce `PluginError::ScriptViolation`
- [ ] Fuel limit: script exceeding 100k ops is killed cleanly
- [ ] `emit_message()` appends to agent message queue

---

## 6. Phase 4 — Hub + Security Guard

**Goal:** Full hub implementation (discovery, download, quarantine, lock) and the
Hermes-compatible security scanner.

### 6.1 Hub Deliverables

| Module | Deliverable |
|---|---|
| `hub/auth.rs` | `GitHubAuth._resolve_token()`: 4-priority chain |
| `hub/sources.rs` | `SkillSource` trait + `GitHubSource` impl (Git Trees API + fallback) |
| `hub/index.rs` | `TapManager` + 4 DEFAULT_TAPS + TTL-based index cache |
| `hub/download.rs` | `download_directory()` (trees API with truncation fallback) |
| `hub/quarantine.rs` | Write to quarantine, move after scan passes |
| `hub/lock.rs` | `lock.json` read/write in Hermes-compatible format |

### 6.2 Guard Deliverables

| Module | Deliverable |
|---|---|
| `guard/patterns.rs` | ALL Hermes patterns + `edgecrab_env_access` (D-12) |
| `guard/scanner.rs` | `scan_skill_bundle()` → `ScanResult` |
| `guard/verdict.rs` | `should_allow_install(trust, scan)` → `VerdictResult` (Option<bool>) |
| `guard/limits.rs` | `MAX_FILE_COUNT=50`, `MAX_TOTAL_SIZE_KB=1024`, `MAX_SINGLE_FILE_KB=256` |

### 6.3 Install Policy (from audit)

```rust
// Must match Hermes INSTALL_POLICY exactly
fn should_allow_install(trust: TrustLevel, verdict: ScanVerdict) -> Option<bool> {
    match (trust, verdict) {
        // Official — always allow
        (Official, _) => Some(true),
        // Trusted — block dangerous only
        (Trusted, Dangerous) => Some(false),
        (Trusted, _) => Some(true),
        // Community — allow only safe
        (Community, Safe) => Some(true),
        (Community, _) => Some(false),
        // Agent-created — allow+warn on dangerous
        (AgentCreated, Dangerous) => None,   // None = ask / allow-with-warning
        (AgentCreated, _) => Some(true),
        // Unverified (local path with no metadata)
        (Unverified, Safe) => None,          // None = ask
        (Unverified, _) => Some(false),
    }
}
```

Note: `None` = "ask user in interactive mode / allow-with-warning in non-interactive".

### 6.4 DEFAULT_TAPS

```rust
pub const DEFAULT_TAPS: &[SkillTapConfig] = &[
    SkillTapConfig { repo: "openai/skills",                  path: "skills/", trust: TrustLevel::Trusted   },
    SkillTapConfig { repo: "anthropics/skills",              path: "skills/", trust: TrustLevel::Trusted   },
    SkillTapConfig { repo: "VoltAgent/awesome-agent-skills", path: "skills/", trust: TrustLevel::Community },
    SkillTapConfig { repo: "garrytan/gstack",                path: "",        trust: TrustLevel::Community },
];
```

### 6.5 CLI Command Wiring

Implement all commands from `010_cli_commands.md`:

```
/plugins list [--all] [--kind TYPE]
/plugins info <name>
/plugins install <identifier>
/plugins remove <name>
/plugins hub search [QUERY] [--limit N]
/plugins hub inspect <identifier>
/plugins hub taps [add|remove] [URL]
/plugins update [name]
/plugins lock
```

### 6.6 Tests Required

- [ ] `GitHubAuth`: priority 1 (env var) resolves without gh CLI
- [ ] `GitHubAuth`: falls back to unauthenticated when no token
- [ ] `GitHubSource.inspect()`: downloads only SKILL.md (not full bundle)
- [ ] `GitHubSource.fetch()`: assembles full SkillBundle
- [ ] Truncated Git Trees response triggers fallback to Contents API
- [ ] Same skill in 2 taps: higher-trust tap wins (D-27)
- [ ] `should_allow_install(community, safe)` → `Some(true)` (AUTO-ALLOW, not "ask")
- [ ] `should_allow_install(agent_created, dangerous)` → `None` (allow+warn, not block)
- [ ] `edgecrab_env_access` pattern fires on `~/.edgecrab/.env` reference
- [ ] Skill with 51 files → caution verdict
- [ ] lock.json written by EdgeCrab parseable by Python fixture (COMPAT-3)

---

## 7. Phase 5 — Tests + CI

**Goal:** Property tests, fuzzing targets, integration tests, and performance benchmarks.

### 7.1 Property Tests (proptest)

```rust
// All edge cases from 014_edge_cases.md as property-based tests
proptest! {
    #[test]
    fn frontmatter_round_trip(s in valid_skill_name_strategy()) { … }
    #[test]
    fn platform_match_exhaustive(os in any_os(), platforms in any_platform_list()) { … }
    #[test]
    fn scanner_no_panic(content in any_string()) { … }
}
```

### 7.2 Fuzz Targets

```
fuzz/fuzz_targets/
├── parse_skill_manifest.rs     arbitrary bytes → must not panic
├── scan_skill_bundle.rs        arbitrary file tree → must not panic
└── parse_lock_json.rs          arbitrary JSON → must not panic
```

Use `cargo fuzz` (libFuzzer). Minimum 1-hour fuzz run before first release tag.

### 7.3 Integration Tests

```
tests/
├── compat_hermes_skills.rs     Load all skills from hermes-agent fixture
├── compat_lock_json.rs         Round-trip lock.json with Python fixture
├── e2e_install_github.rs       #[ignore] — real GitHub API, gated by CI secret
└── e2e_hub_search.rs           #[ignore] — real GitHub API
```

### 7.4 Performance Benchmarks

```
benches/
├── skill_load.rs               time to load 100 skills from disk
└── scan_bundle.rs              time to scan a 50-file bundle
```

Acceptance criteria: loading 100 skills < 50ms; scanning 50-file bundle < 200ms.

### 7.5 COMPAT-* Invariants Coverage

| Invariant | Test location |
|---|---|
| COMPAT-0 | `skill/manifest.rs` unit tests |
| COMPAT-1 | `tests/compat_hermes_skills.rs` |
| COMPAT-2 | `skill/loader.rs` unit tests (checksum before/after) |
| COMPAT-3 | `tests/compat_lock_json.rs` |
| COMPAT-4 | `types.rs` parse tests |
| COMPAT-5 | `skill/platform.rs` proptest |
| COMPAT-6 | `guard/limits.rs` compile-time check |
| COMPAT-7 | `skill/inject.rs` unit tests |
| COMPAT-8 | `skill/readiness.rs` integration tests |
| COMPAT-9 | CLI snapshot tests |
| COMPAT-10 | `guard/scanner.rs` unit tests |

---

## 8. Dependency Budget

| Dependency | Version | Feature gate | Justification |
|---|---|---|---|
| `serde` | 1 | all | JSON/TOML parsing |
| `serde_json` | 1 | all | JSON-RPC, lock.json |
| `toml` | 0.8 | all | plugin.toml |
| `thiserror` | 1 | all | Error types |
| `tracing` | 0.1 | all | Logging |
| `regex` | 1 | all | Name/env var validation |
| `fancy-regex` | 0.13 | guard | Lookbehind in threat patterns |
| `reqwest` | 0.12 | hub | HTTP client |
| `base64` | 0.22 | hub | GitHub App JWT |
| `jsonwebtoken` | 9 | hub | GitHub App JWT |
| `rhai` | 1 | script | Scripting engine |
| `md5` | 0.7 | skill | Bundled manifest hashing |
| `tempfile` | 3 | tests | Quarantine dir |
| `proptest` | 1 | tests | Property-based tests |

---

## 9. Migration from Mock Implementations

The `edgecrab-cli` crate currently has placeholder implementations for some plugin/skill
commands. When Phase 1 ships:

1. Remove placeholder `skills_list()` from `edgecrab-cli/src/commands/plugins.rs`
2. Replace with call to `edgecrab_plugins::skill::loader::scan_skills_dir()`
3. Remove stub `PluginRegistry::new_empty()` and replace with full registry
4. Update `edgecrab-core/src/prompt_builder.rs` to call
   `plugin_registry.collect_skill_fragments()` instead of the hardcoded empty vec

All these touchpoints must be covered by at least one test before merging Phase 1.

---

## 10. Release Milestones

| Milestone | Contents | Target |
|---|---|---|
| **v0.2.0** | Phase 0 + Phase 1 (SkillPlugin) + Phase 1.5 (TUI Toggle) | Sprint 1 |
| **v0.2.1** | Phase 2 (ToolServerPlugin) | Sprint 2 |
| **v0.3.0** | Phase 3 (ScriptPlugin) | Sprint 3 |
| **v0.4.0** | Phase 4 (Hub + Guard) | Sprint 4 |
| **v0.5.0** | Phase 5 (Tests + CI) | Sprint 5 |

Each milestone must pass: `cargo test`, `cargo clippy -- -D warnings`, and the
`014_edge_cases.md` checklist for that phase's scope.

---

## 11. Definition of Done

A phase is DONE when:

1. All deliverables in its section are merged to `main`.
2. All tests listed in its section pass (including any `#[ignore]` tests with `--include-ignored`).
3. `cargo clippy -- -D warnings` produces zero warnings.
4. The COMPAT-* invariants for that phase's scope are covered.
5. The `014_edge_cases.md` edge cases relevant to that phase have corresponding tests.
6. `015_hermes_compatibility.md` §12 checklist boxes for that phase are checked.
7. `CHANGELOG.md` entry written.

---

## 12. Open Questions

| Q# | Question | Blocking |
|---|---|---|
| Q1 | Should `edgecrab-plugins` be a separate published crate or workspace-only? | Phase 0 |
| Q2 | Keychain integration (macOS Keychain / GNOME Keyring) for collected secrets? | Phase 1 |
| Q3 | Interactive credential wizard in TUI vs. plain stdin? **Resolved: reuse `CredentialPromptState` from MCP flow (see [017_plugin_tui] §5.2).** | Phase 1.5 |
| Q4 | `ToolServerPlugin` — MCP vs. custom JSON-RPC 2.0? Are they the same wire protocol? | Phase 2 |
| Q5 | Rhai vs. Lua vs. WASM for ScriptPlugin? Rhai chosen but not final. | Phase 3 |
| Q6 | Hub rate-limit handling — exponential backoff or 403 hard-fail? | Phase 4 |
| Q7 | ClaWhub `https://clawhub.io` — is the API stable? | Phase 4 |
