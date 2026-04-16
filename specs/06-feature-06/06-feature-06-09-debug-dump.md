# ADR-0609: `edgecrab dump` & `/debug` Slash Command

| Field       | Value                                                   |
|-------------|---------------------------------------------------------|
| Status      | Implemented                                             |
| Date        | 2026-04-14                                              |
| Implements  | hermes-agent `hermes dump` + NEW `/debug` slash command |
| Crate       | `edgecrab-cli`                                          |
| File        | `crates/edgecrab-cli/src/dump.rs` (NEW)                 |

---

## 1. Context

When users request support on Discord/GitHub, they need a fast way to share
their configuration state. hermes-agent ships `hermes dump` via
`hermes_cli/dump.py:run_dump()` — a compact, plain-text summary designed for
copy-pasting into support channels.

EdgeCrab already has `/doctor` (comprehensive diagnostics with auto-fix).
`edgecrab dump` is a **complementary** feature: lightweight, non-interactive,
copy-paste-friendly, designed for support threads.

### Distinction: `/doctor` vs `/debug` vs `edgecrab dump`

```
+---------------------------------------------------------------+
|  edgecrab doctor         Full diagnostics + --fix remediation  |
|  /doctor                 Slash command → runs doctor inline    |
|                                                                |
|  edgecrab dump           Compact support summary (NEW)         |
|  /debug                  Slash command → runs dump inline      |
+---------------------------------------------------------------+
```

| Aspect        | `/doctor`                    | `/debug` (new)              |
|---------------|------------------------------|-----------------------------|
| Purpose       | Diagnose & fix               | Share config for support     |
| Output        | Sectioned, colored, verbose  | Compact, plain text, flat    |
| Auto-fix      | Yes (`--fix`)                | No                          |
| API checks    | Yes (connectivity tests)     | No (only set/not-set)       |
| Target        | User self-service            | Copy-paste into Discord      |

---

## 2. First Principles

| Principle       | Application                                              |
|-----------------|----------------------------------------------------------|
| **SRP**         | `dump.rs` only gathers state; no side effects            |
| **Secure**      | API keys show set/not-set only; `--show-keys` shows      |
|                 | first 4 + last 4 chars (never full key)                  |
| **DRY**         | Reuses `AppConfig` loading, `ModelCatalog` for model info |
| **Code is Law** | hermes-agent `hermes_cli/dump.py:run_dump()` as reference|

---

## 3. Architecture

```
+-------------------------------------------------------------------+
|                       dump.rs                                      |
|                                                                    |
|  pub fn run_dump(show_keys: bool) -> String                        |
|    |                                                               |
|    +-- Section 1: Environment                                      |
|    |   version, git_commit, os, arch, rustc_version                |
|    |                                                               |
|    +-- Section 2: Configuration                                    |
|    |   edgecrab_home, model, provider, terminal backend            |
|    |                                                               |
|    +-- Section 3: API Keys                                         |
|    |   22+ env vars: set/not-set (or redacted prefix)              |
|    |                                                               |
|    +-- Section 4: Features                                         |
|    |   toolsets, mcp_servers, gateway status, platforms,            |
|    |   cron_jobs, skills, plugins, memory_provider                  |
|    |                                                               |
|    +-- Section 5: Config Overrides                                 |
|    |   Non-default values vs DEFAULT_CONFIG                        |
|    |                                                               |
|    +-- Output: plain text block with --- markers ---               |
+-------------------------------------------------------------------+
|                                                                    |
|  CLI entry:  edgecrab dump [--show-keys]                           |
|  TUI entry:  /debug → calls run_dump(false), prints to chat       |
+-------------------------------------------------------------------+
```

---

## 4. Data Model

### 4.1 Output Format

```text
--- edgecrab dump ---
version:    0.6.0
commit:     a1b2c3d4
os:         macos aarch64
rust:       1.83.0
home:       ~/.edgecrab
model:      anthropic/claude-opus-4.6
provider:   anthropic
terminal:   local

api_keys:
  ANTHROPIC_API_KEY:    set  (sk-a...xYzW)
  OPENAI_API_KEY:       set
  OPENROUTER_API_KEY:   not set
  GOOGLE_API_KEY:       not set
  ...

features:
  toolsets:     core, browser
  mcp_servers:  2 configured
  gateway:      inactive
  platforms:    telegram, discord
  cron_jobs:    3 active / 5 total
  skills:       12 installed
  plugins:      2 loaded
  memory:       built-in

config_overrides:
  max_iterations:        120  (default: 90)
  compression.threshold: 0.60 (default: 0.50)
  display.skin:          ares (default: default)
--- end dump ---
```

### 4.2 API Keys Checked

```rust
/// Environment variables checked for API key status.
/// Source: hermes-agent dump.py + edgecrab-specific additions.
const API_KEY_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "OPENROUTER_API_KEY",
    "GOOGLE_API_KEY",
    "MISTRAL_API_KEY",
    "DEEPSEEK_API_KEY",
    "GROQ_API_KEY",
    "XAI_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "CEREBRAS_API_KEY",
    "SAMBANOVA_API_KEY",
    "COHERE_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AWS_ACCESS_KEY_ID",
    "TELEGRAM_BOT_TOKEN",
    "DISCORD_BOT_TOKEN",
    "SLACK_BOT_TOKEN",
    "WHATSAPP_ACCESS_TOKEN",
    "MATRIX_ACCESS_TOKEN",
    "MATTERMOST_TOKEN",
    "GITHUB_TOKEN",
];
```

