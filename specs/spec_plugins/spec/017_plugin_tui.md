# Plugin Activation/Deactivation TUI — Specification

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:**
- [004_plugin_types] — SkillPlugin / ToolServerPlugin / ScriptPlugin definitions
- [005_lifecycle] — plugin enable/disable lifecycle contract
- [010_cli_commands] — `/plugins toggle` command registration and `CommandResult::ShowPluginToggle`
- [011_config_schema] — §3 canonical definitions of `plugins.disabled` / `plugins.platform_disabled.*`
- [013_plugins_skills_relation] — how plugin kinds relate to skills
- [015_hermes_compatibility] — COMPAT-* invariants that TUI must not break
- [018_remote_plugin_search_tui] — remote plugin search/browser overlay
- [016_implementation_plan] — Phase 1.5 step-by-step implementation sequencing

**EdgeCrab source files (CODE IS LAW):**
- `crates/edgecrab-cli/src/app.rs` — `App` struct, overlay state fields, key-event dispatch, render loop
- `crates/edgecrab-cli/src/fuzzy_selector.rs` — `FuzzySelector<T>`, `FuzzyItem` trait
- `crates/edgecrab-cli/src/commands.rs` — `CommandRegistry`, `CommandResult` enum
- `crates/edgecrab-cli/src/plugins.rs` — `PluginManager`, `Plugin`, `PluginSource`, `PluginHook`
- `crates/edgecrab-cli/src/plugins_cmd.rs` — `PluginAction`, `run()`, `list_plugins()`
- `crates/edgecrab-cli/src/theme.rs` — `Theme` struct and color constants
- `crates/edgecrab-cli/src/mcp_catalog.rs` — MCP overlay pattern (reference for provider picker)
- `crates/edgecrab-core/src/config.rs` — `AppConfig`, `McpServerConfig`, config persistence

**hermes-agent source files (CODE IS LAW — reference implementation):**
- `hermes-agent/hermes_cli/skills_config.py` — `skills_command()`, `_select_platform()`, `save_disabled_skills()`
- `hermes-agent/hermes_cli/curses_ui.py` — `curses_checklist()`, `_numbered_fallback()`, `status_fn` signature
- `hermes-agent/hermes_cli/tools_config.py` — `CONFIGURABLE_TOOLSETS`, `_prompt_toolset_checklist()`, `_configure_toolset()`, token estimation

---

## 0. First Principles: WHY Users Need to Toggle Plugins

| Motivation | Explanation |
|---|---|
| **Context budget pressure** | Every enabled plugin contributes tool schemas to the LLM prompt (~100–500 tokens each). Power users disable unused plugins to reduce prompt size and cost. |
| **Platform scoping** | A plugin that accesses home automation makes no sense on Discord. Per-platform toggles prevent tool-context pollution. |
| **Security surface reduction** | Fewer enabled plugins = smaller attack surface from injected tool calls. |
| **Credential hygiene** | Some plugins require API keys. Users may want a plugin installed but inactive until they configure credentials. |
| **Developer workflow** | During plugin development, users toggle a plugin off-and-on without restarting. |

**First-Principles Design Rules** (derived from the above):

1. Toggle MUST be instant — no restart required (plugins are loaded per-agent-turn, not at startup).
2. Toggle state MUST persist to `~/.edgecrab/config.yaml` immediately on confirm.
3. The UI MUST provide a live token-cost estimate that updates as items are toggled.
4. Disabling a plugin MUST NOT delete it — a disabled plugin stays on disk, invisible to the LLM.
5. Platform-scoped toggles MUST fall back gracefully to the global state when no platform-specific rule exists.

---

## 1. Scope and Relationship to Existing Overlays

### 1.1 Existing Overlay Inventory (CODE IS LAW — `crates/edgecrab-cli/src/app.rs`)

EdgeCrab already ships these ratatui overlays (verified by reading `App` struct fields at lines ~2923–2989 of `app.rs`):

| Overlay State Field | Triggered By | Widget type | Key source lines |
|---|---|---|---|
| `model_selector` | `/model` | `FuzzySelector<ModelEntry>` | `App::open_model_selector()` |
| `skill_selector` | `/skills` | `FuzzySelector<…>` | `App::open_skill_selector()` |
| `tool_manager` | `/tools`, `/toolsets` | `FuzzySelector<ToolManagerEntry>` | `App::open_tool_manager()` at ~10088 |
| `mcp_preset_selector` | `/mcp` | `FuzzySelector<…>` | `App::open_mcp_preset_selector()` |
| `session_selector` | `/session` | `FuzzySelector<SessionEntry>` | `App::open_session_selector()` |
| `config_center` | `/config` | dedicated state | `App::open_config_center()` |

The **Plugin Toggle Overlay** MUST follow the exact same structural pattern as `tool_manager`:

```
FuzzySelector<PluginToggleEntry>   →   plugin_toggle overlay
```

