# Plugin Manifest Format — `plugin.toml`

**Status:** PROPOSED  
**Version:** 0.1.0  
**Date:** 2026-04-09  
**Cross-refs:** [000_overview], [004_plugin_types], [005_lifecycle], [006_security], [011_config_schema]

---

## 1. Purpose

Every plugin MUST ship a `plugin.toml` at the root of its directory.  
This manifest is the single source of truth for:

- Plugin identity (name, version, author, license)
- Plugin kind (skill / tool-server / script)
- Required capabilities (host permissions)
- Exposed tools (for tool-server and script kinds)
- Security: allowed file paths, network allowlist
- Execution: command, startup timeout, call timeout
- Compatibility: minimum edgecrab version

The agent and installer read ONLY `plugin.toml`. Plugin code is never
executed during install or scan — only after the install policy approves it.

---

## 2. Directory Layout

```
~/.edgecrab/plugins/<plugin-name>/
    plugin.toml          ← REQUIRED: manifest (this spec)
    SKILL.md             ← REQUIRED for kind="skill"
    main.py / main.js /  ← REQUIRED for kind="tool-server"
    main.rhai            ← REQUIRED for kind="script"
    README.md            ← OPTIONAL
    assets/              ← OPTIONAL: static files
    templates/           ← OPTIONAL: prompt templates
    scripts/             ← OPTIONAL: helper scripts for tool-server kind
```

For skill-only plugins, the layout is identical to the existing `~/.edgecrab/skills/`
layout. A `plugin.toml` is added alongside the existing `SKILL.md`.

---

## 3. Full Schema (TOML)

