# 015 — Hermes Agent Compatibility Layer

**Status:** Accepted  
**Disposition:** Mandatory — all EdgeCrab skill loading must satisfy every invariant in this document  
**Primary References:** `tools/skills_tool.py`, `tools/skills_hub.py`, `tools/skills_guard.py`, `tools/skill_manager_tool.py`, and `website/docs/guides/build-a-hermes-plugin.md` in hermes-agent  

---

## 1. Purpose & Scope

Any skill/plugin authored for **Hermes Agent** must run in EdgeCrab without modification to the skill's files. This document is the binding specification for the compatibility layer — the translation rules EdgeCrab applies at load time and run time so that:

- `SKILL.md` authored for Hermes parses identically in EdgeCrab.
- `plugin.yaml` + `__init__.py` Hermes directory plugins install and run without EdgeCrab-specific files.
- Directory structures used by Hermes are recognized by EdgeCrab's scanner.
- Trust policies map correctly between the two naming systems.
- Hub index, taps, and lock files produced by Hermes can be consumed by EdgeCrab.
- Credential-collection workflows defined in a Hermes skill's `setup:` block are honoured in EdgeCrab.
- Size limits, name validation, and security patterns are identical.

Where a decision in this document conflicts with an earlier EdgeCrab spec doc, **this document takes precedence**.

As of the current implementation, EdgeCrab has verified CLI-runtime compatibility for:

- official repo Hermes-format examples under `plugins/`, currently `calculator` and `json-toolbox`
- Hermes directory plugins from `plugin.yaml` + `__init__.py`
- pip entry-point plugins from the `hermes_agent.plugins` group
- top-level plugin CLI commands registered through `ctx.register_cli_command()`
- memory-provider CLI trees registered through `cli.py` `register_cli(subparser)`
- the full upstream Hermes `VALID_HOOKS` set in CLI sessions
- gateway session lifecycle parity for `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset`
- Hermes hub indexing for upstream `plugins/...` directories as installable results
- Hermes hub indexing for the official EdgeCrab repository `plugins/...` tree
- Hermes hub indexing for repo-root Hermes plugin repositories such as `42-evey/hermes-plugins`

Adjacent standalone-skill compatibility is also verified for Claude-style skill bundles:

- `${CLAUDE_SKILL_DIR}` and `${CLAUDE_SESSION_ID}` substitution
- `when_to_use` summary fallback
- helper-file discovery from `references/`, `templates/`, `scripts/`, and `assets/`
- rendering of Claude metadata fields in `skill_view`

These Claude-style bundle features are not Hermes plugin requirements. They are
documented here because users commonly compare Hermes skills and Claude skills
as adjacent prompt-bundle systems.

Explicit boundary: EdgeCrab does not currently auto-execute Claude inline
prompt-shell blocks or auto-fork a dedicated Claude skill sub-agent.

---

## 2. First Principles Analysis

```
QUESTION: Why is compatibility non-trivial?
-----------------------------------------
Hermes is Python; EdgeCrab is Rust.
Hermes has a single home (~/.hermes/); EdgeCrab has a different one (~/.edgecrab/).
Hermes trust-level names differ from EdgeCrab design names.
Hermes uses individual-file GitHub API downloads; EdgeCrab originally specced ZIP archives.
Hermes has extra SKILL.md fields (setup:, required_environment_variables:) unknown to EdgeCrab.

CONSTRAINT: "Hermes skills must work in EdgeCrab without modification."

IMPLICATION: EdgeCrab must be a strict superset of the Hermes skill contract —
*adding* capabilities (toml manifests, WASM, ToolServer), not restricting them.
The compatibility layer is a READ-SIDE shim; write-side (creating skills)
uses EdgeCrab's richer plugin.toml format, which is a superset of SKILL.md.
```

---

## 3. SKILL.md — Canonical Field Reference

This section is the single source of truth for every YAML frontmatter field EdgeCrab
must recognise in a `SKILL.md` file. Fields are listed in the order Hermes assigns them.

### 3.1 Required Fields

| Field | Type | Max Length | Regex | Notes |
|---|---|---|---|---|
| `name` | `string` | 64 chars | `^[a-z0-9][a-z0-9._-]*$` | Kebab-case, no uppercase |
| `description` | `string` | 1024 chars | — | Free prose; used as LLM injection text |

Failure to satisfy either required field MUST cause skill loading to fail with a
structured diagnostic:

```
SkillLoadError::MissingField { field: "name", skill_path: PathBuf }
```

### 3.2 Optional Identification Fields

| Field | Type | Example | Notes |
|---|---|---|---|
| `version` | `string` | `"1.1.0"` | Semver preferred; freeform tolerated |
| `author` | `string` | `"Hermes Agent"` | Display only |
| `license` | `string` | `"MIT"` | Display only |
| `compatibility` | `string` | `"Requires macOS 13+"` | Freeform; shown in `/plugins info` |

### 3.3 Platform Filtering

```yaml
platforms: [macos, linux, windows]   # any subset; absence = all platforms
```

EdgeCrab MUST apply the same normalisation map as Hermes:

```
+--------------------+----------------------+
| SKILL.md value     |  sys::OS identifier  |
+--------------------+----------------------+
| macos              |  darwin              |
| linux              |  linux               |
| windows            |  win32               |
+--------------------+----------------------+
```