**WHY reuse FuzzySelector rather than a separate multi-select widget?**  
`FuzzySelector<T>` (defined in `crates/edgecrab-cli/src/fuzzy_selector.rs`) already provides: scrolling, fuzzy search, j/k navigation, page up/down, and `current()` selection. To support multi-check we store the checked state inside the item (like `ToolManagerCheckState` in `app.rs`) and handle SPACE to toggle it. This is identical to how `tool_manager` works (see `toggle_tool_manager_selected()` at ~10138 in `app.rs`) — zero new TUI infrastructure needed.

### 1.2 Plugin Kinds and Their Toggle Flows

| Plugin Kind | Defined In | Toggle Flow | Provider Wizard? |
|---|---|---|---|
| **SkillPlugin** | `.edgecrab/plugins/<n>/SKILL.md` | Global or per-platform toggle | No |
| **ToolServerPlugin** | `plugin.toml` with `tools` entries | Global or per-platform toggle | Yes (env vars) |
| **ScriptPlugin** | `plugin.toml` with `hooks` entries | Global toggle | No |
| **McpPlugin** | `mcp_servers:` config | Handled by existing `/mcp` overlay | Out of scope |

> **Note:** MCP servers already have a mature toggle mechanism via the `/mcp` command and `enabled:` field in config. This spec covers the three `plugin.toml`/`SKILL.md`-based kinds only.

---

## 2. Component Architecture (DRY + SOLID)

### 2.1 SOLID Analysis of the Tool Manager Pattern

The `tool_manager` overlay in `app.rs` sets the pattern:

```
Single Responsibility:  ToolManagerEntry     = item data only
Open/Closed:            FuzzySelector<T>     = closed, extension via PluginToggleEntry
Liskov:                 PluginToggleEntry    implements FuzzyItem (same as ToolManagerEntry)
Interface Segregation:  FuzzyItem            = 3-method trait (primary, secondary, tag)
Dependency Inversion:   App.open_*()         depends on FuzzySelector<T>, not concrete types
```

### 2.2 New Types Required

```rust
// NEW FILE: crates/edgecrab-cli/src/plugin_toggle.rs
// Pattern source: crates/edgecrab-cli/src/app.rs (ToolManagerEntry, ToolManagerCheckState)
// Trait source:   crates/edgecrab-cli/src/fuzzy_selector.rs (FuzzyItem)

/// Which platform scope this plugin toggle targets.
/// None = global (affects all platforms that don't have an override).
pub enum PluginScope {
    Global,
    Platform(String),  // e.g. "cli", "telegram", "discord", …
}

/// Check state for a plugin toggle entry.
/// Mirrors ToolManagerCheckState exactly.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PluginCheckState {
    On,
    Off,
}

impl PluginCheckState {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::On => "[x]",
            Self::Off => "[ ]",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::On => Self::Off,
            Self::Off => Self::On,
        }
    }
}

/// A single row in the plugin toggle overlay.
#[derive(Clone)]
pub struct PluginToggleEntry {
    /// Plugin identifier (must match `plugin.toml` `name` field).
    pub name: String,
    /// Human-readable label (may include emoji).
    pub display_name: String,
    /// Short one-line description shown in detail pane.
    pub description: String,
    /// Plugin version string.
    pub version: String,
    /// Plugin source: "user" | "project" | "system".
    pub source: String,
    /// Plugin kind: "skill" | "tool-server" | "script".
    pub kind: String,
    /// Number of tools this plugin contributes (ToolServerPlugin only; 0 for others).
    pub tool_count: usize,
    /// Estimated token cost of enabling this plugin (schema tokens).
    pub estimated_tokens: usize,
    /// Current toggle state for the active scope.
    pub check_state: PluginCheckState,
    /// Whether this plugin requires credential setup to be useful.
    pub needs_credentials: bool,
    /// Whether all required env vars are present in the current environment.
    pub credentials_satisfied: bool,
}

impl FuzzyItem for PluginToggleEntry {
    fn primary(&self) -> &str { &self.display_name }
    fn secondary(&self) -> &str { &self.description }
    fn tag(&self) -> &str { &self.kind }
}
```

### 2.3 App State Additions

**In `crates/edgecrab-cli/src/app.rs` `App` struct** (alongside existing `tool_manager: FuzzySelector<ToolManagerEntry>` field, ~line 2959):

```rust
// Plugin toggle overlay (activated by /plugins toggle or /plugins config)
plugin_toggle: FuzzySelector<PluginToggleEntry>,
// Current scope for the plugin toggle overlay.
plugin_toggle_scope: PluginScope,
// One-line descriptive note shown in the plugin toggle status bar.
plugin_toggle_status_note: Option<String>,
```

### 2.4 DRY Helper Functions (no duplication with tool_manager)

```rust
// All in crates/edgecrab-cli/src/app.rs — private impl App { … }
// Naming mirrors tool_manager counterparts (see app.rs ~10088–10222)
fn build_plugin_toggle_entries(scope: &PluginScope) -> Vec<PluginToggleEntry>;
fn open_plugin_toggle(&mut self, scope: PluginScope);
fn toggle_plugin_selected(&mut self);
fn confirm_plugin_toggle(&mut self) -> anyhow::Result<()>;
fn refresh_plugin_toggle_entries(&mut self) -> bool;
```

