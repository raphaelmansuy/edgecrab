# Plugins & Skills — Architecture, Relation, and Implementation Guide

**Status:** PROPOSED  
**Version:** 0.2.0  
**Date:** 2026-04-09  
**Authors:** Engineering Team  
**Cross-refs:** [000_overview], [003_manifest], [004_plugin_types], [005_lifecycle],
               [006_security], [009_discovery_hub], [015_hermes_compatibility]

---

## 1. Executive Summary

EdgeCrab has **two overlapping but distinct concepts**:

| Concept    | Where defined                | What it does                                   |
|------------|------------------------------|------------------------------------------------|
| **Standalone skill**  | `~/.edgecrab/skills/<name>/SKILL.md` | Injects curated text into the system prompt    |
| **Plugin** | `plugin.toml` or Hermes `plugin.yaml` + optional `SKILL.md` | Installable runtime bundle: may inject text AND/OR add tools |

Standalone skills and plugins are **adjacent concepts, not identical ones**.
Both may contain `SKILL.md`, but only plugins participate in plugin discovery,
trust, quarantine, enable/disable, and runtime registration.

A plugin can still include a bundled skill:

- plugin-managed bundled skill: prompt guidance that ships with a plugin bundle
- standalone skill: prompt guidance managed independently under `~/.edgecrab/skills/`

The SKILL.md format remains Hermes-compatible on the read side, so Hermes-style
skill content can be consumed without rewriting the skill body.

The plugin system adds two new kinds that hermes-agent cannot express:

```
         hermes-agent                        edgecrab
         ─────────────                       ─────────────────────────────
         skill (SKILL.md only)    ────────►  StandaloneSkill
                                             Plugin(kind=skill, optional)
                                             ToolServerPlugin
                                             ScriptPlugin
                                             HermesPlugin
```

---

## 2. Conceptual Model

### 2.1 Containment hierarchy

```
EdgeCrab runtime
├── Standalone skill
│     └── reads SKILL.md
│           ├── YAML frontmatter   (metadata, filters, linked files)
│           └── Markdown body      (prompt guidance)
│
└── Plugin
├── kind = "skill"        →  SkillPlugin
│     └── optional bundled SKILL.md
│
├── kind = "tool-server"  →  ToolServerPlugin
│     ├── optionally reads SKILL.md  (adds context to describe the tools)
│     └── spawns subprocess (JSON-RPC 2.0 / MCP)
│           └── exposes N ToolHandler entries to the runtime registry
│
└── kind = "script"       →  ScriptPlugin
      ├── optionally reads SKILL.md  (documents what the script does)
      └── evaluates Rhai script
            └── registers functions as ToolHandler entries
```

### 2.2 Data flow for a standalone skill

```
  Disk                   Skills loader            PromptBuilder          LLM
  ────                   ─────────────            ─────────────          ───
  ~/.edgecrab/
  skills/my-skill/
    SKILL.md    ──────► parse_frontmatter()
                        platform_matches() ──►  summary in prompt ────►  system
                        load linked files            │                   prompt
                        render bundle on view        │
                                                     │
                                              skill_view(name)
```

### 2.3 Data flow for a plugin-bundled skill

```
  Disk                   PluginRegistry           PromptBuilder          LLM
  ────                   ──────────────           ─────────────          ───
  ~/.edgecrab/
  plugins/my-plugin/
    plugin.toml ──────► plugin.kind = Skill|ToolServer|Script|Hermes
    SKILL.md    ──────► bundled plugin skill metadata / prompt text
```

### 2.4 Data flow for a ToolServerPlugin (no hermes-agent equivalent)

```
  Disk                   PluginRegistry          ToolRegistry         LLM call
  ────                   ──────────────          ────────────         ────────
  ~/.edgecrab/
  plugins/my-server/
    plugin.toml ──────► plugin.kind = ToolServer
    SKILL.md    ──────► (optional: add server descrp to prompt)
    server.py   ──────► spawn subprocess
                        negotiate JSON-RPC 2.0
                        tools/list ──────────►  register_dynamic()
                                                   tool_a
                                                   tool_b    ──────►  tool call
                                                                      JSON-RPC
                                                                      response
```

