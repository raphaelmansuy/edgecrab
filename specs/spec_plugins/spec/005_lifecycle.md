# Plugin Lifecycle — State Machine & Operations

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [003_manifest], [004_plugin_types], [006_security], [007_registry], [010_cli_commands]

---

## 1. Lifecycle State Machine

```
                      /plugins install <source>
                               │
                 ┌─────────────▼────────────┐
                 │         DOWNLOADING       │
                 │  fetch from hub / local   │
                 └─────────────┬────────────┘
                               │  fetch OK
                 ┌─────────────▼────────────┐
                 │         QUARANTINE        │
                 │  ~/ .edgecrab/           │
                 │  plugins/.hub/quarantine/ │
                 │  <name>-<version>/        │
                 └─────────────┬────────────┘
                               │  quarantine written
                 ┌─────────────▼────────────┐
                 │          SCANNING         │
                 │  PluginSecurityScanner    │
                 │  (static analysis)        │
                 └─────┬───────────┬────────┘
                       │           │
               scan OK │           │ scan BLOCKED (dangerous)
                       │           │
                       │           └────────────▶ REJECTED
                       │                        (quarantine deleted)
       ┌───────────────▼────────────┐
       │         APPROVED           │
       │  trust level = community   │  ◀── caution verdict blocks here
       │        / trusted           │       unless --force used
       └───────────────┬────────────┘
                       │  user confirms (or auto-approve if trusted)
       ┌───────────────▼────────────┐
       │         INSTALLED          │
       │  moved to                  │
       │  ~/.edgecrab/plugins/<n>/  │
       │  manifest written to DB    │
       └───┬───────────────────┬────┘
           │                   │
/plugins enable (default)    /plugins disable
           │                   │
 ┌─────────▼──────┐   ┌────────▼──────┐
 │    STARTING    │   │   DISABLED    │
 │  (spawn sub-   │   │ (no tools,    │
 │   process /    │   │  no prompt    │
 │  compile Rhai) │   │  injection)   │
 └─────────┬──────┘   └───────────────┘
           │
     ┌─────▼──────┐
     │   RUNNING  │◀────────────── restart (on crash)
     │            │
     └─────┬──────┘
           │
      crash │ or /plugins disable
     ┌──────▼──────┐
     │   FAILED    │
     │  (retry per │ ─── restart_policy ──▶ STARTING
     │   policy)   │
     └─────────────┘
           │
     /plugins uninstall
     ┌──────▼──────┐
     │  REMOVED    │
     │  (files     │
     │   deleted,  │
     │   DB row    │
     │   removed)  │
     └─────────────┘
```

---

## 2. State Definitions

| State | Description | Allowed Transitions |
|---|---|---|
| `Downloading` | Fetching from remote source | → Quarantine, → Rejected |
| `Quarantine` | Staged for security scan | → Scanning |
| `Scanning` | Security scanner running | → Approved, → Rejected |
| `Approved` | Scan passed; awaiting user confirm | → Installed, → Rejected |
| `Installed` | Copied to plugins dir; in registry DB | → Starting, → Disabled |
| `Starting` | Subprocess/script initializing | → Running, → Failed |
| `Running` | Fully operational | → Disabled, → Failed |
| `Disabled` | User-disabled; no tools exposed | → Starting |
| `Failed` | Hard error; restart policy exhausted | → Starting (if policy allows) |
| `Removed` | Uninstalled; terminal state | — |

---

## 3. Install Flow (Detailed)

### 3.1 From a Remote Source (Hub / GitHub)

```
/plugins install <source>
       │
       ├── parse source:
       │     - "github:owner/repo/path"       → GitHub raw download
       │     - "https://..."                  → direct URL
       │     - "hub:<source-id>/<plugin-name>" → hub index lookup
       │
       ├── fetch files into quarantine dir
       │     .edgecrab/plugins/.hub/quarantine/<name>-<uuid>/
       │
       ├── validate plugin.toml
       │     - parse TOML ✓
       │     - required fields present ✓
       │     - version is valid semver ✓
       │     - kind is known value ✓
       │     - trust.level NOT "trusted" / "builtin" in author file ✓
       │
       ├── run PluginSecurityScanner (see [006_security])
       │     - scan all text files for threat patterns
       │     - score findings → verdict: safe / caution / dangerous
       │
       ├── apply trust policy (source trust × verdict):
       │     trusted  × safe      → auto-approve (no prompt)
       │     trusted  × caution   → auto-approve + log warning
       │     trusted  × dangerous → BLOCKED
       │     community× safe      → ask user (y/N)
       │     community× caution   → BLOCKED (unless --force)
       │     community× dangerous → BLOCKED always
       │
       ├── (if approved) compute SHA-256 of quarantine dir tree
       │     write to manifest integrity.checksum
       │
       ├── move quarantine → ~/.edgecrab/plugins/<name>/
       │
       ├── record in plugin database (~/.edgecrab/plugins/.registry.db):
       │     INSERT INTO plugins (name, version, kind, state, installed_at, source, checksum)
       │
       └── if auto-enable:
             PluginRegistry::enable("<name>")
```

### 3.2 From Local Path