Exact naming mirrors the `tool_manager` counterparts in `app.rs`:
- `build_tool_manager_entries()` (~10078) → `build_plugin_toggle_entries()`
- `open_tool_manager()` (~10088) → `open_plugin_toggle()`
- `toggle_tool_manager_selected()` (~10138) → `toggle_plugin_selected()`
- `reset_tool_manager_policy()` (~10218) → `(no equivalent — plugins have no "default" policy)`

---

## 3. TUI Widget Specification

### 3.1 Key Bindings (CODE IS LAW — must match existing overlays)

Source: `crates/edgecrab-cli/src/app.rs`, key-handler block `if self.tool_manager.active { … }` (~line 7026); and `FuzzySelector` nav methods in `crates/edgecrab-cli/src/fuzzy_selector.rs` (`move_up`, `move_down`, `page_up`, `page_down`):

| Key | Action |
|---|---|
| `↑` / `k` | Move cursor up |
| `↓` / `j` | Move cursor down |
| `PgUp` | Page up (8 rows) |
| `PgDn` | Page down (8 rows) |
| `SPACE` | Toggle check state of current entry |
| `ENTER` | **Confirm** — persist changes to config, close overlay |
| `Esc` | **Cancel** — discard any in-flight toggle changes, close overlay |
| `Tab` | Cycle platform scope (Global → cli → telegram → discord → … → Global) |
| `r` | Reset current entry to default (same as tool_manager's Reset policy) |
| `/` | Focus the fuzzy-search bar (typing starts filtering without `/`) |
| `F1` | Toggle detail pane fullscreen |
| `?` | Toggle help footer |

> **Design decision:** `Tab` cycles scope (not a new key conflict) because it is already used in overlays like `tool_manager` to cycle `All → Toolsets → Tools`. Scope cycling mirrors that exact UX.

### 3.2 Screen Layout

```
┌─────────────────────────────────────────────────────────────────────┐
│ PLUGIN TOGGLE  [Global]  2 enabled, 1 disabled  Tab: switch scope   │  ← header (yellow bold)
├─────────────────────────────────────────────────────────────────────┤
│ 🔍 > filter...                                                       │  ← search bar (dim)
├─────────────────────────────────────────────────────────────────────┤
│ [x] 🧰 my-tools       tool-server  v1.2.0  ~220 tokens             │  ← row (green+bold = cursor)
│ [ ] 📚 my-skills      skill        v0.3.0  ~80 tokens              │  ← row (normal)
│ [x] 🔧 deploy-hooks  script       v1.0.0  ~0 tokens               │  ← row (normal)
│                                                                     │
├─────────────────────────────────────────────────────────────────────┤
│ DETAIL: my-tools · Official deployment tool suite · Source: user    │  ← detail pane
│ Tools: deploy, rollback, status, logs (4 tools)                     │
│ ⚠ Missing env: DEPLOY_TOKEN                                         │  ← credential warning
├─────────────────────────────────────────────────────────────────────┤
│ ↑↓ navigate  SPACE toggle  ENTER confirm  Tab scope  Esc cancel     │  ← help bar (dim)
│ Est. plugin context: ~300 tokens                    Global scope    │  ← status bar (dim gray, right-aligned)
└─────────────────────────────────────────────────────────────────────┘
```

### 3.3 Visual Design Rules (CODE IS LAW — match ratatui theme in `crates/edgecrab-cli/src/theme.rs`)

| Element | Style |
|---|---|
| Header row | `theme.overlay_title` (typically yellow + bold) |
| Cursor row | `theme.overlay_selected` (typically green + bold) |
| Normal rows | Default foreground |
| Check glyph `[x]` | Green when On, dim when Off |
| Search bar hint | Dim italic |
| Detail pane | Background `theme.overlay_block_bg` |
| Warning `⚠` | Yellow |
| Error `✗` | Red |
| Help bar | Dim |
| Status bar | Dim gray, right-aligned |
| Scope badge `[Global]` | Bold cyan |

### 3.4 Row Format

```
{check_glyph}  {emoji} {display_name:<20}  {kind:<12}  {version:<8}  ~{tokens} tokens
```

Example:
```
[x]  🧰 my-tools             tool-server   v1.2.0    ~220 tokens
[ ]  📚 my-skills            skill         v0.3.0    ~80 tokens
[x]  🔧 deploy-hooks         script        v1.0.0    ~0 tokens
```

### 3.5 Live Status Bar (token estimation)

```rust
fn plugin_toggle_status_line(entries: &[PluginToggleEntry]) -> String {
    let total: usize = entries
        .iter()
        .filter(|e| e.check_state == PluginCheckState::On)
        .map(|e| e.estimated_tokens)
        .sum();

    let token_str = if total >= 1_000 {
        format!("~{:.1}k tokens", total as f64 / 1_000.0)
    } else {
        format!("~{total} tokens")
    };

    format!("Est. plugin context: {token_str}")
}
```

This function is called **every render frame** and appended (right-aligned) to the status bar. It is the Rust equivalent of hermes-agent's `tools_config.py::status_fn`.

### 3.6 Token Estimation Per Plugin Kind

| Kind | Estimation Strategy |
|---|---|
| `SkillPlugin` | `SKILL.md` body length in bytes ÷ 4 (conservative chars-to-tokens heuristic) |
| `ToolServerPlugin` | Sum `tool_tokens(tool_schema_json)` for each tool in `plugin.toml[tools]` |
| `ScriptPlugin` | 0 (hooks inject no LLM context; they run outside the LLM loop) |

The token estimation follows the same algorithm as `tools_config.py`:

```rust
/// Approximate token count for a JSON-serialized tool schema.
fn estimate_tool_tokens(schema_json: &str) -> usize {
    // ~4 chars per token for dense JSON
    schema_json.len() / 4
}
```

---

## 4. Activation Flows

### 4.1 `/plugins toggle` — Direct Entry Point

```
/plugins toggle [<name>] [--platform <platform>]
```

- No args → open interactive TUI (scope = Global)
- `<name>` → toggle that specific plugin non-interactively, persist, print result
- `--platform <p>` → scope the toggle to a specific platform

**Command registration:** `crates/edgecrab-cli/src/commands.rs`, `COMMAND_REGISTRY` (see [010_cli_commands] §2 for the full `CommandDef` entry).  
**Full TUI spec:** [010_cli_commands] §3.9b for command surface; this document (§3, §5, §6) for overlay internals.

**TUI activation path** (mirrors `open_tool_manager` from `crates/edgecrab-cli/src/app.rs` ~10088):

```rust
// In AppState.handle_command():
CommandResult::ShowPluginToggle { platform } => {
    let scope = platform
        .map(PluginScope::Platform)
        .unwrap_or(PluginScope::Global);
    self.open_plugin_toggle(scope);
}
```

### 4.2 Scope Cycling (Tab key)

Available scopes, in cycle order:

```
Global → cli → telegram → discord → slack → whatsapp → signal
→ email → homeassistant → mattermost → matrix → dingtalk → (back to Global)
```

Only platforms that are actually enabled in `gateway.platforms` are included in the cycle. This prevents the user from accidentally creating orphan platform-specific configs for platforms they don't use.

**Implementation:**

```rust
fn cycle_plugin_toggle_scope(&mut self) {
    let enabled_platforms = self.config.enabled_platform_names();
    self.plugin_toggle_scope = next_scope(
        &self.plugin_toggle_scope,
        &enabled_platforms,
    );
    self.refresh_plugin_toggle_entries();
    self.plugin_toggle_status_note = Some(
        format!("Scope: {}", self.plugin_toggle_scope.label())
    );
}
```

### 4.3 ENTER Confirm — Persist to Config

When the user presses Enter:

1. Collect all entries where `check_state` changed from the initial snapshot.
2. Separate into `newly_enabled` and `newly_disabled` sets.
3. For each `newly_enabled` where `needs_credentials && !credentials_satisfied`:
   - **Block** the confirm, instead open the **Credential Wizard** (see §5).
   - After wizard completion (or skip), resume the confirm.
4. Call `persist_plugin_toggle_to_config(scope, disabled_names)`.
5. Close overlay.
6. Print: `"✓ Saved: {n} enabled, {m} disabled ({scope_label})."`

```rust
fn persist_plugin_toggle_to_config(
    config: &mut AppConfig,
    scope: &PluginScope,
    disabled_names: &[String],
) -> anyhow::Result<()> {
    // Per spec 011_config_schema.md §3.1:
    //   plugins.disabled: [name-a, name-b]              (global)
    //   plugins.platform_disabled.telegram: [name-c]    (per-platform)
    match scope {
        PluginScope::Global => config.plugins.disabled = disabled_names.to_vec(),
        PluginScope::Platform(p) => {
            config.plugins
                .platform_disabled
                .entry(p.clone())
                .and_modify(|v| *v = disabled_names.to_vec())
                .or_insert_with(|| disabled_names.to_vec());
        }
    }
    config.save()
}
```

> **DRY note:** The config fields `plugins.disabled` and `plugins.platform_disabled` are already specified in [011_config_schema] §3. This function is the **only** place in the codebase that writes plugin disable state. All readers call `config.is_plugin_enabled(name, platform)` (to be added in `crates/edgecrab-core/src/config.rs`). Pattern reference: `persist_tool_filters_to_config()` in `crates/edgecrab-cli/src/app.rs` (~line 998).

### 4.4 ESC Cancel — No Persistence

When the user presses Esc:
1. Discard the in-flight toggle diff (restore `check_state` values from pre-open snapshot).
2. Close overlay without writing config.
3. Print nothing (silent cancel, same as tool_manager).

---

## 5. Credential Wizard

### 5.1 When It Triggers

After confirming (ENTER), for each **newly-enabled** `ToolServerPlugin` where:
- `needs_credentials == true` AND
- `credentials_satisfied == false`

The wizard runs in sequence (one plugin at a time) before persisting.

### 5.2 Wizard Flow

```
┌─────────────────────────────────────────────────────────────────┐
│ Configure: my-tools                                             │
│ This plugin requires the following environment variables:       │
│                                                                 │
│  DEPLOY_TOKEN         (required)                               │
│  Paste your deploy token from https://example.com/tokens        │
│  > ••••••••••••••••••                                           │  ← masked input
│                                                                 │
│  Enter to confirm · Esc to skip this plugin                     │
└─────────────────────────────────────────────────────────────────┘
```

**Implementation:** Reuse the existing `CredentialPromptState` in `crates/edgecrab-cli/src/app.rs` (search for `CredentialPromptState` / `SecretInput` overlay — this overlay is already used by the MCP install flow in `mcp_support.rs`). Do NOT create a new masked-input widget.

```rust
// Already in crates/edgecrab-cli/src/app.rs (CredentialPromptState ~line 1492):
struct CredentialPromptState {
    label: String,
    prompt: String,
    value: String,
    masked: bool,
}
```

The plugin wizard pushes items from `plugin.required_environment_variables` into this existing overlay, one at a time. Each confirmed value is written to `~/.edgecrab/.env` via the existing `write_env_var()` helper.

If the user presses Esc during the wizard:
- That plugin is kept in the "enabled" set (the toggle change is preserved)
- The wizard moves to the next plugin that needs credentials
- A status note is shown: `"⚠ my-tools: credentials skipped — plugin enabled but may not work"`

### 5.3 Provider Picker (ToolServerPlugin with multiple backends)

Some ToolServer plugins support multiple backend providers (e.g., a "web search" plugin might support Tavily, Exa, or DuckDuckGo). When a plugin manifest includes a `providers:` list:

```toml
# plugin.toml
[[providers]]
id   = "tavily"
env  = ["TAVILY_API_KEY"]
url  = "https://app.tavily.com/home"

[[providers]]
id   = "exa"
env  = ["EXA_API_KEY"]
url  = "https://exa.ai"
```

The wizard shows a provider picker **before** the env-var prompts:

```
┌──────────────────────────────────────────────────────────┐
│ Configure: web-search-plugin                             │
│ Choose a provider:                                       │
│                                                          │
│ ▶ Tavily          (TAVILY_API_KEY)                       │
│   Exa             (EXA_API_KEY)                          │
│   DuckDuckGo      (no key required)                      │
│                                                          │
│ ↑↓ navigate  Enter select  Esc skip                      │
└──────────────────────────────────────────────────────────┘
```

**Implementation:** Reuse `FuzzySelector<ProviderEntry>` — the same pattern used by the MCP install wizard in `crates/edgecrab-cli/src/mcp_support.rs`. Provider list comes from `plugin.toml` `[[providers]]` entries (see [003_manifest] §4 for the `providers` array schema).

---

## 6. Platform-Scoped Toggle Logic

### 6.1 Effective Check State Calculation

When building entries for a given scope:

```rust
fn effective_check_state(
    name: &str,
    scope: &PluginScope,
    config: &AppConfig,
) -> PluginCheckState {
    match scope {
        PluginScope::Platform(p) => {
            // Platform-specific override takes strict precedence
            if let Some(platform_disabled) = config.plugins.platform_disabled.get(p) {
                if platform_disabled.contains(name) {
                    return PluginCheckState::Off;
                }
            }
            // Fall through to global
            if config.plugins.disabled.contains(name) {
                PluginCheckState::Off
            } else {
                PluginCheckState::On
            }
        }
        PluginScope::Global => {
            if config.plugins.disabled.contains(name) {
                PluginCheckState::Off
            } else {
                PluginCheckState::On
            }
        }
    }
}
```

**WHY this exact algorithm?**  
It mirrors `hermes-agent/hermes_cli/skills_config.py` `_is_skill_disabled(name, platform, config)` (search: `platform_disabled` in that file):
- Platform-specific `platform_disabled` takes strict precedence over global `disabled`.
- No "inherit from global" fallback in `platform_disabled` — the lists are disjoint per scope.

### 6.2 Scope Badge in Header

```
PLUGIN TOGGLE  [Global]      ← Global scope
PLUGIN TOGGLE  [telegram]    ← Platform scope: telegram
PLUGIN TOGGLE  [cli]         ← Platform scope: cli
```

When in a platform scope: the header also shows global state as dimmed annotation:

```
PLUGIN TOGGLE  [telegram]   (5 globally enabled → 3 telegram-visible)
```

---

## 7. Fallback: Non-TTY / No ratatui Mode

When the overlay is requested in a non-interactive context (e.g., `stdout` is not a TTY, or `EDGECRAB_NO_TUI=1` is set):

```
Plugins (Global scope):

  1. [x] my-tools       tool-server  v1.2.0
  2. [ ] my-skills      skill        v0.3.0
  3. [x] deploy-hooks  script       v1.0.0

Type numbers to toggle (e.g. 1,2), empty to keep, q to cancel:
> _
```

This is the Rust equivalent of hermes-agent's `_numbered_fallback()` in `curses_ui.py`.

**Implementation:**

```rust
fn plugin_toggle_text_fallback(
    entries: &mut [PluginToggleEntry],
) -> Option<Vec<String>> {           // None = cancelled, Some = disabled names
    // Print numbered list
    // Read line from stdin
    // Parse comma-separated indices
    // Toggle those entries
    // Return disabled names or None
}
```

---

## 8. Command Registration Updates

The following entries are added/updated in `commands.rs` `COMMAND_REGISTRY`:

```rust
// UPDATE existing /plugins entry:
CommandDef {
    name: "plugins",
    aliases: &["plugin"],
    description: "Manage plugins (list, install, remove, toggle)",
    category: "Tools & Skills",
    args_hint: "[subcommand|name]",
}

// ADD:
CommandDef {
    name: "plugins toggle",
    aliases: &["plugins enable", "plugins disable"],
    description: "Interactive plugin toggle (enable/disable per plugin or platform)",
    category: "Tools & Skills",
    args_hint: "[<name>] [--platform <p>]",
}
```

---

## 9. Remote Search Integration

The local plugin toggle and the remote plugin browser are separate overlays with a
shared navigation model.

Rules:

1. Pressing `R` inside the local plugin toggle MUST open the remote plugin browser.
2. Pressing `L` inside the remote plugin browser MUST return to the local plugin toggle
   at the current scope.
3. The two overlays MUST reuse shared browser infrastructure where possible. In the
   current implementation this is a generic `RemoteBrowserState<T>` used by remote
   skills, remote plugins, and remote MCP discovery, keeping the TUI DRY.
4. Remote install and update actions triggered from the browser MUST flow through the
   same install/update backend used by `/plugins install` and `/plugins update`.

**Handler addition in `commands.rs`:**

```rust
"toggle" | "enable" | "disable" => {
    let name_arg = sub_args.next().map(str::to_string);
    let platform = if sub_args.as_str().contains("--platform") {
        sub_args.skip_while(|a| *a != "--platform")
                .nth(1)
                .map(str::to_string)
    } else {
        None
    };
    CommandResult::ShowPluginToggle { name: name_arg, platform }
}
```

**CommandResult addition:**

```rust
// In commands.rs CommandResult enum:
ShowPluginToggle {
    name: Option<String>,
    platform: Option<String>,
},
```

---

## 9. Render Function Specification

**Location:** `crates/edgecrab-cli/src/app.rs` `render_plugin_toggle()` — modelled after `render_tool_manager()` (~line 18254 in `app.rs`). Registered at the bottom of the render dispatch, alongside `if self.tool_manager.active { self.render_tool_manager(frame, frame.area()); }`.

```rust
fn render_plugin_toggle(&self, frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);

    // 1. Vertical layout: header(3) | search(3) | list(fill) | detail(6) | help(2)
    let chunks = Layout::vertical([
        Constraint::Length(3),   // header + scope badge
        Constraint::Length(3),   // search bar
        Constraint::Fill(1),     // scrollable list
        Constraint::Length(6),   // detail pane
        Constraint::Length(2),   // help bar + status line
    ]).split(area);

    // 2. Header
    self.render_plugin_toggle_header(frame, chunks[0]);

    // 3. Search bar
    self.render_plugin_toggle_search(frame, chunks[1]);

    // 4. List
    self.render_plugin_toggle_list(frame, chunks[2]);

    // 5. Detail pane
    self.render_plugin_toggle_detail(frame, chunks[3]);

    // 6. Help bar + live token status
    self.render_plugin_toggle_footer(frame, chunks[4]);
}
```

### 9.1 List Row Render

```rust
fn render_plugin_row(entry: &PluginToggleEntry, is_cursor: bool) -> Line<'_> {
    let glyph_style = if entry.check_state == PluginCheckState::On {
        Style::default().fg(Color::Green)
    } else {
        Style::default().dim()
    };

    let check = Span::styled(entry.check_state.glyph(), glyph_style);
    let name  = Span::styled(
        format!("  {} {:<22}", entry.emoji(), entry.display_name),
        if is_cursor { Style::default().bold().fg(Color::Green) }
        else         { Style::default() },
    );
    let kind  = Span::styled(format!("  {:<14}", entry.kind), Style::default().dim());
    let ver   = Span::styled(format!("  {:<9}", entry.version), Style::default().dim());
    let tok   = Span::styled(
        format!("  ~{} tokens", entry.estimated_tokens),
        Style::default().dim(),
    );

    Line::from(vec![check, name, kind, ver, tok])
}
```

### 9.2 Footer Render

```rust
fn render_plugin_toggle_footer(&self, frame: &mut Frame, area: Rect) {
    let help = Line::from(vec![
        Span::styled("↑↓ navigate  ", Style::default().dim()),
        Span::styled("SPACE toggle  ", Style::default().dim()),
        Span::styled("ENTER confirm  ", Style::default().dim()),
        Span::styled("Tab scope  ", Style::default().dim()),
        Span::styled("Esc cancel", Style::default().dim()),
    ]);

    let enabled_entries: Vec<_> = self.plugin_toggle.items.iter()
        .filter(|e| e.check_state == PluginCheckState::On)
        .collect();
    let status = plugin_toggle_status_line(&enabled_entries);
    let scope_label = self.plugin_toggle_scope.label();

    // Right-align status + scope
    let right = format!("{}    {}", status, scope_label);

    // Render help left, status right
    render_two_column_footer(frame, area, help, right);
}
```

---

## 10. Config Schema (Reference — DRY)

> **Do NOT duplicate config field definitions here.** See [011_config_schema] §3 for canonical definitions. Config persistence uses `crates/edgecrab-core/src/config.rs` `AppConfig::save()`.

**Summary of relevant keys** (for quick reference — definitions live in 011):

| Key | Type | Purpose |
|---|---|---|
| `plugins.disabled` | `Vec<String>` | Globally disabled plugin names |
| `plugins.platform_disabled.<platform>` | `Vec<String>` | Platform-scoped additional disabled names |

The plugin toggle overlay reads AND writes exactly these two keys via `AppConfig`. No other config key is touched by `toggle_plugin_selected()` or `confirm_plugin_toggle()`.

---

## 11. Integration with `/plugins list`

The existing `/plugins list` output is enhanced to show toggle state:

```
INSTALLED PLUGINS  (Global)
──────────────────────────────────────────────────────────────────────
my-tools        v1.2.0  [ENABLED]   tool-server   user    ~220 tokens
my-skills       v0.3.0  [DISABLED]  skill         user    ~80 tokens
deploy-hooks    v1.0.0  [ENABLED]   script        user    ~0 tokens
──────────────────────────────────────────────────────────────────────
Total context: ~220 tokens  (2 enabled, 1 disabled)

  Tip: /plugins toggle  →  interactive enable/disable
```

---

## 12. Acceptance Criteria

| ID | Criterion | Source |
|---|---|---|
| TUI-01 | Overlay opens with `FuzzySelector<PluginToggleEntry>`, not a new widget type | DRY/SOLID §2 |
| TUI-02 | Key bindings match `tool_manager` exactly (↑↓jk, SPACE, ENTER, Esc, Tab, /) | §3.1 |
| TUI-03 | Live token estimate updates every frame (no debounce — same as hermes status_fn) | §3.5 |
| TUI-04 | ENTER persists ONLY to `plugins.disabled` and `plugins.platform_disabled.*` | §4.3, [011] |
| TUI-05 | ESC discards all in-flight changes (no config write) | §4.4 |
| TUI-06 | Tab cycles scope through enabled platforms only | §4.2 |
| TUI-07 | Credential wizard reuses `CredentialPromptState`, NOT a new overlay | §5.2 |
| TUI-08 | Provider picker reuses `FuzzySelector<ProviderEntry>`, NOT a new overlay | §5.3 |
| TUI-09 | Non-TTY fallback renders numbered list and accepts comma-separated toggle | §7 |
| TUI-10 | `/plugins list` shows enabled/disabled state and total token estimate | §11 |
| TUI-11 | `PluginCheckState::glyph()` returns `[x]` / `[ ]` — same as `ToolManagerCheckState` | §2.2 |
| TUI-12 | Token estimation: SkillPlugin = `body_bytes / 4`, ToolServer = schema-sum, Script = 0 | §3.6 |
| TUI-13 | Platform-scope effective state: platform_disabled overrides global, no inheritance | §6.1 |
| TUI-14 | Overlay render follows 5-zone vertical layout (header/search/list/detail/footer) | §9 |
| TUI-15 | Scope badge `[Global]` / `[telegram]` always visible in header | §3.2, §6.2 |

---

## 13. Implementation Notes for Reviewers

### Do Use
- `FuzzySelector<PluginToggleEntry>` — `crates/edgecrab-cli/src/fuzzy_selector.rs`
- `FuzzyItem` trait — `crates/edgecrab-cli/src/fuzzy_selector.rs`
- `CredentialPromptState` — `crates/edgecrab-cli/src/app.rs` (~line 1492)
- `theme.overlay_selected`, `theme.overlay_title` — `crates/edgecrab-cli/src/theme.rs` (no hardcoded colors)
- `persist_tool_filters_to_config()` — `crates/edgecrab-cli/src/app.rs` (~line 998) as pattern reference
- `PluginManager::discover()` — `crates/edgecrab-cli/src/plugins.rs`
- `AppConfig::save()` — `crates/edgecrab-core/src/config.rs`

### Do NOT
- Create a new multi-select or checklist widget — `FuzzySelector<T>` with per-item check state IS the multi-select
- Duplicate config field definitions from [011_config_schema]
- Restart the agent when a plugin is toggled — reload happens on the next agent turn
- Add platform scopes for disabled platforms — only cycle through `config.enabled_platform_names()`

### Rust Crates (no new dependencies required)
- `ratatui` — existing (`Cargo.toml` workspace dep), all layout/render
- `crossterm` — existing, key event handling
- `serde_yml` / `toml` — existing, config persistence
- `anyhow` — existing, error handling

---

## 14. Reference: hermes-agent Source Mapping

All hermes-agent paths are relative to the repo root `hermes-agent/` (NousResearch/hermes-agent).

| hermes-agent source | Function / symbol | EdgeCrab equivalent | File |
|---|---|---|---|
| `hermes_cli/curses_ui.py` | `curses_checklist()` | `FuzzySelector<PluginToggleEntry>` + SPACE-toggle | `crates/edgecrab-cli/src/fuzzy_selector.rs` |
| `hermes_cli/curses_ui.py` | `_numbered_fallback()` | `plugin_toggle_text_fallback()` | `crates/edgecrab-cli/src/plugin_toggle.rs` |
| `hermes_cli/curses_ui.py` | `status_fn(chosen)` parameter | `plugin_toggle_status_line(&entries)` | `crates/edgecrab-cli/src/plugin_toggle.rs` |
| `hermes_cli/skills_config.py` | `_select_platform()` | `cycle_plugin_toggle_scope()` via Tab | `crates/edgecrab-cli/src/app.rs` |
| `hermes_cli/skills_config.py` | `save_disabled_skills()` | `persist_plugin_toggle_to_config()` | `crates/edgecrab-cli/src/app.rs` |
| `hermes_cli/skills_config.py` | `_is_skill_disabled()` | `effective_check_state()` | `crates/edgecrab-cli/src/plugin_toggle.rs` |
| `hermes_cli/tools_config.py` | `_configure_toolset()` | credential wizard via `CredentialPromptState` | `crates/edgecrab-cli/src/app.rs` |
| `hermes_cli/tools_config.py` | `_toolset_needs_configuration_prompt()` | `entry.needs_credentials && !entry.credentials_satisfied` | `crates/edgecrab-cli/src/plugin_toggle.rs` |
| `hermes_cli/tools_config.py` | `_prompt_toolset_checklist()` | `open_plugin_toggle()` | `crates/edgecrab-cli/src/app.rs` |
| `hermes_cli/tools_config.py` | `ToolManagerCheckState` (On/Off/Mixed) | `PluginCheckState` (On/Off) | `crates/edgecrab-cli/src/plugin_toggle.rs` |
| `config.yaml` key `skills.disabled` | — | `plugins.disabled` | `crates/edgecrab-core/src/config.rs` → [011_config_schema] §3 |
| `config.yaml` key `skills.platform_disabled.telegram` | — | `plugins.platform_disabled.telegram` | `crates/edgecrab-core/src/config.rs` → [011_config_schema] §3 |

---

## 15. Source Code Index

Quick reference table mapping every function described in this spec to its target file.

| Symbol | File | Status |
|---|---|---|
| `PluginToggleEntry` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `PluginCheckState` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `PluginScope` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `plugin_toggle_status_line()` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `estimate_plugin_tokens()` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `plugin_toggle_text_fallback()` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `effective_check_state()` | `crates/edgecrab-cli/src/plugin_toggle.rs` | NEW |
| `App.plugin_toggle` field | `crates/edgecrab-cli/src/app.rs` | ADD to existing struct |
| `App.plugin_toggle_scope` field | `crates/edgecrab-cli/src/app.rs` | ADD to existing struct |
| `App.plugin_toggle_status_note` field | `crates/edgecrab-cli/src/app.rs` | ADD to existing struct |
| `App::build_plugin_toggle_entries()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::open_plugin_toggle()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::toggle_plugin_selected()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::confirm_plugin_toggle()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::refresh_plugin_toggle_entries()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::cycle_plugin_toggle_scope()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::persist_plugin_toggle_to_config()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `App::render_plugin_toggle()` | `crates/edgecrab-cli/src/app.rs` | NEW method |
| `CommandResult::ShowPluginToggle` | `crates/edgecrab-cli/src/commands.rs` | ADD variant |
| `AppConfig::is_plugin_enabled()` | `crates/edgecrab-core/src/config.rs` | NEW method |
| `FuzzySelector<T>` | `crates/edgecrab-cli/src/fuzzy_selector.rs` | EXISTING — no change |
| `FuzzyItem` trait | `crates/edgecrab-cli/src/fuzzy_selector.rs` | EXISTING — no change |
| `CredentialPromptState` | `crates/edgecrab-cli/src/app.rs` | EXISTING — reused |
| `Theme.overlay_selected` | `crates/edgecrab-cli/src/theme.rs` | EXISTING — reused |
| `persist_tool_filters_to_config()` | `crates/edgecrab-cli/src/app.rs` | EXISTING — pattern ref only |
| `PluginManager::discover()` | `crates/edgecrab-cli/src/plugins.rs` | EXISTING — called by builder |