```toml
# ─────────────────────────────────────────────────────────────────────────────
# REQUIRED: plugin identity
# ─────────────────────────────────────────────────────────────────────────────

[plugin]
# Unique plugin identifier. Filesystem-safe: [a-z0-9][a-z0-9._-]*
# REQUIRED. Max 64 chars.
name = "my-plugin"

# Semantic version following semver.org.
# REQUIRED.
version = "1.0.0"

# Brief human-readable description shown in /plugins list.
# REQUIRED. Max 256 chars.
description = "Does something useful for the agent."

# Plugin kind:
#   "skill"       – injects content into system prompt (no code run)
#   "tool-server" – subprocess exposing tools via JSON-RPC 2.0 stdio
#   "script"      – embedded Rhai script evaluated in-process
# REQUIRED.
kind = "tool-server"

# Human name of the author / organization.
# OPTIONAL. Max 128 chars.
author = "Alice Smith"

# SPDX license identifier. E.g. "MIT", "Apache-2.0", "GPL-3.0-only"
# OPTIONAL.
license = "MIT"

# Homepage / documentation URL.
# OPTIONAL. Must be HTTPS.
homepage = "https://example.com/my-plugin"

# Minimum edgecrab version required to run this plugin.
# OPTIONAL. Compared with semver: plugin is rejected if
# running edgecrab version < min_edgecrab_version.
min_edgecrab_version = "0.1.4"


# ─────────────────────────────────────────────────────────────────────────────
# Tool-server kind: execution config
# REQUIRED only when kind = "tool-server"
# ─────────────────────────────────────────────────────────────────────────────

[exec]
# Command to spawn the plugin subprocess.
# Relative paths are resolved from the plugin directory.
# REQUIRED for kind = "tool-server".
command = "python3"
args = ["main.py"]
# OR: command = "node", args = ["dist/index.js"]
# OR: command = "./my-plugin" (compiled binary)

# Working directory for the subprocess.
# OPTIONAL. Defaults to the plugin directory.
# Must not escape ~/.edgecrab/plugins/<name>/ (validated).
cwd = "."

# Additional environment variables set in the subprocess.
# OPTIONAL. Values are plain strings.
# Secrets MUST use [capabilities] + host/secret_get, not hardcoded here.
[exec.env]
MY_VAR = "some-value"
LOG_LEVEL = "info"

# How many seconds to wait for the initialize handshake to complete.
# OPTIONAL. Default: 10. Range: 1–60.
startup_timeout_secs = 10

# Per-tool-call timeout in seconds.
# OPTIONAL. Default: 60. Range: 1–300.
call_timeout_secs = 60

# Restart policy when plugin subprocess dies unexpectedly.
#   "never"    – ToolError::PluginCrashed is returned immediately
#   "once"     – restart once; on second crash, return ToolError::PluginCrashed
#   "always"   – restart up to restart_max_attempts times (default 3)
# OPTIONAL. Default: "once".
restart_policy = "once"
restart_max_attempts = 3


# ─────────────────────────────────────────────────────────────────────────────
# Script kind: Rhai source
# REQUIRED only when kind = "script"
# ─────────────────────────────────────────────────────────────────────────────

[script]
# Path to the Rhai script file, relative to plugin directory.
# REQUIRED for kind = "script".
file = "main.rhai"

# Max Rhai AST operations before the script is interrupted.
# Prevents infinite loops. OPTIONAL. Default: 100_000.
max_operations = 100_000

# Max Rhai call stack depth. OPTIONAL. Default: 50.
max_call_depth = 50


# ─────────────────────────────────────────────────────────────────────────────
# REQUIRED for kind = "tool-server" or "script"
# OPTIONAL for kind = "skill" (no tools exposed)
# ─────────────────────────────────────────────────────────────────────────────

# [[tools]] declares the tools this plugin exposes.
# The runtime cross-checks this list against what tools/list returns.
# Tools not in this list but returned by tools/list are IGNORED.
# Tools in this list but not in tools/list = install warning.
[[tools]]
name = "my_tool"
description = "Does X given Y."

[[tools]]
name = "my_other_tool"
description = "Does something else."


# ─────────────────────────────────────────────────────────────────────────────
# Capabilities — permissions the plugin needs from the host.
# Principle of least privilege: request only what you need.
# Missing capabilities = ToolError::PermissionDenied when called.
# ─────────────────────────────────────────────────────────────────────────────

[capabilities]
# Subset of host API functions the plugin may call.
# Full catalog in [008_host_api.md].
#
# "host:memory_read"    – read agent MEMORY.md / USER.md
# "host:memory_write"   – append to agent memory files
# "host:secret_get"     – read a named secret from agent keychain
# "host:inject_message" – inject a message into conversation history
# "host:session_search" – search session history
# "host:tool_call"      – call another registered host tool
#
host = ["host:memory_read", "host:memory_write", "host:secret_get"]

# Outbound HTTP network access.
# Empty = no outbound network allowed (enforced by SSRF guard).
# Wildcards are NOT supported (each host must be listed explicitly).
allowed_hosts = [
    "api.github.com",
    "hacker-news.firebaseio.com",
]

# File system access roots (read+write).
# Paths outside these roots are blocked by path safety check.
# Variables supported: $EDGECRAB_HOME, $HOME, $CWD
# OPTIONAL. Default: [] (no FS access).
allowed_paths = ["$CWD"]

# Additional named toolsets from the host tool registry that this plugin
# requires to be enabled when it runs. The registry checks these are present.
# OPTIONAL.
required_host_toolsets = []


# ─────────────────────────────────────────────────────────────────────────────
# Trust level — propagated from the install source.
# This field is SET BY THE INSTALLER, not by the plugin author.
# Attempts by plugin.toml to self-assign "trusted" or "builtin" are rejected.
# OPTIONAL in the author's file; override by installer into installed copy.
# ─────────────────────────────────────────────────────────────────────────────

[trust]
# "community" | "trusted" | "builtin"
# Default: "community"
level = "community"

# Source URL or repo path from which this plugin was installed.
# Set by the installer from discovery metadata.
source = "https://github.com/raphaelmansuy/edgecrab/tree/main/plugins/my-plugin"


# ─────────────────────────────────────────────────────────────────────────────
# Integrity — set by the installer after download, checked on load.
# ─────────────────────────────────────────────────────────────────────────────

[integrity]
# SHA-256 of the entire plugin directory tree (sorted, canonical).
# Computed at install time; verified on every load.
# Set by installer. Absent = integrity check skipped (local dev mode).
checksum = "sha256:abc123..."
```

---