---

## 3. Skills, Bundles, and Plugins

### 3.1 Historical context

hermes-agent grew its "skill" system organically:

1. **v0.1** — SKILL.md injected as a raw string into the prompt. No frontmatter.
2. **v0.2** — YAML frontmatter added: `name`, `description`, `platforms`.
3. **v0.3** — `required_environment_variables` + `collect_secrets` for guided setup.
4. **v0.4** — Skills Hub (GitHub-backed discovery), security scanning, tap registry.
5. **Future** — skills cannot grow further without executable capability.

EdgeCrab's plugin system is the clean-room design of what hermes-agent would have
built if it started today — separating the manifest (`plugin.toml`) from the prompt
content (`SKILL.md`) and adding two new kinds for runtime tool registration.

### 3.2 The conceptual leap

```
hermes-agent view:              edgecrab view:

  skill = { metadata + body }     plugin = { manifest } + optional{ skill }
      ^                                          │
      │                                          ▼
      └── "everything lives in SKILL.md"    "plugin.toml owns lifecycle;
                                             SKILL.md owns prompt content"
```

This separation means:
- Standalone skills stay lightweight and prompt-focused.
- Plugins stay lifecycle-managed and runtime-capable.
- The SKILL.md format stays Hermes-compatible on the read side.
- ToolServer, Script, and Hermes plugins may *optionally* include a SKILL.md to
  give the LLM context about what the tools do.

### 3.3 Claude-style standalone skill bundles

Claude Code skill bundles fit the standalone-skill side of the model when they
only provide prompt guidance plus helper files.

Supported in EdgeCrab:

- bundled `SKILL.md` directories
- `${CLAUDE_SKILL_DIR}` substitution
- `${CLAUDE_SESSION_ID}` substitution
- `read_files` loading
- helper-file discovery from `references/`, `templates/`, `scripts/`, and `assets/`
- metadata parsing for `when_to_use`, `arguments`, `argument-hint`,
  `allowed-tools`, `user-invocable`, `disable-model-invocation`, `context`, and `shell`

Not currently implemented:

- automatic prompt-shell execution from skill markdown
- automatic forked skill-agent execution from Claude-specific metadata

---

## 4. SKILL.md — Complete Field Reference

The canonical source of truth is `hermes-agent/agent/skill_utils.py` and
`hermes-agent/tools/skills_tool.py`.

### 4.1 Frontmatter layout

```yaml
---
# ── Identity ────────────────────────────────────────────────────────────
name: "my-skill"
description: "One-line description shown in /skills list."
category: "coding"           # freeform; used for grouping only
version: "1.0.0"             # optional SemVer

# ── Context files ────────────────────────────────────────────────────────
read_files:                  # additional markdown files appended after body
  - references/cheatsheet.md
  - templates/starter.md

# ── Platform filter ─────────────────────────────────────────────────────
# Absent / empty = all platforms (default, backward-compat)
platforms: [macos, linux]    # "windows" also accepted

# ── Dependency hints ─────────────────────────────────────────────────────
related_skills:              # advisory only; not auto-installed
  - rust-patterns
  - cargo-workspace

# ── Guided setup (optional) ──────────────────────────────────────────────
setup:
  help: "Visit https://example.com/api to get your API key."

  required_environment_variables:
    - name: MY_API_KEY          # also accepted: env_var: MY_API_KEY
      description: "API key for the Example service."
      help: "https://example.com/api"     # optional; overrides setup.help

  collect_secrets:
    - env: MY_API_KEY
      prompt: "Paste your Example API key:"
      provider_url: "https://example.com/api"   # also accepted: url:
      mask: true
      provider_name: "Example Service"

# ── Remote-env flag (Hermes-specific, preserved for compat) ─────────────
interactive_setup_unsupported_on: [docker, singularity, modal, ssh, daytona]
---

## Main content

Put the text that will be injected into the system prompt here.

This field MUST NOT be empty. At least one non-whitespace character
is required after the closing `---` of the frontmatter.
```

