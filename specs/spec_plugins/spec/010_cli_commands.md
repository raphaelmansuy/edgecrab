# CLI Commands — `/plugins` Slash Command Specification

**Status:** PROPOSED  
**Version:** 0.2.0  
**Date:** 2026-04-09  
**Cross-refs:** [004_plugin_types], [005_lifecycle], [007_registry], [009_discovery_hub], [011_config_schema], [015_hermes_compatibility], [017_plugin_tui], [018_remote_plugin_search_tui]

**EdgeCrab source files (CODE IS LAW):**
- `crates/edgecrab-cli/src/commands.rs` — `CommandRegistry`, `CommandResult` enum, `COMMAND_REGISTRY` static list
- `crates/edgecrab-cli/src/plugins_cmd.rs` — existing `PluginAction::{List, Install, Update, Remove}`, `run()` dispatch
- `crates/edgecrab-cli/src/app.rs` — `App::handle_command_result()`, TUI overlay dispatch
- `crates/edgecrab-cli/src/plugins.rs` — `PluginManager::discover()`, `Plugin`, `PluginSource`

**hermes-agent source files (CODE IS LAW — reference implementation):**
- `hermes-agent/hermes_cli/commands.py` — `COMMAND_REGISTRY`, `CommandDef`, `resolve_command()` pattern
- `hermes-agent/hermes_cli/plugins.py` — `/plugins` command handler (skills + toolsets combined)

---

## 1. Overview

The `/plugins` command integrates into EdgeCrab's existing slash command system
defined in `crates/edgecrab-cli/src/commands.rs`. The baseline plugin CLI dispatch is
implemented in `crates/edgecrab-cli/src/plugins_cmd.rs` (`PluginAction` enum, `run()`
function) — this document specifies the additions to that file.

```
/plugins                  → alias for /plugins list
/plugins list             → show installed plugins
/plugins info <name>      → detailed info for one plugin
/plugins install <source> → install a plugin
/plugins remove <name>    → uninstall a plugin
/plugins enable <name>    → enable a disabled plugin
/plugins disable <name>   → disable a running plugin
/plugins status           → runtime health of all plugins
/plugins upgrade [<name>] → upgrade one or all plugins
/plugins audit            → show security audit log
/plugins search [<q>]     → open remote plugin browser (optionally seeded with query)
/plugins search --source <name> <q> → open remote browser scoped to one source
/plugins browse           → open remote plugin browser with empty query
/plugins hub              → hub command alias group (search, browse, refresh)
/plugins hub search <q>   → alias of /plugins search <q>
/plugins hub browse       → alias of /plugins browse
/plugins hub refresh      → clear hub index cache
/plugins toggle           → interactive enable/disable overlay (TUI)
/plugins toggle <name>    → toggle a named plugin directly (non-interactive)
```

> **TUI detail:** `/plugins toggle` opens the local enable/disable overlay and
> `/plugins search` or `/plugins browse` opens the remote plugin browser. The
> remote browser is specified in **[018_remote_plugin_search_tui]** and shares
> the same browser-state architecture as the remote skills browser.

---

## 2. Registration in `crates/edgecrab-cli/src/commands.rs`

**Baseline file:** `crates/edgecrab-cli/src/plugins_cmd.rs` — currently implements `PluginAction::{List, Install, Update, Remove}`. The entries below extend `COMMAND_REGISTRY` in `crates/edgecrab-cli/src/commands.rs` and add `CommandResult::ShowPluginToggle` to the `CommandResult` enum.  
**hermes reference:** `hermes-agent/hermes_cli/commands.py` `COMMAND_REGISTRY` for the `CommandDef` struct pattern.

The following `CommandDef` entries are added to `COMMAND_REGISTRY`:

```rust
CommandDef("plugins",         "Manage plugins",                    "Tools & Skills",
           aliases=&["plugin"], args_hint="[subcommand]"),
CommandDef("plugins list",    "List installed plugins",            "Tools & Skills",
           args_hint="[--all|--running|--disabled]"),
CommandDef("plugins info",    "Show details for a plugin",         "Tools & Skills",
           args_hint="<name>"),
CommandDef("plugins install", "Install a plugin from hub or path", "Tools & Skills",
           args_hint="<source> [--no-enable]"),
CommandDef("plugins remove",  "Uninstall a plugin",                "Tools & Skills",
           args_hint="<name> [--force]"),
CommandDef("plugins enable",  "Enable a disabled plugin",          "Tools & Skills",
           args_hint="<name>"),
CommandDef("plugins disable", "Disable a running plugin",          "Tools & Skills",
           args_hint="<name>"),
CommandDef("plugins status",  "Show runtime health of all plugins","Tools & Skills"),
CommandDef("plugins upgrade", "Upgrade one or all plugins",        "Tools & Skills",
           args_hint="[<name>]"),
CommandDef("plugins audit",   "Show plugin security audit log",    "Tools & Skills",
           args_hint="[--lines N]"),
CommandDef("plugins hub",     "Browse and search the plugin hub",  "Tools & Skills",
           args_hint="[search <q>|browse|refresh]"),
CommandDef("plugins toggle",  "Interactive plugin enable/disable TUI", "Tools & Skills",
           args_hint="[<name>] [--platform <p>]"),
// Note: "plugins enable" and "plugins disable" are also routing aliases for
// the toggle subcommand when a <name> argument is provided (non-interactive path).
```