Implementation (Rust):

```rust
fn platform_matches(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true; // no restriction
    }
    let os = std::env::consts::OS; // "macos" | "linux" | "windows" on rust
    let hermes_os = match os {
        "macos"   => "darwin",
        "linux"   => "linux",
        "windows" => "win32",
        _         => os,
    };
    platforms.iter().any(|p| {
        let normalized = match p.as_str() {
            "macos"   => "darwin",
            "linux"   => "linux",
            "windows" => "win32",
            other     => other,
        };
        normalized == hermes_os
    })
}
```

A skill whose `platforms:` list does not include the current OS MUST NOT be loaded.
It MUST appear in `plugins list --all` with status `[platform-excluded]`.

### 3.4 Legacy Prerequisites Block

```yaml
prerequisites:          # LEGACY — Hermes still honours this; EdgeCrab must too
  env_vars: [API_KEY]   # normalised into required_environment_variables at load time
  commands: [curl, jq]  # advisory; EdgeCrab logs missing commands but does not block load
```

**Normalisation algorithm** (mirrors `_get_required_environment_variables` in hermes `skills_tool.py`):

1. If `required_environment_variables:` is present, use it directly (modern form wins).
2. Else if `prerequisites.env_vars` is present, convert each string `VAR` to:
   ```yaml
   - name: VAR
     prompt: "Enter value for VAR"
     help: ""
   ```
3. Merge result into the unified `RequiredEnvVar` list.

### 3.5 Modern Credential Collection

```yaml
required_environment_variables:
  - name: GITHUB_TOKEN            # Required sub-field (alias: env_var)
    prompt: "Enter GitHub token"  # Optional; default: "Enter value for NAME"
    help: "https://docs.github.com/en/authentication/..."  # Optional URL or text
    required_for: "creating PRs"  # Optional — if present, shown as "(required for X)"
```

EdgeCrab MUST parse all four sub-fields. The `name` sub-field is required; the others
are optional with the defaults shown above.

**`env_var` alias (D-6):** The key `env_var` is accepted as an alias for `name` in every
entry of `required_environment_variables`. Both keys normalise to the same `RequiredEnvVar.name`
field. Hermes source: `env_name = str(entry.get("name") or entry.get("env_var") or "").strip()`.

**Env var name validation (D-8):** Every env var name MUST match 
`^[A-Za-z_][A-Za-z0-9_]*$` (identical to `_ENV_VAR_NAME_RE` in Hermes `skills_tool.py`).
Names that fail this check MUST be rejected with a clear error at install time.

```rust
static ENV_VAR_NAME_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap()  // Hermes _ENV_VAR_NAME_RE
});
```

### 3.6 Setup Block (Interactive Credential Wizard)

```yaml
setup:
  help: |
    How to get your 1Password Service Account Token:
    1. Go to developer.1password.com
    2. Create a new Service Account
  collect_secrets:
    - env_var: OP_SERVICE_ACCOUNT_TOKEN    # Required
      prompt: "1Password Service Account Token"  # Optional
      provider_url: "https://developer.1password.com/docs/service-accounts/"  # Optional
      url: "https://developer.1password.com/docs/service-accounts/"  # alias for provider_url (D-5)
      secret: true                         # Optional; default true; if true, mask echo
```

**`url` alias for `provider_url` (D-5):** The key `url` is accepted as an alias for
`provider_url` in every `collect_secrets` entry. Hermes source:
`provider_url = str(item.get("provider_url") or item.get("url") or "").strip()`.

**`setup.help` fallback (D-11):** If an individual `collect_secrets` entry has no
`help` / `provider_url` / `url` key, EdgeCrab MUST fall back to the top-level `setup.help`
string as the help text for that entry. This mirrors Hermes
`help_text = entry.get("help") or entry.get("provider_url") or setup_block.get("help") or ""`.

**EdgeCrab behaviour for `setup.collect_secrets`:**

- When loading a skill and at least one `collect_secrets` entry names an unset env var,
  EdgeCrab MUST offer an interactive credential wizard **before** injecting the skill.
- In non-interactive mode (gateway, batch, pipe) EdgeCrab MUST log a structured warning
  and inject the skill text unchanged (the LLM will surface the missing credential naturally).
- `setup.help` text MUST be printed before the first secret prompt.
- When `secret: true`, input MUST be hidden (no echo).
- Collected values are stored in the platform keychain or, if unavailable, in
  `~/.edgecrab/.env` with mode `0o600`, identical to Hermes convention.

**Remote environment backends (D-10):** When EdgeCrab is running inside a remote
execution backend, interactive credential wizards MUST be suppressed. The skill readiness
status is set to `UNSUPPORTED` instead of `SETUP_NEEDED`. Remote backends are identified
by the `HERMES_ENVIRONMENT` env var:

```rust
const REMOTE_ENV_BACKENDS: &[&str] = &["docker", "singularity", "modal", "ssh", "daytona"];
// Mirrors Hermes _REMOTE_ENV_BACKENDS in skills_tool.py
```

**Skill Readiness Status (D-9):** Every loaded skill MUST be assigned one of three
readiness states (mirrors Hermes `SkillReadinessStatus`):