### 4.2 Field validation rules (source-verified)

| Field | Validation | Source |
|-------|-----------|--------|
| `name` | `^[a-z0-9][a-z0-9._-]*$`, max 64 chars | `skill_manager_tool.py VALID_NAME_RE` |
| `description` | non-empty string, max 1024 chars | `skill_manager_tool.py MAX_DESCRIPTION_LENGTH` |
| `category` | same regex as `name` | `skill_manager_tool.py` |
| `version` | free string (SemVer recommended) | no regex currently |
| `platforms` | list of `"macos"`, `"linux"`, `"windows"` | `skill_utils.py PLATFORM_MAP` |
| `read_files` | paths relative to skill dir, no `..`, no absolute | `skill_manager_tool.py` |
| env var `name`/`env_var` | `^[A-Za-z_][A-Za-z0-9_]*$` | `skills_tool.py _ENV_VAR_NAME_RE` |
| body (after `---`) | MUST be non-empty after strip | `skill_manager_tool.py _validate_frontmatter` |
| `related_skills` | list of skill names matching `VALID_NAME_RE` | `skills_tool.py` |
| allowed extra subdirs | `references/`, `templates/`, `scripts/`, `assets/` | `skill_manager_tool.py ALLOWED_SUBDIRS` |
| max single file size | 1 MiB (1 048 576 bytes) | `skill_manager_tool.py MAX_SKILL_FILE_BYTES` |
| max total skill chars | 100 000 characters | `skill_manager_tool.py MAX_SKILL_CONTENT_CHARS` |
| max file count | 50 files | `skills_guard.py MAX_FILE_COUNT` |
| max total size | 1 MiB | `skills_guard.py MAX_TOTAL_SIZE_KB` |

### 4.3 Key alias pairs (both keys accepted)

```yaml
# In collect_secrets entries:
provider_url: "..."   # canonical
url: "..."            # accepted alias → identical behaviour

# In required_environment_variables entries:
name: MY_VAR         # canonical
env_var: MY_VAR      # accepted alias → identical behaviour
```

### 4.4 setup.help fallthrough

If a `required_environment_variables` entry has no `help`, `provider_url`, or `url`
of its own, the top-level `setup.help` string is used as the fallback:

```python
# skills_tool.py _get_required_environment_variables()
help_text = (
    entry.get("help")
    or entry.get("provider_url")
    or entry.get("url")
    or setup.get("help")       # ← last-resort fallback for ALL entries
)
```

---

## 5. Platform Matching — Exact Algorithm

Source: `hermes-agent/agent/skill_utils.py skill_matches_platform()`

```
Input: frontmatter dict
Output: bool (True = skill is compatible with current OS)

1. Read frontmatter["platforms"]
2. If missing or empty → return True  (compatible with everything)
3. Normalize each entry: lower, strip
4. Map to sys.platform prefix:
     "macos"   → "darwin"
     "linux"   → "linux"
     "windows" → "win32"
   (Unknown strings passed through as-is)
5. For each platform in list:
     if sys.platform.startswith(mapped) → return True
6. Return False
```

```
Examples
─────────
platforms: [macos]       on macOS  → True
platforms: [macos]       on Linux  → False
platforms: [macos, linux] on Linux → True
(absent)                 on any    → True
```

EdgeCrab Rust equivalent:

```rust
// edgecrab-plugins/src/kinds/skill.rs
fn platform_matches(platforms: &[String]) -> bool {
    if platforms.is_empty() { return true; }
    let current = std::env::consts::OS; // "macos", "linux", "windows"
    platforms.iter().any(|p| {
        let normalized = p.to_lowercase();
        let mapped = match normalized.as_str() {
            "macos" => "macos",
            "linux" => "linux",
            "windows" => "windows",
            other => other,
        };
        current.starts_with(mapped)
    })
}
```

---

## 6. Skill Readiness States

Source: `hermes-agent/tools/skills_tool.py SkillReadinessStatus`

```
SkillReadinessStatus
├── AVAILABLE     — all required_environment_variables are set and non-empty
├── SETUP_NEEDED  — one or more required env vars are missing
└── UNSUPPORTED   — interactive setup impossible (remote env backend)
                    AND vars are missing
```

### 6.1 Resolution logic

```
is_remote_env = backend in {docker, singularity, modal, ssh, daytona}

for each var in required_environment_variables:
    if os.getenv(var) is missing or empty:
        if is_remote_env:
            return UNSUPPORTED
        else:
            return SETUP_NEEDED

return AVAILABLE
```

### 6.2 EdgeCrab Rust mapping

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillReadiness {
    Available,
    SetupNeeded { missing_vars: Vec<String> },
    Unsupported { reason: String },
}
```

### 6.3 UX implications

| Status | Prompt injected? | User sees |
|--------|-----------------|-----------|
| `AVAILABLE` | YES | Normal operation |
| `SETUP_NEEDED` | NO | Setup wizard surfaced by agent |
| `UNSUPPORTED` | NO | Warning: "This skill requires interactive setup unavailable in this environment." |

---

## 7. Prompt Injection — End-to-End Flow

```
PluginRegistry::build_prompt_section()
  │
  ├─ for each enabled SkillPlugin:
  │    ├─ platform_matches(frontmatter) → skip if false
  │    ├─ check_readiness() → skip if SETUP_NEEDED / UNSUPPORTED
  │    ├─ read SKILL.md body
  │    ├─ for each path in read_files:
  │    │    append file content
  │    └─ append to skill_prompt_block
  │
  └─ return skill_prompt_block
                │
                ▼
PromptBuilder::build()
  ├─ identity
  ├─ platform hint
  ├─ date/time
  ├─ AGENTS.md / SOUL.md / context files
  ├─ memory
  ├─ skill_prompt_block  ◄── injected HERE
  └─ tool schemas
```

**Caching note:** The assembled system prompt (including skill content) is cached once
per session and MUST NOT be rebuilt mid-conversation.  Skill content changing on disk
during a session has no effect until the next session start.  This preserves Anthropic
prompt cache validity (exact same bytes → cache hit).

---

## 8. Configuration — Enable/Disable per Platform

Source: `hermes-agent/agent/skill_utils.py get_disabled_skill_names()`

`config.yaml` controls which skills are active:

```yaml
skills:
  # Globally disabled (all platforms)
  disabled:
    - my-noisy-skill
    - legacy-tool

  # Per-platform overrides (checked FIRST, falls through to global)
  platform_disabled:
    telegram:
      - heavy-skill       # disable only for Telegram
    cli: []               # nothing extra disabled for CLI
```

Platform resolution order:
1. Check `HERMES_PLATFORM` env var
2. Check `HERMES_SESSION_PLATFORM` env var
3. Fall back to global `skills.disabled` list

EdgeCrab equivalent (`config.yaml`):
```yaml
plugins:
  disabled:
    - my-noisy-skill
  platform_disabled:
    telegram:
      - heavy-skill
  external_dirs:          # additional search paths beyond ~/.edgecrab/plugins/
    - ~/my-shared-skills
    - /opt/company/skills
```

---

## 9. External Skill Directories

Source: `hermes-agent/agent/skill_utils.py get_external_skills_dirs()`

Skills/plugins may live outside the main `~/.edgecrab/plugins/` directory:

```yaml
# config.yaml
plugins:
  external_dirs:
    - ~/company/shared-skills     # ~ expanded
    - ${TEAM_SKILL_PATH}          # env vars expanded
    - /absolute/path/to/skills    # absolute paths ok