## 4. Validation Rules

### 4.1 Name Validation

```
RULE: name MUST match /^[a-z0-9][a-z0-9._-]*$/
RULE: name MAX 64 characters
RULE: name MUST be unique within the plugin registry
      (collision = PluginError::NameConflict)
RULE: name MUST NOT be the same as any compile-time inventory! tool name
      (inventory tools take priority, plugin would be silently shadowed → ERROR)
```

### 4.2 Version Validation

```
RULE: version MUST be valid semver (major.minor.patch[-prerelease][+build])
RULE: plugin upgrade is allowed only when new version > installed version
RULE: downgrade is allowed only with --force flag
```

### 4.3 Capability Validation

```
RULE: capabilities MUST be a subset of the catalog in [008_host_api.md]
RULE: unknown capability names → install warning (not error)
RULE: allowed_hosts MUST be valid hostnames (no IPs, no URLs with paths)
RULE: allowed_paths MUST not contain `..` components after variable expansion
```

### 4.4 Tool Name Validation

```
RULE: tool name MUST match /^[a-z][a-z0-9_]*$/
RULE: tool name MAX 64 characters
RULE: tool name MUST NOT collide with compile-time inventory! tool names
      (INV-4: inventory tools win; plugin tool with same name = install error)
```

### 4.5 Trust Level Validation

```
RULE: plugin.toml authors CANNOT set trust.level = "trusted" or "builtin"
      → installer rewrites this field based on source trust
RULE: trust.level MUST be one of: "community" | "trusted" | "builtin"
```

---

## 5. Minimal Examples

### 5.1 Skill Plugin (simplest)

```toml
[plugin]
name      = "rust-patterns"
version   = "1.0.0"
description = "Injects common Rust design patterns into the system prompt."
kind      = "skill"
author    = "Community"
license   = "MIT"
```

No `[exec]`, no `[capabilities]`, no `[[tools]]`. Just a SKILL.md + plugin.toml.

### 5.2 Tool-Server Plugin (Python)

```toml
[plugin]
name      = "github-tools"
version   = "2.1.0"
description = "Create/query GitHub issues and PRs."
kind      = "tool-server"
author    = "raphaelmansuy"
license   = "Apache-2.0"
min_edgecrab_version = "0.1.4"

[exec]
command = "python3"
args    = ["main.py"]
call_timeout_secs = 30
restart_policy = "once"

[[tools]]
name        = "create_github_issue"
description = "Create a GitHub issue in the given repo."

[[tools]]
name        = "list_github_issues"
description = "List open issues for a repo."

[capabilities]
host         = ["host:secret_get"]
allowed_hosts = ["api.github.com"]
```

### 5.3 Script Plugin (Rhai)

```toml
[plugin]
name      = "date-formatter"
version   = "1.0.0"
description = "Format dates in various locales without a subprocess."
kind      = "script"
license   = "MIT"

[script]
file             = "main.rhai"
max_operations   = 50_000

[[tools]]
name        = "format_date"
description = "Format a date string into a requested locale format."

[capabilities]
host = []
```

---

## 6. Schema Version & Compatibility

The manifest format is versioned via the edgecrab binary version in `min_edgecrab_version`.
Future manifest fields are OPTIONAL and ignored by older runtimes unless they change behavior.

A `manifest_version` field may be added in Phase 2 to enable strict schema validation.

---

## 7. Hermes-Compatible SKILL.md (Skill-Only Path)

Skill plugins authored for **Hermes Agent** do not ship a `plugin.toml`. EdgeCrab MUST
accept a bare `SKILL.md` as a valid skill plugin with `kind = "skill"`.

The full SKILL.md field spec is in `015_hermes_compatibility.md §3`. This section lists
the fields that extend beyond what earlier EdgeCrab docs described.

### 7.1 Credential-Collection Fields

#### `required_environment_variables` (modern form)

```yaml
required_environment_variables:
  - name: GITHUB_TOKEN            # required; also accepted: env_var: GITHUB_TOKEN
    prompt: "GitHub personal access token"  # optional; shown to user
    help: "https://github.com/settings/tokens"  # optional URL/text
    required_for: "creating PRs"  # optional; appended as "required for X"
```