```rust
pub enum SkillReadinessStatus {
    /// All required env vars are set (or skill has none). Skill is injected normally.
    Available,
    /// At least one required env var is missing. Wizard offered in interactive mode.
    SetupNeeded { missing: Vec<String> },
    /// Skill requires setup but runs in a remote/non-interactive backend. NOT injected.
    Unsupported { reason: String },
}
```

Resolution logic:
1. If no `required_environment_variables` and no `collect_secrets` → `Available`.
2. If all named env vars are set → `Available`.
3. If backend ∈ `REMOTE_ENV_BACKENDS` and vars missing → `Unsupported`.
4. Otherwise → `SetupNeeded { missing }`.

### 3.7 Metadata Block

```yaml
metadata:
  hermes:
    tags: [GitHub, Issues, API]          # string list; used for hub search
    related_skills: [github-auth, github-pr-workflow]  # cross-refs; display only in EdgeCrab
    category: security                   # optional; maps to hub category taxonomy
```

EdgeCrab MUST:
- Preserve all `metadata` sub-keys in the parsed `SkillManifest` struct.
- Expose `metadata.hermes.tags` to hub search indexing.
- Display `metadata.hermes.related_skills` in `/plugins info <name>` output with a
  note if any listed skill is not installed.
- Ignore unknown keys under `metadata` without error.

### 3.8 Complete SKILL.md Example (Round-Trip Correct)

```yaml
---
name: github-issues
description: >
  Manage GitHub issues via the gh CLI — create, list, triage, label, assign,
  and close issues with structured workflows. Supports bulk operations and
  milestone tracking.
version: 1.1.0
author: Hermes Agent
license: MIT
platforms: [macos, linux]
prerequisites:
  commands: [gh, git]
required_environment_variables:
  - name: GITHUB_TOKEN
    prompt: "GitHub personal access token"
    help: "https://github.com/settings/tokens — needs repo + issues scope"
    required_for: "creating and updating issues"
metadata:
  hermes:
    tags: [GitHub, Issues, CLI]
    related_skills: [github-auth, github-pr-workflow]
    category: version-control
---

# GitHub Issues Workflow

... skill body text ...
```

### 3.9 Hermes Python Directory Plugins

Hermes also defines a Python directory-plugin contract in the upstream guide
`website/docs/guides/build-a-hermes-plugin.md`. EdgeCrab MUST treat that guide as law.

#### 3.9.1 Supported Layout

```text
calculator/
├── plugin.yaml
├── __init__.py
├── schemas.py        # optional
├── tools.py          # optional
├── SKILL.md          # optional bundled skill
├── data/             # optional bundled data files
└── references/       # optional bundled docs
```

EdgeCrab MUST recognize a Hermes plugin root when both `plugin.yaml` (or `plugin.yml`) and
`__init__.py` are present.

#### 3.9.2 `plugin.yaml` Fields

| Field | Type | Notes |
|---|---|---|
| `name` | `string` | Required; defaults to directory name only if omitted by Hermes |
| `version` | `string` | Optional freeform |
| `description` | `string` | Optional freeform |
| `provides_tools` / `tools` | `string[]` | Declarative tool names; may be empty if runtime registration supplies tools |
| `provides_hooks` / `hooks` | `string[]` | Declarative hook names |
| `requires_env` | `string[]` or object[] | Same readiness gating semantics as Hermes |

#### 3.9.3 Local Install Rule

When a user runs `edgecrab plugins install ./path/to/plugin` on a raw Hermes directory plugin,
EdgeCrab MUST accept the bundle without requiring the author to add `plugin.toml`.
Implementation detail: EdgeCrab may synthesize internal metadata during quarantine, but the
Hermes-authored files MUST remain valid unchanged inputs.

#### 3.9.4 Runtime Contract

The plugin entrypoint is `register(ctx)` from `__init__.py`. EdgeCrab MUST accept the guide's
documented registration surface:

- `ctx.register_tool(...)` in positional or keyword form
- `ctx.register_hook(name, callback)`
- `ctx.register_memory_provider(provider)`
- `ctx.inject_message(content, role="user")`
- `ctx.register_cli_command(...)`

If EdgeCrab does not surface a feature natively yet, it MUST fail soft rather than failing to
load the plugin. `ctx.register_cli_command(...)` metadata, for example, may be accepted and
ignored temporarily, but MUST NOT break installation or runtime loading.

#### 3.9.5 Bundled Skill and Data Files

If a Hermes plugin root contains `SKILL.md`, EdgeCrab MUST load it as a bundled plugin skill
using the same parsing, readiness, path-translation, `compatibility`, and
`metadata.hermes.related_skills` rules as standalone Hermes skills.

Bundled skills MUST NOT disable runtime tools solely because the bundled skill is
`setup-needed` or platform-excluded. Prompt injection readiness and runtime tool readiness are
separate concerns.

Relative imports, `Path(__file__)` lookups, and reads from bundled `data/`, `references/`,
`templates/`, or similar subdirectories MUST continue to work unchanged.

For curated GitHub plugin sources whose repository contract explicitly includes repo-root
support files required by plugins below that root, EdgeCrab MUST materialize those files
into the installed plugin parent directory before first runtime use. This rule exists for
repositories such as `42-evey/hermes-plugins`, where plugins may import or locate
`evey_utils.py` from the repository root.