```

Rules (source-verified):
- Each path is `os.path.expanduser()` + `os.path.expandvars()` + `Path.resolve()`
- Only directories that exist at startup are included
- The main `~/.edgecrab/plugins/` dir is silently de-duplicated
- Duplicates are silently de-duplicated

---

## 10. Skills vs Plugins — Complete Comparison Table

| Dimension | hermes-agent **Skill** | edgecrab **SkillPlugin** | edgecrab **ToolServerPlugin** | edgecrab **ScriptPlugin** |
|-----------|------------------------|--------------------------|-------------------------------|---------------------------|
| Manifest | Entirely in SKILL.md frontmatter | `plugin.toml` + `SKILL.md` | `plugin.toml` (+ optional SKILL.md) | `plugin.toml` (+ optional SKILL.md) |
| Prompt injection | YES | YES | Optional | Optional |
| Adds tools | NO | NO | YES (via JSON-RPC) | YES (via Rhai) |
| Subprocess | NO | NO | YES (crash-isolated) | NO (in-process) |
| Security scan | YES (text) | YES (text) | YES (text + binary) | YES (text + script) |
| Platform filter | YES | YES | YES | YES |
| Setup / env vars | YES | YES | YES | YES |
| Hermes-compatible | ✅ IS the source | ✅ 100% | ❌ no equivalent | ❌ no equivalent |
| Agent-creatable | YES | YES | NO (security) | Limited (sandboxed) |
| Max content size | 100 KB chars | 100 KB chars | no content limit | 1 MiB script |
| Hub discovery | YES | YES | YES | YES |

---

## 11. Hermes Skill → EdgeCrab Plugin Migration

### 11.1 Zero-change migration (SkillPlugin)

A hermes-agent skill is drop-in compatible:

```
hermes-agent                          edgecrab
~/.hermes/skills/                     ~/.edgecrab/plugins/
    my-skill/                             my-skill/
        SKILL.md          ───────►            SKILL.md  (unchanged)
        references/       ───────►            references/ (unchanged)
```

EdgeCrab will auto-detect `SKILL.md` and synthesize a `plugin.toml` with `kind = "skill"`.
No manual migration step required.

### 11.2 Explicit `plugin.toml` (recommended for new plugins)

```toml
# plugin.toml
[plugin]
name        = "my-skill"
version     = "1.0.0"
kind        = "skill"
description = "Common Rust patterns for idiomatic code."

[plugin.skill]
skill_md = "SKILL.md"     # default; can be omitted
```

### 11.3 Path translation

| hermes-agent path | edgecrab equivalent |
|-------------------|---------------------|
| `~/.hermes/skills/` | `~/.edgecrab/plugins/` |
| `~/.hermes/skills/.hub/` | `~/.edgecrab/plugins/.hub/` |
| `~/.hermes/skills/.bundled_manifest` | `~/.edgecrab/plugins/.bundled_manifest` |
| `HERMES_HOME/.env` | `EDGECRAB_HOME/.env` |
| `skills.disabled` in config | `plugins.disabled` in config |
| `skills.external_dirs` | `plugins.external_dirs` |

### 11.4 Security pattern overrides

The hermes-agent security scanner contains a pattern that checks for `~/.hermes/.env`:

```python
# skills_guard.py
(r'\$HOME/\.hermes/\.env|\~/\.hermes/\.env',
 "hermes_env_access", "critical", "exfiltration", ...)
```

EdgeCrab MUST add a parallel pattern for `~/.edgecrab/.env`:

```rust
// edgecrab-plugins/src/security/patterns.rs
("edgecrab_env_access",
 r"(?:\$HOME|~)/\.edgecrab/\.env",
 Severity::Critical, Category::Exfiltration,
 "directly references EdgeCrab secrets file"),