```
/plugins install /path/to/my-plugin/
       │
       ├── validate path is a directory with plugin.toml ✓
       ├── trust level = "community" (local paths are not pre-trusted)
       ├── scan with PluginSecurityScanner
       ├── (skip quarantine copy — use symlink OR copy depending on config)
       └── register → Starting → Running
```

### 3.3 From Agent (plugin_manage tool)

```
plugin_manage { action: "create", name: "...", kind: "script", ... }
       │
       ├── validate name, kind, rhai_code fields
       ├── write files to ~/.edgecrab/plugins/<name>/
       ├── trust level = "agent-created" (scanned with agent-created policy)
       ├── PluginSecurityScanner: caution → allow + warn, dangerous → ask user
       └── PluginRegistry::register_and_start("<name>")
```

---

## 4. Enable / Disable

### 4.1 Enable

```rust
// Effect depends on plugin kind:
//
// SkillPlugin:
//   - set state = Running
//   - invalidate_skills_cache() so next PromptBuilder call picks up injection
//   - NO subprocess spawn needed
//
// ToolServerPlugin:
//   - set state = Starting
//   - spawn subprocess → initialize handshake → tools/list
//   - set state = Running
//   - add tools to runtime tool table
//
// ScriptPlugin:
//   - set state = Starting
//   - compile Rhai AST once (cached)
//   - set state = Running
//   - add tools to runtime tool table
```

### 4.2 Disable

```rust
// Effect depends on plugin kind:
//
// SkillPlugin:
//   - set state = Disabled
//   - invalidate_skills_cache()
//
// ToolServerPlugin:
//   - remove tools from runtime tool table FIRST (atomic swap)
//   - send notifications/shutdown to subprocess
//   - wait 5s → SIGKILL if still alive
//   - set state = Disabled
//
// ScriptPlugin:
//   - remove tools from runtime tool table
//   - drop compiled AST (free memory)
//   - set state = Disabled
```

Key invariant: **tools are removed from dispatch table BEFORE signalling shutdown**
to prevent in-flight tool calls being dispatched to a dying plugin.

---

## 5. Uninstall

```
/plugins uninstall <name>
       │
       ├── if state == Running → Disable first
       ├── remove files: rm -rf ~/.edgecrab/plugins/<name>/
       ├── DELETE FROM plugins WHERE name = ?
       └── emit PluginEvent::Uninstalled { name }
```

Uninstall is IRREVERSIBLE. The plugin must be re-installed from source.

---

## 6. Upgrade

```
/plugins upgrade <name>
       │
       ├── fetch new version from original source
       ├── run through full install flow (quarantine → scan → approve)
       ├── if new version passes:
       │     - Disable old version
       │     - Replace files in-place
       │     - Re-enable at new version
       └── if upgrade fails:
             - Old version remains running
             - PluginError::UpgradeFailed emitted
```

Upgrade is atomic at the filesystem level: new files are staged in quarantine,
then moved into place only after all checks pass.

---

## 7. State Persistence

Plugin state is stored in a SQLite database at `~/.edgecrab/plugins/.registry.db`.

```sql
CREATE TABLE IF NOT EXISTS plugins (
    name            TEXT PRIMARY KEY,
    version         TEXT NOT NULL,
    kind            TEXT NOT NULL,      -- "skill" | "tool_server" | "script"
    state           TEXT NOT NULL,      -- "installed" | "disabled" | "failed"
    installed_at    INTEGER NOT NULL,   -- unix timestamp
    source          TEXT,               -- install source URL / path
    checksum        TEXT,               -- SHA-256 of installed files
    trust_level     TEXT NOT NULL DEFAULT 'community',
    auto_enable     INTEGER NOT NULL DEFAULT 1,
    fail_reason     TEXT,               -- last failure message if state=failed
    restart_count   INTEGER NOT NULL DEFAULT 0
);
```

On agent startup, `PluginRegistry::load_all()` reads this table and starts all
plugins where `state != 'disabled'` and `state != 'failed'`.

---

## 8. Crash Recovery

```
ToolServerPlugin subprocess dies unexpectedly
       │
       ├── reader task detects EOF on stdout pipe
       ├── all pending tool calls receive PluginError::ProcessDied
       │
       ├── check restart_policy:
       │     "never"  → set state = Failed, log error
       │     "once"   → if restart_count < 1 → increment + restart
       │                 if restart_count >= 1 → state = Failed
       │     "always" → if restart_count < restart_max_attempts → restart
       │                 else → state = Failed
       │
       └── on restart:
             - spawn new subprocess
             - re-initialize handshake
             - rebuild tool table (tools may differ!)
             - resume accepting calls
```

Plugin state = Failed is surfaced to the user via `/plugins status` output.
The agent continues running. Tools from the failed plugin return `ToolError::PluginCrashed`.

---

## 9. Events

```rust
pub enum PluginEvent {
    Installed { name: String, version: String, kind: PluginKind },
    Started   { name: String },
    Stopped   { name: String },
    Crashed   { name: String, reason: String, restart_count: u32 },
    Upgraded  { name: String, from_version: String, to_version: String },
    Removed   { name: String },
    ScanResult { name: String, verdict: Verdict, findings_count: usize },
}
```

Events are broadcast on a `tokio::sync::broadcast::Sender<PluginEvent>` so the
TUI, gateway, and logging subsystems can subscribe without tight coupling.