EdgeCrab MUST NOT guess these support files from a bare local plugin directory. Automatic
materialization is only allowed when repository identity is explicit and the support-file
contract is declared in EdgeCrab's curated source mapping.

#### 3.9.6 Minimum Hook Parity

EdgeCrab MUST execute at minimum these Hermes hook names when the plugin registers them:

- `on_session_start`
- `pre_llm_call`
- `post_tool_call`
- `on_session_end`

The runtime MAY support more Hermes hooks, but these four are the minimum compatibility floor
because they are exercised by the upstream guide and real upstream plugins.

---

## 4. Directory Structure

### 4.1 Skill Root Layouts (Both Supported)

```
Flat layout (simple skills):
    ~/.edgecrab/skills/
    └── my-skill/
        └── SKILL.md

Nested category layout (Hermes convention):
    ~/.edgecrab/skills/
    └── github/
        └── github-issues/
            ├── SKILL.md
            └── templates/
                └── issue-template.md
```

EdgeCrab's scanner MUST walk both layouts. Detection algorithm:

```
for each entry in skills_dir:
    if entry has a SKILL.md → it is a skill root (flat)
    elif entry is a directory:
        for each sub-entry:
            if sub-entry has a SKILL.md → it is a skill root (nested, 1 level deep)
```

Maximum scan depth: **2 levels below `skills/`**. Deeper nesting is not supported.

### 4.2 Allowed Supporting Subdirectories

EdgeCrab MUST allow (not block on security scan) these subdirectories inside a skill root:

```
references/    Supporting documentation markdown files
templates/     Output template files  
scripts/       Helper shell / Python scripts invoked by the skill
assets/        Images, JSON fixtures, supplementary files
```

Any other directory name inside a skill root MUST be flagged during validation but MUST NOT
prevent skill loading (warning only).

### 4.3 Excluded Directories

The following directory names MUST be excluded from skill scanning at any depth:

```rust
const EXCLUDED_SKILL_DIRS: &[&str] = &[".git", ".github", ".hub"];
```

This matches Hermes `_EXCLUDED_SKILL_DIRS = frozenset((".git", ".github", ".hub"))`.

### 4.4 Size Limits

These limits are identical to Hermes `skill_manager_tool.py`:

```rust
const MAX_SKILL_NAME_LENGTH: usize         = 64;
const MAX_SKILL_DESCRIPTION_LENGTH: usize  = 1_024;
const MAX_SKILL_CONTENT_CHARS: usize       = 100_000;   // ~36 k tokens
const MAX_SKILL_FILE_BYTES: u64            = 1_048_576; // 1 MiB per supporting file
```

Violations of content char limits MUST produce a hard error at install time and a warning
at load time (an already-installed skill that grew is still loaded, truncated with a banner).

### 4.5 Name Validation Regex

```rust
static VALID_SKILL_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^[a-z0-9][a-z0-9._-]*$").unwrap()
});
```

This is identical to Hermes `VALID_NAME_RE = re.compile(r'^[a-z0-9][a-z0-9._-]*$')`.

---

## 5. Trust Level Mapping

### 5.1 Canonical Names

EdgeCrab's canonical trust level names are kept for internal consistency. The Hermes names
are valid aliases that MUST be accepted wherever a trust level is specified.

```
+------------------+------------------+-------------------------------------------+
| EdgeCrab Name    | Hermes Alias     | Semantics                                 |
+------------------+------------------+-------------------------------------------+
| Official         | builtin          | Ships with the agent. Never scanned.      |
| Trusted          | trusted          | Verified third-party (whitelist of repos).|
| Community        | community        | Hub-sourced but not on whitelist.         |
| AgentCreated     | agent-created    | Produced at runtime by the agent itself.  |
| Unverified       | (no alias)       | Loaded from local path without metadata.  |
+------------------+------------------+-------------------------------------------+
```

**Parsing rule:** When reading `trust_level` from a hub index or lock file, map Hermes
aliases to EdgeCrab canonical names before storing. Emit the Hermes alias form when
writing files that may be consumed back by Hermes (e.g., `lock.json`).

### 5.2 Install Policy Matrix

Matches Hermes `INSTALL_POLICY` exactly:

```
+------------------+--------+---------+-----------+
| Trust Level      | Safe   | Caution | Dangerous  |
+------------------+--------+---------+-----------+
| Official         | allow  | allow   | allow      |
| Trusted          | allow  | allow   | block      |
| Community        | allow  | block   | block      |
| AgentCreated     | allow  | allow   | ask        |
| Unverified       | ask    | block   | block      |
+------------------+--------+---------+-----------+
```

`ask` means: pause installation and prompt the user for explicit confirmation, showing
the full scan findings. In non-interactive mode `ask` → `block`.

### 5.3 Trusted Repository Whitelist

```rust
/// Matches Hermes TRUSTED_REPOS hardcoded set.
/// EdgeCrab appends its own list but never removes these.
const HERMES_TRUSTED_REPOS: &[&str] = &[
    "openai/skills",
    "anthropics/skills",
];
```

Skills fetched from these repos receive `Trusted` trust level automatically.

---

## 6. Hub Compatibility

### 6.1 Hub Directory Layout