```

Both patterns should be active in EdgeCrab (skills from hermes-agent repos may
reference either path).

---

## 12. Security Model — Skills vs Other Plugins

### 12.1 Threat scope by plugin kind

| Threat | SkillPlugin | ToolServerPlugin | ScriptPlugin |
|--------|------------|-----------------|--------------|
| Prompt injection (text) | YES — text scanned | YES — SKILL.md scanned | YES — script scanned |
| Exfiltration (runtime) | NO (no code runs) | YES — subprocess isolated | PARTIAL — Rhai sandbox |
| Supply chain | LOW | HIGH | MEDIUM |
| Path traversal | LOW | HIGH | MEDIUM |
| Persistence | LOW (text only) | HIGH | MEDIUM |

### 12.2 Trust levels and install policy

```
                            Verdict
Source         safe          caution       dangerous
────────────── ─────────     ─────────     ─────────
builtin        allow         allow         allow
trusted        allow         allow         block
community      allow         block         block
agent-created  allow         allow         ask → allow + warn
```

`ask` (for agent-created) means: allow the install but surface a warning to the user
about the concerning findings. It does NOT block.

### 12.3 All 10 threat categories

Skills (all text) are scanned across all 10 categories; ToolServer plugins additionally
trigger binary and executable checks.

| # | Category | Description |
|---|----------|-------------|
| 1 | `exfiltration` | Credential theft, env dumping, DNS exfil, context leaking |
| 2 | `injection` | Prompt injection, role hijacking, jailbreaks |
| 3 | `destructive` | `rm -rf`, `mkfs`, disk overwrite, truncation |
| 4 | `persistence` | Cron, shell RC mods, SSH backdoors, launchd, systemd |
| 5 | `network` | Reverse shells, tunnel services, bind-all |
| 6 | `obfuscation` | Base64 decode, eval, exec, dynamic import, chr-building |
| 7 | `execution` | subprocess, os.system, os.popen, child_process |
| 8 | `traversal` | `../..`, /etc/passwd, /proc, /dev/shm |
| 9 | `mining` | xmrig, stratum+tcp, mining indicators |
| 10 | `supply_chain` | curl-pipe-shell, unpinned pip/npm, git clone at runtime |

Additional categories present in the scanner:

| Category | Description |
|----------|-------------|
| `privilege_escalation` | sudo, setuid, NOPASSWD, suid bit, allowed-tools field |
| `credential_exposure` | Hardcoded API keys, embedded private keys |

---

## 13. Skills Bundled Sync

Source: `hermes-agent/tools/skills_sync.py`

EdgeCrab ships bundled skills (in `skills/` repo directory) and syncs them to the user's
`~/.edgecrab/plugins/` on first run and on `edgecrab update`.

### 13.1 Manifest format

```
~/.edgecrab/plugins/.bundled_manifest   (v2 format)

rust-patterns:a3f8bc2d14...     ← name:md5_of_bundled_at_sync_time
cargo-workspace:9e12dc7a...
```

### 13.2 Update logic

```
For each skill in bundled skills/:
  case NEW (not in manifest):
    copy to user dir
    record origin hash

  case EXISTING (in manifest):
    compute hash of user copy
    if user_hash == origin_hash:
      user hasn't customized → safe to overwrite with updated bundled
      update origin hash
    else:
      user has customized → SKIP (preserve user changes)

  case DELETED by user (in manifest, absent from user dir):
    do not re-copy (respect user deletion)

  case REMOVED from bundled (in manifest, gone from repo):
    remove from manifest (do not touch user dir)