---

## 3. Command Specifications

### 3.1 `/plugins list`

```
USAGE:
  /plugins list [--all] [--running] [--disabled]

FLAGS:
  --all       Show all states (default)
  --running   Show only running plugins
  --disabled  Show only disabled plugins

OUTPUT:
  INSTALLED PLUGINS
  ─────────────────────────────────────────────────────────
  github-tools     v1.2.0   [RUNNING]   tool-server   Official
    Create issues, PRs, and search GitHub repositories
    Tools: create_github_issue, search_github, create_pr

  my-skill         v0.3.1   [RUNNING]   skill         Unverified
    Custom Rust snippets and patterns

  old-helper       v0.1.0   [DISABLED]  tool-server   Community
    ...
  ─────────────────────────────────────────────────────────
  3 plugins (2 running, 1 disabled)
  Type /plugins enable old-helper to start it.

EMPTY STATE:
  No plugins installed.
  Use /plugins hub search <query> to find plugins.
  Use /plugins install <source> to install one.
```

### 3.2 `/plugins info <name>`

```
USAGE:
  /plugins info <name>

OUTPUT:
  PLUGIN: github-tools
  ────────────────────────────────────────────────────────
  Version:      1.2.0
  Kind:         ToolServer (subprocess, Python)
  State:        RUNNING (PID 84213)
  Trust Level:  Official
  Installed:    2026-04-07 14:23:11 UTC
  Source:       github:edgecrab/plugins/github-tools
  License:      MIT
  Author:       EdgeCrab Team
  Homepage:     https://github.com/edgecrab/plugins/tree/main/github-tools

  Description:
    Create GitHub issues, open pull requests, and search
    repositories without leaving your conversation.

  Tools Provided:
    create_github_issue   Create a new issue in any repository
    search_github         Search code, issues, and PRs
    create_pr             Open a pull request

  Required Env:
    GITHUB_TOKEN          GitHub personal access token

  Capabilities:
    memory_read           false
    secret_get            GITHUB_TOKEN
    tool_delegate         false

  Restart Policy:
    restart_on_crash      true (max 3, count: 0)
  ────────────────────────────────────────────────────────
  /plugins disable github-tools   /plugins remove github-tools
```

### 3.3 `/plugins install <source>`

```
USAGE:
  /plugins install <source> [--no-enable] [--force]

ARGUMENTS:
  <source>     One of:
                 github:owner/repo/path
                 https://example.com/plugin.zip
                 local:/path/to/plugin-dir
                 ./relative/path

FLAGS:
  --no-enable  Install but do not start/enable the plugin
  --force      Allow Community trust level even if scanner says Caution

FLOW:
  1. Resolve source URL
  2. Download to quarantine
  3. Run security scanner
  4. If Dangerous: abort, show threat report
  5. If Caution + no --force: ask user confirmation
  6. If Safe or (Caution + --force): proceed
  7. Move out of quarantine → installed dir
  8. Start plugin (unless --no-enable)
  9. Show success output

OUTPUT (success):
  Installing github-tools...
  - Downloading from github:edgecrab/plugins/github-tools  OK
  - Verifying checksum sha256:abc123...                     OK
  - Security scan: Safe (0 findings)                        OK
  - Starting plugin...                                      OK

  github-tools v1.2.0 installed and running.
  New tools available: create_github_issue, search_github, create_pr

OUTPUT (caution, awaiting user):
  Installing anon-plugin...
  - Downloading...    OK
  - Checksum...       OK
  - Security scan: Caution (2 findings)

  Findings:
    MEDIUM  network:outbound_http   Lines 12, 45
      Plugin makes HTTP requests (expected for this tool type)
    LOW     obfuscation:base64      Line 78
      Base64 decoding present (may be benign)

  Trust level: Community
  Proceed with installation? [y/N]:

OUTPUT (dangerous, blocked):
  Installing malicious-plugin...
  - Downloading...    OK
  - Checksum...       OK
  - Security scan: Dangerous (1 findings)

  Findings:
    CRITICAL  exfiltration:env_dump   Line 23
      Code reads all environment variables and may exfiltrate them

  Installation blocked. This plugin has critical security issues.
  If you trust this source, inspect the code manually and install with --force.
```