EdgeCrab's hub state directory mirrors Hermes exactly (substituting `~/.edgecrab/` for
`~/.hermes/`):

```
~/.edgecrab/skills/.hub/
├── lock.json          # Provenance of hub-installed skills
├── taps.json          # User-added tap sources (registries)
├── quarantine/        # Staging area during install verification
├── audit.log          # Append-only install/remove log
└── index-cache/       # Remote index JSON files
    ├── <tap-name>.json
    └── <tap-name>.json.etag
```

### 6.2 Index Cache TTL

```rust
const INDEX_CACHE_TTL_SECONDS: u64 = 3_600; // 1 hour — matches Hermes
```

**This overrides the 900 s value that appeared in `009_discovery_hub.md`.**

### 6.3 lock.json Format

The lock file records provenance for every hub-installed skill so EdgeCrab can detect
out-of-band modifications and support `plugins lock`.

```jsonc
{
  "lock_version": 1,
  "generated_at": "2025-01-15T10:30:00Z",
  "entries": {
    "github-issues": {
      "source":      "github",
      "identifier":  "openai/skills/github-issues",
      "trust_level": "trusted",        // Hermes alias form for Hermes interop
      "installed_at":"2025-01-15T10:29:58Z",
      "content_hash":"sha256:abcdef...",
      "files": ["SKILL.md", "templates/issue-template.md"]
    }
  }
}
```

EdgeCrab MUST be able to read a `lock.json` written by Hermes (which uses identical
schema under `~/.hermes/skills/.hub/lock.json`).

### 6.4 taps.json Format

```jsonc
{
  "taps": [
    {
      "name":    "official",
      "url":     "https://raw.githubusercontent.com/openai/skills/main/index.json",
      "enabled": true
    },
    {
      "name":    "community-hub",
      "url":     "https://clawhub.io/index.json",
      "enabled": true
    }
  ]
}
```

EdgeCrab MUST read Hermes-written `taps.json` without error.

### 6.5 Supported Hub Sources

EdgeCrab MUST recognise (and fetch from) all sources that Hermes supports:

```

EdgeCrab MAY also ship extra curated sources that are not part of Hermes core, provided
they are clearly labeled as EdgeCrab-curated additions. The current implementation includes
`hermes-evey` for `42-evey/hermes-plugins`.
+---------------------+------------------------------------------+---------------+
| source identifier   | Description                              | Trust default |
+---------------------+------------------------------------------+---------------+
| official            | openai/skills, anthropics/skills         | Trusted       |
| github              | Arbitrary GitHub org/repo path           | Community     |
| clawhub             | https://clawhub.io  (Hermes hub)         | Community     |
| claude-marketplace  | Anthropic Claude Marketplace             | Community     |
| lobehub             | https://lobehub.com/mcp/plugins-store    | Community     |
+---------------------+------------------------------------------+---------------+
```

### 6.6 Skill Download Protocol (No ZIP Archives)

**Important:** Hermes does NOT use ZIP archives. Skills are downloaded file-by-file via
the GitHub Contents API. EdgeCrab MUST implement the same protocol.

**GitHub Authentication (D-3) — 4-priority method chain:**

```
Priority 1: GITHUB_TOKEN or GH_TOKEN env var
            → Personal Access Token (PAT) — highest rate limit (5000 req/hr)
Priority 2: `gh auth token` subprocess
            → GitHub CLI token — requires gh CLI to be installed
Priority 3: GitHub App JWT
            → Requires GITHUB_APP_ID + GITHUB_APP_PRIVATE_KEY_PATH +
              GITHUB_APP_INSTALLATION_ID env vars
              Token cached for 3500 seconds
Priority 4: Unauthenticated
            → 60 req/hr — adequate for casual use
```

Mirrors Hermes `GitHubAuth._resolve_token()` in `tools/skills_hub.py`.

**DEFAULT_TAPS (D-4) — 4 sources (not 2):**

```rust
pub const DEFAULT_TAPS: &[SkillTapConfig] = &[
    SkillTapConfig { repo: "openai/skills",                  path: "skills/", trust: TrustLevel::Trusted    },
    SkillTapConfig { repo: "anthropics/skills",              path: "skills/", trust: TrustLevel::Trusted    },
    SkillTapConfig { repo: "VoltAgent/awesome-agent-skills", path: "skills/", trust: TrustLevel::Community  },
    SkillTapConfig { repo: "garrytan/gstack",                path: "",        trust: TrustLevel::Community  },
];
// Matches Hermes DEFAULT_TAPS exactly — skills_hub.py
```

Note: `garrytan/gstack` uses `path: ""` (repo root), while the others use `"skills/"`.

```
Algorithm:
  1. Resolve identifier → (owner, repo, path)  e.g. "openai/skills/github-issues"
  2. Try Git Trees API first (single request, avoids per-dir rate limiting):
     GET https://api.github.com/repos/{owner}/{repo}/git/trees/{branch}?recursive=1
     If truncated: true → fall back to step 3
  3. Fallback: GitHub Contents API recursive:
     GET https://api.github.com/repos/{owner}/{repo}/contents/{path}
       Header: Accept: application/vnd.github+json
       Header: Authorization: Bearer <resolved-token>
     For each entry:
       if type == "file":  download entry.download_url → bytes
       if type == "dir":   recurse (max 1 level deep)
  4. Assemble SkillBundle { name, files: BTreeMap<relative_path, bytes>, ... }
  5. Write to quarantine dir, scan, then move to skills dir.
```