```

### 13.3 EdgeCrab env var override

```
EDGECRAB_BUNDLED_PLUGINS=/path/to/dir   (overrides repo-relative detection)
```

---

## 14. Skills Hub — Discovery and Trust

### 14.1 Default tap registry

```python
DEFAULT_TAPS = [
    {"repo": "openai/skills",                  "path": "skills/"},  # trusted
    {"repo": "anthropics/skills",              "path": "skills/"},  # trusted
    {"repo": "VoltAgent/awesome-agent-skills", "path": "skills/"},  # community
    {"repo": "garrytan/gstack",               "path": ""},          # community
]
```

Trust level assignment:
- `TRUSTED_REPOS = {"openai/skills", "anthropics/skills"}` → `"trusted"`
- Everything else → `"community"`

### 14.2 GitHub Authentication (4 priority levels)

```
Priority 1: GITHUB_TOKEN or GH_TOKEN env var  (PAT method)
Priority 2: `gh auth token` CLI subprocess    (gh-cli method)
Priority 3: GitHub App JWT                    (github-app method)
             env: GITHUB_APP_ID, GITHUB_APP_PRIVATE_KEY_PATH,
                  GITHUB_APP_INSTALLATION_ID
             cached for 3500 seconds (tokens valid 1 hour)
Priority 4: Unauthenticated                   (60 req/hr, public repos)
```

### 14.3 Index caching

- Cache TTL: `INDEX_CACHE_TTL = 3600` seconds (1 hour)
- Location: `~/.edgecrab/plugins/.hub/index-cache/<repo_path>.json`
- Cache is keyed on `repo_name + path` with `_` replacing `/`

### 14.4 SkillSource interface

```rust
// edgecrab-plugins/src/hub/source.rs
pub trait SkillSource {
    fn source_id(&self) -> &str;
    fn trust_level_for(&self, identifier: &str) -> TrustLevel;
    fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta>;
    fn fetch(&self, identifier: &str) -> Option<SkillBundle>;
    fn inspect(&self, identifier: &str) -> Option<SkillMeta>; // metadata only, no download
}
```

`inspect()` is distinct from `fetch()` — it downloads only `SKILL.md` for preview,
not the entire skill bundle.  This enables fast browsing in `/plugins hub search`.

---

## 15. Implementation Checklist

Use this section to verify correctness during code review.

### 15.1 SkillPlugin must

- [ ] Parse `SKILL.md` using `parse_frontmatter()` (split on `\n---\n`)
- [ ] Validate `name` against `^[a-z0-9][a-z0-9._-]*$` (max 64 chars)
- [ ] Validate `description` max 1024 chars
- [ ] Validate env var names against `^[A-Za-z_][A-Za-z0-9_]*$`
- [ ] Accept both `provider_url` and `url` in `collect_secrets` entries
- [ ] Accept both `name` and `env_var` in `required_environment_variables` entries
- [ ] Use `setup.help` as fallback help text for all env var entries
- [ ] Return `Unsupported` when remote env backend and missing env vars
- [ ] Reject SKILL.md with empty body (after stripping whitespace)
- [ ] Apply platform filter: absent = all platforms; list = match any
- [ ] NOT inject content when status is `SetupNeeded` or `Unsupported`
- [ ] Respect `skills.disabled` / `plugins.disabled` in config
- [ ] De-duplicate external dirs, skip dirs matching main plugins dir

### 15.2 Security scanner must

- [ ] Cover all 10 standard threat categories
- [ ] Cover `privilege_escalation` and `credential_exposure` categories
- [ ] Check for invisible/zero-width Unicode characters
- [ ] Check for suspicious binary extensions (`.exe`, `.dll`, `.so`, ...)
- [ ] Flag `hermes_env_access` (`~/.hermes/.env`)
- [ ] Flag `edgecrab_env_access` (`~/.edgecrab/.env`)
- [ ] Apply `INSTALL_POLICY[source]` to determine allow/block/ask
- [ ] For `agent-created` + `ask`: allow install but surface findings to user
- [ ] Gracefully degrade when guard crate unavailable

---

*See also: [015_hermes_compatibility] for field-by-field Hermes↔EdgeCrab mapping and
[006_security] for the complete threat pattern catalogue.*