### 3.4 `/plugins remove <name>`

```
USAGE:
  /plugins remove <name> [--force]

FLAGS:
  --force   Skip confirmation prompt

OUTPUT:
  Remove github-tools? This will:
  - Stop the running subprocess (PID 84213)
  - Remove tools: create_github_issue, search_github, create_pr
  - Delete ~/.edgecrab/plugins/github-tools/
  [y/N]:

  Removing github-tools...   OK
  github-tools removed.
```

### 3.5 `/plugins enable <name>`

```
USAGE:
  /plugins enable <name>

OUTPUT:
  Enabling github-tools...
  - Starting subprocess...   OK
  - Registering tools...     OK (3 tools)
  github-tools is now running.

ERROR (already running):
  github-tools is already running.

ERROR (not installed):
  Plugin not found: github-tools
  Use /plugins list to see installed plugins.
```

### 3.6 `/plugins disable <name>`

```
USAGE:
  /plugins disable <name>

OUTPUT:
  Disabling github-tools...
  - Removing tools from dispatch...   OK
  - Sending shutdown signal...        OK
  github-tools is now disabled.
```

### 3.7 `/plugins status`

Shows runtime health of all plugins. Designed to match `/doctor` output style.

```
USAGE:
  /plugins status

OUTPUT:
  PLUGIN RUNTIME STATUS
  ─────────────────────────────────────────────────
  github-tools     RUNNING    pid=84213  mem=12MB   restarts=0
  my-skill         RUNNING    (no subprocess)        restarts=0
  old-helper       DISABLED   ---                    restarts=0
  ─────────────────────────────────────────────────
  Runtime tools: 3 | Skill injections: 1 | Total: 3 plugins

  Host API call rate (last 60s):
    github-tools:  host:secret_get x2, host:log x7
```

### 3.8 `/plugins upgrade [<name>]`

```
USAGE:
  /plugins upgrade [<name>]

  If <name> is omitted, upgrade all upgradeable plugins.

OUTPUT (one plugin):
  Checking github-tools for updates...
  Current: v1.2.0
  Latest:  v1.3.0  (source: edgecrab-official)

  Changes in v1.3.0:
    - Add create_pr_draft tool
    - Fix rate limit handling

  Upgrading github-tools...
  - Download v1.3.0...       OK
  - Security scan...         OK
  - Atomic swap...           OK
  - Restart plugin...        OK
  github-tools upgraded to v1.3.0.

OUTPUT (all, no updates):
  All 3 plugins are up to date.
```

If the installed plugin manifest contains `trust.source`, upgrade MUST use that
stamped remote source for re-materialization, re-scan, checksum verification, and
atomic replacement. Only plugins without a stamped remote source fall back to
`git pull --ff-only`.

### 3.9 `/plugins audit [--lines N]`

```
USAGE:
  /plugins audit [--lines N]   (default N=50)

OUTPUT:
  PLUGIN AUDIT LOG (last 50 entries)
  ────────────────────────────────────────────────────────────
  2026-04-09 14:23:11  INSTALL    github-tools  v1.2.0  verdict=Safe
  2026-04-09 14:23:12  ENABLE     github-tools  state=Running
  2026-04-09 14:23:45  HOST_CALL  github-tools  host:secret_get key=GITHUB_TOKEN  ok=true
  2026-04-09 15:01:12  HOST_CALL  github-tools  host:log level=info  ok=true
  ────────────────────────────────────────────────────────────
  Showing 4 / 4 total audit entries.
```

### 3.9b `/plugins toggle [<name>] [--platform <p>]`

> **Full TUI specification:** [017_plugin_tui]. This section covers only the
> command surface — argument parsing, non-TUI output, and CommandResult dispatch.

```
USAGE:
  /plugins toggle                               # open TUI (global scope)
  /plugins toggle --platform <platform>         # open TUI (platform scope)
  /plugins toggle <name>                        # non-interactive: toggle named plugin
  /plugins toggle <name> --platform <platform>  # non-interactive: platform-scoped toggle

PLATFORMS: cli, telegram, discord, slack, whatsapp, signal, email,
           homeassistant, mattermost, matrix, dingtalk, feishu, wecom, webhook

NON-INTERACTIVE OUTPUT (when <name> is given):
  ✓ my-tools is now enabled. (Global)    ~220 tokens added to context.
  ✓ my-tools is now disabled. (Global)   ~220 tokens removed from context.
  ✓ my-tools is now enabled. (telegram)
  ✓ my-tools is now disabled. (telegram)

  Error: Plugin 'xxx' not found. Use /plugins list.
  Error: Platform 'xxx' is not enabled. Use /platforms to see active platforms.

TUI OUTPUT (when no <name> given):
  (overlay opens — see [017_plugin_tui] §3 for the full screen layout)
  On ENTER confirm:
    ✓ Saved: 2 enabled, 1 disabled (Global).
  On Esc:
    (silent — no output, no config change)
```