**SkillBundle Rust representation:**

```rust
pub struct SkillBundle {
    pub name:        String,
    pub files:       BTreeMap<String, Vec<u8>>, // relative_path -> content
    pub source:      String,
    pub identifier:  String,
    pub trust_level: TrustLevel,
    pub metadata:    serde_json::Value,
}
```

### 6.7 audit.log Format

Append-only JSONL (one JSON object per line):

```jsonc
{"ts":"2025-01-15T10:29:58Z","event":"install","name":"github-issues","source":"official","trust":"trusted","verdict":"allow"}
{"ts":"2025-01-15T10:31:00Z","event":"remove","name":"old-skill","by":"user"}
```

---

## 7. Security Scan Compatibility

### 7.1 Threat Pattern Format

EdgeCrab's security scanner MUST use threat patterns that are a strict superset of
Hermes `THREAT_PATTERNS`. Pattern tuple format (Hermes):

```
( regex_pattern, pattern_id, severity, category, description )
```

Rust representation:

```rust
pub struct ThreatPattern {
    pub regex:       Regex,
    pub pattern_id:  &'static str,
    pub severity:    Severity,   // Critical | High | Medium | Low
    pub category:    Category,   // 12 categories — see §7.3
    pub description: &'static str,
}
```

### 7.2 Severity Vocabulary

Must match Hermes exactly:

```rust
pub enum Severity { Critical, High, Medium, Low }
```

### 7.3 Category Vocabulary

Must match Hermes **exactly** — all 12 categories (D-1: earlier drafts listed only 6):

```rust
pub enum Category {
    // Original 6
    Exfiltration,
    Injection,
    Destructive,
    Persistence,
    Network,
    Obfuscation,
    // Additional 6 (confirmed in skills_guard.py source)
    Execution,           // python_subprocess, os_system, backtick_subshell, etc.
    Traversal,           // path_traversal_deep, proc_access, dev_shm, etc.
    Mining,              // crypto_mining, mining_indicators
    SupplyChain,         // curl_pipe_shell, wget_pipe_shell, uv_run, git_clone, docker_pull, etc.
    PrivilegeEscalation, // sudo_usage, setuid_setgid, nopasswd_sudo, suid_bit, etc.
    CredentialExposure,  // hardcoded_secret, embedded_private_key, *_key_leaked, etc.
}
```

### 7.4 Minimum Required Threat Patterns

EdgeCrab MUST ship with at minimum every pattern Hermes ships with. The canonical
Hermes patterns are in `tools/skills_guard.py` → `THREAT_PATTERNS`. Representative
examples across all 12 categories (EdgeCrab must include all of them):

```
Pattern ID                  Severity  Category
---------------------------+----------+---------------------
env_exfil_curl              critical   exfiltration
env_exfil_wget              critical   exfiltration
hermes_env_access           high       exfiltration
edgecrab_env_access         high       exfiltration   ← D-12: EdgeCrab-specific pattern
context_exfil               high       exfiltration
send_to_url                 high       exfiltration
rm_rf_root                  critical   destructive
dd_device_write             critical   destructive
curl_exec                   high       injection
jailbreak_dan               high       injection
jailbreak_dev_mode          high       injection
eval_b64                    high       obfuscation
cron_write                  high       persistence
ssh_keygen_remote           high       persistence
agent_config_mod            high       persistence
hermes_config_mod           high       persistence
reverse_shell               critical   network
python_subprocess           high       execution
python_os_system            high       execution
backtick_subshell           high       execution
path_traversal_deep         critical   traversal
dev_shm                     medium     traversal
crypto_mining               critical   mining
curl_pipe_shell             critical   supply_chain
wget_pipe_shell             critical   supply_chain
uv_run                      high       supply_chain
docker_pull                 medium     supply_chain
sudo_usage                  high       privilege_escalation
setuid_setgid               high       privilege_escalation
hardcoded_secret            high       credential_exposure
embedded_private_key        critical   credential_exposure
github_token_leaked         critical   credential_exposure
openai_key_leaked           critical   credential_exposure
```

**EdgeCrab-specific pattern (D-12):** `edgecrab_env_access` must fire on patterns that
access `~/.edgecrab/.env` — this is the EdgeCrab equivalent of `hermes_env_access`. Both
patterns must be present in the scanner since a Hermes skill converted to EdgeCrab may
reference either path. Pattern regex:
```
~\/\.edgecrab\/\.env
```

The full list is documented in `006_security.md` §4.

---

## 8. Path Translation Policy

### 8.1 Home Directory References in Skill Content

A Hermes skill may embed `~/.hermes/` paths in its instruction text. EdgeCrab MUST
translate these on display without modifying the file on disk:

```
~/.hermes/          →  ~/.edgecrab/
~/.hermes/skills/   →  ~/.edgecrab/skills/
~/.hermes/memories/ →  ~/.edgecrab/memories/
~/.hermes/.env      →  ~/.edgecrab/.env
```

Translation is applied to the skill body text **before** injecting it into the system
prompt. The original file is never modified.

Implementation hook:

```rust
fn translate_hermes_paths(content: &str) -> Cow<str> {
    if !content.contains("~/.hermes/") {
        return Cow::Borrowed(content);
    }
    Cow::Owned(content.replace("~/.hermes/", "~/.edgecrab/"))
}
```

### 8.2 Script References

If a skill's body references a script via a relative path like `./scripts/run.sh`, the
path is resolved relative to the skill's root directory. EdgeCrab MUST NOT attempt to
execute scripts referenced in skill body text automatically; they are executed only when
the LLM invokes the `terminal` tool.

---

## 9. Optional Skills Concept

Hermes ships with an `optional-skills/` directory: skills that are bundled but not
default-activated. EdgeCrab represents this via the `enabled: false` default in
`config.yaml`:

```yaml
plugins:
  skills_dir: "~/.edgecrab/skills"
  optional_skills_dir: "~/.edgecrab/optional-skills"  # mirrored from Hermes
  optional_skills_enabled: []                           # explicit opt-in list
```

When a user runs `/plugins install <name>` for an optional built-in, EdgeCrab MUST copy
the skill from `optional_skills_dir` into `skills_dir` (not fetch from network).

An optional skill's `SKILL.md` uses identical format to a normal skill; there is no
frontmatter difference.

---

## 10. Data Flow Diagram

```
Hermes SKILL.md                          EdgeCrab SkillManifest
--------------------                     --------------------------
name: github-issues       parse          name: "github-issues"
description: "..."   ------------->      description: "..."
version: 1.1.0                           version: Some("1.1.0")
platforms: [macos]    normalize platform compatible: true (on macOS)
prerequisites:        normalize envvars  required_env_vars: [...]
  env_vars: [TOKEN]
required_env_vars:    merge (modern wins) required_env_vars: [...merged...]
setup:                credential wizard  on-load: prompt wizard if needed
  collect_secrets:
metadata.hermes.tags  hub indexing       tags: [...]
metadata.hermes.  
  related_skills       display only      related_skills: [...]
                            |
                            v
                    inject translated body
                    into system prompt
```

---

## 11. Invariants (Compatibility Guarantees)

| ID | Invariant | Verification |
|----|-----------|-------------|
| COMPAT-0 | Body after frontmatter must be non-empty after strip. | Unit test: SKILL.md with only frontmatter → `Err(EmptyBody)`. |
| COMPAT-1 | Every SKILL.md that passes Hermes validation also passes EdgeCrab validation. | Unit test: load all skills from hermes-agent fixture dir. |
| COMPAT-2 | SKILL.md files are never modified by EdgeCrab. | Test: checksum before/after load. |
| COMPAT-3 | `lock.json` written by EdgeCrab is readable by Hermes without modification. | Integration test: round-trip read by Python fixture. |
| COMPAT-4 | Trust level Hermes aliases are accepted in all EdgeCrab APIs. | Unit test: parse "trusted" → Trusted, "community" → Community, etc. |
| COMPAT-5 | Platform exclusion matches Hermes behaviour for all three OS values. | Property test: every combination of `platforms:` list and OS. |
| COMPAT-6 | Size limits are bit-identical to Hermes constants. | Compile-time `static_assert`-style check in test module. |
| COMPAT-7 | `~/.hermes/` references in skill body are translated before prompt injection. | Unit test: skill body with `~/.hermes/` emits `~/.edgecrab/`. |
| COMPAT-8 | `setup.collect_secrets` triggers credential wizard in interactive mode; `UNSUPPORTED` in remote backends. | Integration test with mock terminal + mock backend env var. |
| COMPAT-9 | `metadata.hermes.related_skills` appears in `/plugins info` output. | CLI snapshot test. |
| COMPAT-10 | `scripts/` is an allowed subdir and its contents are not blocked. | Unit test: security scan passes a skill with `scripts/helper.sh`. |
| COMPAT-11 | A raw Hermes directory plugin installs locally without a handwritten `plugin.toml`. | CLI E2E test installs guide-style plugin directory. |
| COMPAT-12 | A bundled `SKILL.md` inside a Hermes plugin root is loaded with normal skill metadata. | Integration test inspects bundled `compatibility` + `related_skills`. |
| COMPAT-13 | A guide-style Hermes plugin can read bundled data files and execute `post_tool_call`. | CLI E2E test runs calculator plugin and verifies hook side effect. |
| COMPAT-14 | A real upstream Hermes plugin installs and executes through the CLI. | CLI E2E test installs `plugins/memory/holographic`. |
| COMPAT-15 | A pip-installed Hermes entry-point plugin is discovered through the selected Python runtime. | CLI E2E test installs a local wheel into a temp venv and verifies `plugins list`. |
| COMPAT-16 | `ctx.register_cli_command()` is exposed as a real top-level EdgeCrab CLI command. | CLI E2E test runs `edgecrab entry-demo status`. |
| COMPAT-17 | Hermes `pre_api_request` and `post_api_request` hooks fire around provider calls. | Core integration test records both hooks from a Hermes plugin. |
| COMPAT-18 | CLI session boundary hooks fire for reset/finalize. | Core integration test verifies `on_session_reset` and `on_session_finalize`. |
| COMPAT-19 | Hermes memory-provider `cli.py register_cli(subparser)` is exposed through the compatibility bridge. | Integration test invokes real upstream `honcho` CLI help through the installed bundle. |
| COMPAT-20 | Installed Hermes bundles stamped with install-time trust metadata remain valid on rediscovery. | Manifest test accepts installer-stamped trusted metadata; CLI E2E rediscovery covers real `honcho`. |
| COMPAT-21 | Gateway sessions are isolated per chat and do not leak prior user history across chats. | Gateway integration test sends cross-chat messages and verifies per-session history boundaries. |
| COMPAT-22 | Gateway session boundary hooks fire across chat, reset, and shutdown paths. | Gateway integration test verifies `on_session_start`, `on_session_end`, `on_session_finalize`, and `on_session_reset`. |