Both `name` and `env_var` are accepted as the key for the variable name (source:
`skills_tool.py _get_required_environment_variables()`):

```python
env_name = str(entry.get("name") or entry.get("env_var") or "").strip()
```

Env var names are validated with `^[A-Za-z_][A-Za-z0-9_]*$` (allows uppercase,
distinct from skill name regex which requires lowercase start).

EdgeCrab MUST parse this field and, when a listed variable is unset, offer the
interactive credential wizard before injecting the skill into the system prompt.

#### `prerequisites.env_vars` (legacy form — normalised at load time)

```yaml
prerequisites:
  env_vars: [GITHUB_TOKEN, OPENAI_API_KEY]
  commands: [gh, git]            # advisory only; missing commands log a warning
```

`prerequisites.env_vars` is normalised into `required_environment_variables` entries
with `prompt: "Enter value for {NAME}"` and empty `help`. The modern form always wins
if both are present.

#### `setup` block (interactive wizard)

```yaml
setup:
  help: |
    How to obtain the required token:
    1. Visit https://developer.1password.com/docs/service-accounts/
    2. Create a Service Account and copy the token.
  collect_secrets:
    - env_var: OP_SERVICE_ACCOUNT_TOKEN   # required; also accepted: env: OP_SERVICE_ACCOUNT_TOKEN
      prompt: "1Password Service Account Token"  # optional
      provider_url: "https://developer.1password.com/"  # optional URL; also accepted: url:
      secret: true                         # optional; default true (mask echo)
```

Both `provider_url` and `url` are accepted in `collect_secrets` entries (source:
`skills_tool.py _normalize_setup_metadata()`):

```python
provider_url = str(item.get("provider_url") or item.get("url") or "").strip()
```

The `setup.help` text is displayed before the first credential prompt. Each entry in
`collect_secrets` triggers a prompt when its `env_var` is unset. When `secret: true`
the input MUST be hidden (no echo to terminal).

**`setup.help` fallback:** If a `required_environment_variables` entry has no individual
`help`, `provider_url`, or `url`, the top-level `setup.help` string is used as the
fallback help text for that entry.

### 7.2 Platform Field

```yaml
platforms: [macos, linux]   # omit for cross-platform
```

Valid values: `macos`, `linux`, `windows`. Maps internally to `darwin`, `linux`, `win32`.
Skills restricted to a platform that doesn't match the running OS are not loaded.

### 7.3 Metadata Block

```yaml
metadata:
  hermes:
    tags: [GitHub, Issues]
    related_skills: [github-auth, github-pr-workflow]
    category: version-control
```

`tags` are used for hub search. `related_skills` are shown in `/plugins info` with a
note if any are not installed. Unknown metadata keys are silently preserved.

### 7.4 SKILL.md Body Requirement

Source: `skill_manager_tool.py _validate_frontmatter()`

```python
body = content[end_match.end() + 3:].strip()
if not body:
    return "SKILL.md must have content after the frontmatter (instructions, procedures, etc.)."
```

The SKILL.md body (the text after the closing `---`) MUST be non-empty after stripping
whitespace. A SKILL.md with only a frontmatter block and an empty body is REJECTED.
This is validated at install time (not at prompt-injection time).

### 7.5 SKILL.md → PluginManifest Mapping

When EdgeCrab infers a `plugin.toml`-equivalent from a SKILL.md it applies:

```
SKILL.md field                   →  plugin.toml equivalent
-------------------------------     ------------------------------
name                             →  [plugin].name
description                      →  [plugin].description
version                          →  [plugin].version
author                           →  [plugin].author
license                          →  [plugin].license
platforms                        →  (runtime filter, no TOML equiv)
required_environment_variables   →  (wizard only, no TOML equiv)
setup.collect_secrets            →  (wizard only, no TOML equiv)
metadata.hermes.tags             →  (hub search index)
metadata.hermes.related_skills   →  (display in /plugins info)
(no tools, no exec, no script)   →  kind = "skill"
```

The inferred manifest is NOT written to disk. It exists only in memory during the
load pipeline.