### 4.3 Redaction

```rust
/// Redact API key: show first 4 + last 4 chars.
/// Matches hermes-agent dump.py:_redact()
fn redact_key(key: &str) -> String {
    if key.len() <= 12 {
        return "****".to_string();
    }
    format!("{}...{}", &key[..4], &key[key.len()-4..])
}
```

### 4.4 Config Overrides Detection

```rust
/// Config paths to diff against defaults.
/// Source: hermes-agent dump.py:_config_overrides()
const INTERESTING_PATHS: &[&str] = &[
    "max_iterations",
    "streaming",
    "save_trajectories",
    "skip_context_files",
    "skip_memory",
    "compression.threshold",
    "compression.protect_last_n",
    "display.skin",
];
```

---

## 5. CLI Integration

### 5.1 Subcommand

```
USAGE:
    edgecrab dump [OPTIONS]

OPTIONS:
    --show-keys     Show redacted API key prefixes (first/last 4 chars)
```

### 5.2 Slash Command

| Command   | Aliases    | Category | Description                        |
|-----------|------------|----------|------------------------------------|
| `/debug`  | `/dump`    | Info     | Show compact setup summary         |

```rust
// In commands.rs
CommandDef {
    name: "debug",
    description: "Show compact setup summary for support",
    category: CommandCategory::Info,
    aliases: &["dump"],
    args_hint: None,
    cli_only: false,
    gateway_only: false,
}
```

### 5.3 Handler in `app.rs`

```rust
"debug" | "dump" => {
    let output = crate::dump::run_dump(false);
    self.push_system_message(&output);
}
```

---

## 6. Edge Cases & Roadblocks

| #  | Edge Case                              | Remediation                                         |
|----|----------------------------------------|------------------------------------------------------|
| 1  | API key value is very short (<8 chars) | Redact to `****` if len <= 12                        |
| 2  | Config file missing                    | Show "config: missing" instead of panicking          |
| 3  | Git not installed (no commit hash)     | Show "commit: unknown"                               |
| 4  | EDGECRAB_HOME override                 | Use resolved home path, not hardcoded `~/.edgecrab`  |
| 5  | sessions.db locked/corrupt             | Skip session stats, show "sessions: unavailable"     |
| 6  | MCP server config parse error          | Show "mcp_servers: parse error" instead of panic     |
| 7  | Skills directory missing               | Show "skills: 0 installed"                           |
| 8  | No terminal (piped output)             | Plain text format works anywhere — no ANSI colors    |
| 9  | Unicode in skin name                   | Display skin name as-is (UTF-8 safe)                 |
| 10 | Concurrent dump + config write         | Read-only snapshot — no locking needed               |

---

## 7. Implementation Plan

### 7.1 Files to Create

| File                                   | Purpose                              |
|----------------------------------------|--------------------------------------|
| `crates/edgecrab-cli/src/dump.rs`      | `run_dump()` and helpers             |

### 7.2 Files to Modify

| File                                   | Change                               |
|----------------------------------------|--------------------------------------|
| `crates/edgecrab-cli/src/main.rs`      | Add `dump` subcommand                |
| `crates/edgecrab-cli/src/commands.rs`  | Add `debug`/`dump` command def       |
| `crates/edgecrab-cli/src/app.rs`       | Handle `/debug` slash command        |

### 7.3 Dependencies

None — uses only `std::env`, existing `AppConfig`, `ModelCatalog`.

### 7.4 Test Matrix

| Test                                 | Validates                                     |
|--------------------------------------|------------------------------------------------|
| `test_dump_output_format`            | Output has `--- edgecrab dump ---` markers     |
| `test_dump_no_full_keys`             | Full API key values never appear in output     |
| `test_dump_redact_short_key`         | Keys ≤12 chars → `****`                        |
| `test_dump_redact_normal_key`        | Keys >12 chars → `sk-a...xYzW` format         |
| `test_dump_missing_config`           | Graceful output when config.yaml absent        |
| `test_dump_missing_git`              | Shows "commit: unknown" when git unavailable   |
| `test_dump_edgecrab_home_override`   | Respects EDGECRAB_HOME env var                 |
| `test_dump_show_keys_flag`           | `--show-keys` includes redacted prefixes       |
| `test_dump_no_show_keys`             | Default mode shows only set/not-set            |

---

## 8. Acceptance Criteria

- [ ] `edgecrab dump` outputs compact plain-text summary
- [ ] API keys show set/not-set by default
- [ ] `--show-keys` shows first 4 + last 4 chars, never full key
- [ ] `/debug` slash command invokes `run_dump()` inline
- [ ] Output is copy-paste friendly (no ANSI escape codes)
- [ ] Handles missing config/git/sessions gracefully
- [ ] Respects `EDGECRAB_HOME` for profile-aware paths
- [ ] All tests pass: `cargo test -p edgecrab-cli -- dump`