---

## 12. Implementation Checklist

Use this checklist when submitting a PR for the compatibility layer in `edgecrab-plugins`:

- [ ] `SkillManifest` struct has all fields from §3
- [ ] `parse_skill_manifest()` normalises `prerequisites.env_vars` into `required_environment_variables` 
- [ ] Platform matching uses the `darwin`/`linux`/`win32` normalisation map
- [ ] `VALID_SKILL_NAME` regex matches `^[a-z0-9][a-z0-9._-]*$`
- [ ] Size constants match §4.4 exactly
- [ ] `EXCLUDED_SKILL_DIRS` contains `.git`, `.github`, `.hub`
- [ ] `ALLOWED_SUBDIRS` contains `references`, `templates`, `scripts`, `assets`
- [ ] Scanner at depth ≤ 2 levels below `skills_dir`
- [ ] Trust level aliases parsed (§5.1 table)
- [ ] Install policy matrix matches §5.2
- [ ] `HERMES_TRUSTED_REPOS` seeded with `openai/skills` and `anthropics/skills`
- [ ] `DEFAULT_TAPS` includes all 4 repos: openai/skills, anthropics/skills, VoltAgent/awesome-agent-skills, garrytan/gstack (§6.6)
- [ ] GitHub auth uses 4-method priority chain: env var → gh CLI → App JWT → unauthenticated (§6.6)
- [ ] Hub sources recognise `clawhub`, `claude-marketplace`, `lobehub` (§6.5)
- [ ] Skill download uses GitHub Contents API file-by-file (§6.6) — no ZIP
- [ ] `lock.json` format matches §6.3
- [ ] `taps.json` format matches §6.4
- [ ] `INDEX_CACHE_TTL_SECONDS = 3600`
- [ ] `translate_hermes_paths()` applied before prompt injection (§8.1)
- [ ] `setup.collect_secrets` wizard implemented (§3.6)
- [ ] `url` alias accepted for `provider_url` in `collect_secrets` (§3.6, D-5)
- [ ] `env_var` alias accepted for `name` in `required_environment_variables` (§3.5, D-6)
- [ ] `ENV_VAR_NAME_RE` validates all env var names `^[A-Za-z_][A-Za-z0-9_]*$` (§3.5, D-8)
- [ ] `setup.help` used as fallback when individual entry has no help/url (§3.6, D-11)
- [ ] `SkillReadinessStatus` enum: Available/SetupNeeded/Unsupported (§3.6, D-9)
- [ ] `REMOTE_ENV_BACKENDS` suppresses wizard and marks `Unsupported` (§3.6, D-10)
- [ ] `edgecrab_env_access` threat pattern present in scanner (§7.4, D-12)
- [ ] Non-empty body required after frontmatter (§7.4, D-7)
- [ ] `metadata.hermes.related_skills` displayed in info command (§3.7)
- [ ] Hermes entry-point plugins discovered from `hermes_agent.plugins`
- [ ] `ctx.register_cli_command()` surfaced as `edgecrab <plugin-command> ...`
- [ ] Hermes `VALID_HOOKS` set wired in CLI runtime
- [ ] Hermes hub indexing includes upstream `plugins/...` directories
- [ ] All 23 COMPAT-* invariants covered by tests
- [ ] Raw Hermes directory plugin install works without `plugin.toml` (§3.9.3)
- [ ] Bundled `SKILL.md` in Hermes plugins participates in prompt injection metadata (§3.9.5)
- [ ] `post_tool_call` is dispatched for Hermes plugins (§3.9.6)
- [ ] Raw-guide and real-upstream Hermes plugin CLI E2E tests exist (§11 COMPAT-11..14)
- [ ] `optional_skills_dir` wired to config (§9)

---

## 13. References

| Document | Relevance |
|---|---|
| `003_manifest.md` | `plugin.toml` format — superset of SKILL.md |
| `006_security.md` | Full threat pattern list, scan algorithm |
| `007_registry.md` | PluginRegistry — how skills are loaded at runtime |
| `009_discovery_hub.md` | Hub protocol — download, index, cache |
| `011_config_schema.md` | `plugins:` config section, env var resolution |
| `012_crate_structure.md` | `edgecrab-plugins` crate, module layout |
| hermes `tools/skills_tool.py` | Ground truth for SKILL.md parsing logic |
| hermes `tools/skills_hub.py` | Ground truth for hub file layout and SkillBundle |
| hermes `tools/skills_guard.py` | Ground truth for trust levels and threat patterns |
| hermes `tools/skill_manager_tool.py` | Ground truth for size limits and name validation |