### 3.10 `/plugins search [<query>]`

```
USAGE:
  /plugins search [<query>]
  /plugins search --source <source> <query>
  /plugins hub search <query>           # alias

CLI/TUI BEHAVIOR:
  In the TUI, this opens the remote plugin browser immediately.
  If <query> is provided, it seeds the fuzzy-search field and kicks off search.
  If --source is provided, the browser is restricted to that source family.
  Outside the TUI, the command may render text output from the flat search API.

TUI RESULT MODEL:
  - Source-grouped results with a visible source label per row
  - Default action is derived per result:
      install  = plugin not installed locally
      update   = installed plugin was hub-installed from the same identifier
      replace  = local name collision exists but install source differs
  - Per-source failures are shown as source notes, not fatal browser errors
```

### 3.11 `/plugins browse`

Opens the remote plugin browser with an empty query. The browser does not eagerly
fetch every registry entry; it shows configured source summaries and waits for the
user to type a query. `/plugins hub browse` is an alias.

```
USAGE:
  /plugins browse
  /plugins hub browse   # alias

TUI BEHAVIOR:
  - Header lists "Remote Plugins"
  - Detail pane lists configured registry sources and trust levels
  - Typing starts a debounced remote search
  - `R` from the local plugin toggle opens this browser
  - `L` from this browser returns to the local plugin toggle
```

### 3.12 `/plugins hub refresh`

```
USAGE:
  /plugins hub refresh

OUTPUT:
  Refreshing plugin hub indices...
  - edgecrab-official  42 plugins   OK
  - community          183 plugins  OK
  Hub refreshed. 225 plugins available.
```

---

## 4. CommandResult Variants

Extending the existing `CommandResult` enum:

```rust
pub enum CommandResult {
    // ... existing variants ...
    PluginList(Vec<PluginInfo>),
    PluginInfo(PluginInfo),
    PluginInstalled(PluginInfo),
    PluginRemoved(String),
    PluginEnabled(String),
    PluginDisabled(String),
    PluginStatus(Vec<PluginRuntimeStatus>),
    PluginUpgraded(String, String),  // name, new_version
    PluginAudit(Vec<AuditEntry>),
    PluginHubResults(Vec<PluginSearchResult>),
    PluginConfirmationRequired(PluginInstallContext),
    /// Open the plugin toggle TUI overlay.
    /// name=None → open interactive TUI, name=Some(_) → non-interactive toggle.
    /// See [017_plugin_tui] for the full overlay specification.
    ShowPluginToggle {
        name: Option<String>,
        platform: Option<String>,
    },
}
```

---

## 5. Gateway Support

The following commands are available in gateway (Telegram, Discord, etc.) mode:

| Command | Gateway support |
|---|---|
| `/plugins list` | Yes (text output) |
| `/plugins info` | Yes (text output) |
| `/plugins install` | No (requires confirmation TUI) |
| `/plugins enable` | No (admin-only operation) |
| `/plugins disable` | No (admin-only operation) |
| `/plugins remove` | No (admin-only operation) |
| `/plugins status` | Yes (text output) |
| `/plugins upgrade` | No |
| `/plugins audit` | No |
| `/plugins search` | Yes (text output outside TUI) |
| `/plugins browse` | No (TUI only) |
| `/plugins hub refresh` | No |

The `cli_only: true` flag is set on all install-class commands.

---

## 6. Auto-Completion

The existing `SlashCommandCompleter` is extended to provide:

```
/plugins <TAB>
  list  info  install  remove  enable  disable  status  upgrade  audit  hub

/plugins enable <TAB>
  <shows only disabled plugin names>

/plugins disable <TAB>
  <shows only running plugin names>

/plugins remove <TAB>
  <shows all installed plugin names>

/plugins hub <TAB>
  search  browse  refresh
```

---

## 7. Error Messages

| Situation | Message |
|---|---|
| Plugin not found | `Plugin not found: <name>. Use /plugins list to see installed plugins.` |
| Already running | `<name> is already running.` |
| Already disabled | `<name> is already disabled.` |
| Tool name conflict | `Cannot enable <name>: tool '<tool>' conflicts with existing tool from <other>.` |
| Scanner blocked | `Installation blocked: <name> has critical security threats. See findings above.` |
| Network error | `Download failed: <error>. Try again or install from a local path.` |
| Subprocess crash | `<name> crashed on startup. Check /plugins audit for details.` |
| No hub sources | `No hub sources configured. Check your config.yaml plugins.hub.sources.` |
