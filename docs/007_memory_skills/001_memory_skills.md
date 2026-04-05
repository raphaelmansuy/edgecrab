# 007.001 — Memory & Skills

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.003 Prompt Builder](../003_agent_core/003_prompt_builder.md) | [→ 009 Config](../009_config_state/001_config_state.md)
> **Source**: `edgecrab-tools/src/tools/memory.rs`, `edgecrab-tools/src/tools/skills.rs`, `edgecrab-tools/src/tools/session_search.rs`

## 1. Memory System

### Memory Files

| File | Location | Purpose |
|------|----------|---------|
| `MEMORY.md` | `~/.edgecrab/memories/MEMORY.md` | Agent's persistent knowledge base |
| `USER.md` | `~/.edgecrab/memories/USER.md` | User preferences and context |

> **Important**: Memory files reside in `memories/` subdir of EDGECRAB_HOME.
> The directory is auto-created on first write.

### Memory Tool Implementation

```rust
// edgecrab-tools/src/tools/memory.rs (actual implementation)

/// Entry delimiter — § (section sign) separates entries
const ENTRY_DELIMITER: &str = "\n§\n";

/// Maximum characters for MEMORY.md (agent's curated notes).
const MEMORY_MAX_CHARS: usize = 2200;
/// Maximum characters for USER.md (user profile).
const USER_MAX_CHARS: usize = 1375;

/// memory_write tool — actions: add | replace | remove
pub struct MemoryWriteTool;

/// memory_read tool — reads MEMORY.md or USER.md
pub struct MemoryReadTool;

// Both tools use direct async file I/O (tokio::fs):
//   1. read_to_string the target file
//   2. split by ENTRY_DELIMITER
//   3. apply action (add / replace / remove)
//   4. check injection via edgecrab_security::check_injection()
//   5. enforce char limit (MEMORY_MAX_CHARS / USER_MAX_CHARS)
//   6. write_all back atomically (write to tmp, rename)
```

### Memory Nudge

The agent periodically reminds itself to review and update memory:

```rust
struct MemoryNudgeTracker {
    turns_since_last: u32,
    interval: u32,       // from config, default 10
}

impl MemoryNudgeTracker {
    fn should_nudge(&mut self) -> bool {
        self.turns_since_last += 1;
        if self.turns_since_last >= self.interval {
            self.turns_since_last = 0;
            true
        } else {
            false
        }
    }
}
```

## 2. Skills System

### Skill Directory Structure

```
~/.edgecrab/skills/
├── my_skill/
│   ├── SKILL.md           # Skill description + system prompt
│   ├── config.yaml        # Parameters, triggers, dependencies
│   └── examples/          # Example interactions
├── another_skill/
│   └── SKILL.md
```

### Skill Tool Handlers

**Source**: `edgecrab-tools/src/tools/skills.rs`

Skills are not managed by a central `SkillStore` struct. Instead, five `ToolHandler`
implementations each perform direct filesystem I/O over `~/.edgecrab/skills/`:

| Tool struct | Tool name | Purpose |
|-------------|-----------|---------|
| `SkillsListTool` | `skills_list` | List all skills across configured directories |
| `SkillsCategoriesList` | `skills_categories` | List skills grouped by category |
| `SkillViewTool` | `skill_view` | Read a skill's `SKILL.md` and optional extra files |
| `SkillManageTool` | `skill_manage` | Create / update / delete / enable / disable skills |
| `SkillsHubTool` | `skills_hub` | Search and install skills from the hub registry |

Path expansion supports `~` and `${VAR}` substitution. External skill directories
(beyond the default `~/.edgecrab/skills/`) can be added via config. Discovery walks
all directories, matching subdirectories that contain a `SKILL.md` file.

Skills support YAML frontmatter for metadata:

```yaml
---
name: My Skill
description: What this skill does
category: coding
platforms: [cli, telegram]
read_files: [extra.md, examples.md]
---
```

### Skill Nudge

```rust
struct SkillNudgeTracker {
    iters_since_last: u32,
    interval: u32,       // from config, default 15
}
```

## 3. Session Search (FTS5)

**Source**: `edgecrab-state/src/session_db.rs`, `edgecrab-tools/src/tools/session_search.rs`

The `session_search` tool is a thin wrapper over the `SessionDb` FTS5 engine inside
`edgecrab-state`. There is no separate `SessionSearch` struct.

### FTS5 Schema (edgecrab-state/src/schema.sql)

```sql
-- Content-table FTS5 index synced to messages table via triggers.
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    content=messages,
    content_rowid=id
);

-- Sync triggers: keep the index in lockstep with the messages table.
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages
  BEGIN INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content); END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages
  BEGIN INSERT INTO messages_fts(messages_fts, rowid, content)
        VALUES('delete', old.id, old.content); END;
```

### Search Result Type (edgecrab-state/src/session_db.rs)

```rust
/// FTS5 search result with BM25 score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub session_id: String,
    pub role: String,
    pub snippet: String,
    pub score: f64,
}
```

### SessionSearchTool Behaviour

- If `query` is absent or empty → falls back to listing the N most-recent sessions
  ordered by start time (no FTS involved).
- FTS5 queries are escaped before execution to prevent SQLite FTS5 syntax errors.
- Default `limit`: 10 results.

## 4. Honcho Integration

Cross-session user modeling via external Honcho service:

```rust
// edgecrab-core/src/honcho.rs

pub struct HonchoClient {
    base_url: String,
    api_key: String,
    client: reqwest::Client,
}

pub struct HonchoConfig {
    pub enabled: bool,
    pub base_url: String,
    pub app_id: String,
    pub recall_mode: RecallMode, // "hybrid" | "tools" | "prefetch"
}

#[derive(Clone, Copy)]
pub enum RecallMode {
    Hybrid,    // prefetch + tools
    Tools,     // tools only (honcho_context/search/profile)
    Prefetch,  // automatic prefetch only
}

impl HonchoClient {
    pub async fn prefetch(&self, user_id: &str, message: &str) -> Result<Option<String>> { ... }
    pub async fn get_context(&self, session_key: &str) -> Result<String> { ... }
    pub async fn get_profile(&self, user_id: &str) -> Result<String> { ... }
    pub async fn search(&self, query: &str) -> Result<Vec<String>> { ... }
    pub async fn conclude(&self, session_key: &str, summary: &str) -> Result<()> { ... }
}
```

## 5. Skills Guard & Sync

| Module | Purpose | EdgeCrab Implementation |
|--------|---------|----------------|
| Skills Guard | Sandbox skill execution, prevent escapes | `edgecrab-tools/src/tools/skills_guard.rs` — regex-based static analysis |
| Skills Sync | Sync skills across directories | `edgecrab-tools/src/tools/skills_sync.rs` — filesystem copy with `notify` watcher |
| Skills Hub | Community skill marketplace | `edgecrab-tools/src/tools/skills_hub.rs` — HTTP client → skills registry API |

## 6. Memory Content Scanning (Security)

Memory entries are injected into the system prompt and must be sanitized before writing.
EdgeCrab uses `check_injection()` from `edgecrab-security/src/injection.rs`:

```rust
// edgecrab-security/src/injection.rs

/// Patterns that indicate a prompt injection attempt in user-supplied text.
const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous",
    "ignore all instructions",
    "override system",
    "you are now",
    "forget everything",
    "new instructions:",
    "system prompt:",
    "disregard",
];

/// Return a human-readable error message if `text` contains a prompt injection
/// pattern, or `None` if the text is safe.
///
/// The check is case-insensitive. Use this before persisting any user-supplied
/// content that will later be injected into an LLM prompt.
pub fn check_injection(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    for p in INJECTION_PATTERNS {
        if lower.contains(p) {
            return Some("Content contains prompt injection pattern — write blocked");
        }
    }
    None
}
```

The `memory_write` tool calls `check_injection()` on every entry before persisting
to disk. Blocked writes return `ToolError::Validation` with the reason string.

## 7. Skills Guard Detail (v0.4.0)

### 7.1 Trust Levels & Install Policy

**Source**: `edgecrab-tools/src/tools/skills_guard.rs`

Trust level is represented as a `&str` string, not an enum:

| Trust level string | Description |
|-------------------|-------------|
| `"builtin"` | Ships with EdgeCrab — always allowed regardless of scan verdict |
| `"trusted"` | From `TRUSTED_REPOS` (`openai/skills`, `anthropics/skills`) — blocked only on `Dangerous` verdict |
| `"community"` | All other sources — blocked on `Caution` or `Dangerous` |

```rust
pub const TRUSTED_REPOS: &[&str] = &["openai/skills", "anthropics/skills"];

/// Determine whether a skill should be allowed based on scan results and trust.
/// Returns `(allowed, reason)`.
pub fn should_allow_install(result: &ScanResult) -> (bool, String) {
    match (result.trust_level.as_str(), result.verdict) {
        ("builtin", _) => (true, "builtin skills are always trusted".into()),
        ("trusted", Verdict::Dangerous) => (false, "trusted skill has dangerous findings".into()),
        ("trusted", _) => (true, "trusted source, scan passed".into()),
        ("community", Verdict::Safe) => (true, "community skill passed scan".into()),
        ("community", _) => (false, "community skill has suspicious/dangerous findings".into()),
        _ => (false, "unknown trust level".into()),
    }
}
```

### 7.2 Scan Result & Findings

```rust
// edgecrab-tools/src/tools/skills_guard.rs

pub struct Finding {
    pub pattern_id: String,
    pub severity: Severity,     // Critical | High | Medium | Low
    pub category: ThreatCategory, // Exfiltration | Injection | Destructive | Persistence | Network | Obfuscation
    pub file: String,             // path string (not PathBuf)
    pub line: usize,
    pub matched_text: String,     // the matching text fragment
    pub description: String,
}

pub struct ScanResult {
    pub skill_name: String,
    pub source: String,
    pub trust_level: String,      // string, not enum
    pub verdict: Verdict,
    pub findings: Vec<Finding>,
    pub summary: String,
}
```

### 7.3 Threat Categories Scanned

| Category | Examples |
|----------|----------|
| Exfiltration | curl/wget/fetch/httpx with `$KEY`/`$TOKEN`/`$SECRET`, reading `.env`/`.netrc` |
| Injection | `ignore previous instructions`, `you are now`, `system prompt override` |
| Destructive | `rm -rf /`, drop database, format disk |
| Persistence | `authorized_keys`, `~/.ssh`, crontab modification |
| Network | Outbound connections to unknown hosts |
| Obfuscation | Base64-encoded commands, eval/exec of dynamic strings |

## 8. Skills Hub Architecture (v0.4.0)

### 8.1 Hub State Directory

```
~/.edgecrab/skills/.hub/
├── lock.json         # Provenance tracking of installed hub skills
├── quarantine/       # Downloaded skills held until scanned
├── audit.log         # All install/remove/scan events
├── taps.json         # Registered source repositories ("taps")
└── index-cache/      # Cached skill indexes per source
```

### 8.2 Skill Bundle & Meta Types

**Source**: `edgecrab-tools/src/tools/skills_hub.rs`

```rust
pub struct SkillMeta {
    pub name: String,
    pub description: String,
    pub source: String,       // "official", "github"
    pub identifier: String,   // source-specific ID
    pub trust_level: String,  // "builtin", "trusted", "community"
    pub repo: Option<String>,
    pub path: Option<String>,
    pub tags: Vec<String>,
}

pub struct SkillBundle {
    pub name: String,
    pub files: HashMap<String, String>, // relative_path -> file content
    pub source: String,
    pub identifier: String,
    pub trust_level: String,
}

pub struct Tap {
    pub name: String,
    pub url: String,
    pub trust_level: String,
}
```

GitHub sources are fetched via the GitHub Contents API using `reqwest` directly;
there is no dedicated `GitHubSource` struct.

### 8.3 Lock File

The lock file is a `HashMap<String, LockEntry>` serialized to JSON (no wrapper struct):

```rust
// edgecrab-tools/src/tools/skills_hub.rs

pub struct LockEntry {
    pub source: String,
    pub identifier: String,
    pub installed_at: String,   // ISO-8601 string
    pub content_hash: String,   // SHA fingerprint for integrity
}

/// Read the hub lock file (~/.edgecrab/skills/.hub/lock.json).
pub fn read_lock() -> HashMap<String, LockEntry> { ... }
```

## 9. Skill Frontmatter & Readiness (v0.4.0)

### 9.1 YAML Frontmatter Schema

```yaml
---
name: my-skill
description: "Short one-liner"
tags: [research, productivity]
enabled: true
priority: 50
platforms: [cli, telegram, discord]    # platform filtering
prerequisites:
  env_vars:
    - name: OPENAI_API_KEY
      description: "Required for skill"
  python_packages: [requests, beautifulsoup4]
setup:
  instructions: "Run: pip install requests"
  auto_install: true
triggers: ["keyword1", "keyword2"]
allowed-tools: [read_file, web_search]   # skill-scoped tool restrictions
---
```

### 9.2 Readiness Status

```rust
pub enum SkillReadinessStatus {
    Ready,                // all prerequisites met
    MissingEnvVars(Vec<String>),
    MissingPackages(Vec<String>),
    PlatformMismatch,     // not available on current platform
    Disabled,
}

impl Skill {
    pub fn check_readiness(&self) -> SkillReadinessStatus {
        if !self.config.enabled { return SkillReadinessStatus::Disabled; }
        if !self.matches_platform() { return SkillReadinessStatus::PlatformMismatch; }
        let missing_env = self.missing_env_vars();
        if !missing_env.is_empty() { return SkillReadinessStatus::MissingEnvVars(missing_env); }
        SkillReadinessStatus::Ready
    }
}
```

### 9.3 Skill Commands

Skills can register custom slash commands via their frontmatter:

```rust
pub fn scan_skill_commands(skills: &SkillStore) -> HashMap<String, SkillCommand> {
    let mut commands = HashMap::new();
    for (name, skill) in skills.iter() {
        if let Some(cmd) = &skill.config.slash_command {
            commands.insert(cmd.clone(), SkillCommand {
                skill_name: name.clone(),
                description: skill.description.clone(),
            });
        }
    }
    commands
}
```

## 10. Claude Skills Compatibility (EdgeCrab Extension)

EdgeCrab implements a **superset** of Anthropic's Claude Skills YAML frontmatter format,
plus compatibility with the agentskills.io convention. Skills authored for either system
work in EdgeCrab without modification.

### 10.1 Full YAML Frontmatter Schema (Unified)

```yaml
---
# === Core fields (both agentskills.io + Claude Skills) ===
name: my-skill                        # Required, unique identifier (max 64 chars)
description: "Short one-liner"         # Required, max 1024 chars

# === Claude Skills fields ===
disable-model-invocation: false        # If true, skill runs without LLM (pure script)
user-invocable: true                   # If false, only other skills/agents can invoke
allowed-tools:                         # Restrict which tools this skill can use
  - read_file
  - web_search
  - terminal
context: fork                          # "fork" = run skill in isolated subagent context
agent: "Claude"                        # Override agent/model for this skill
model: claude-sonnet-4-20250514    # Override model specifically
effort: medium                         # Reasoning effort: low | medium | high
shell: /bin/bash                       # Shell for !`cmd` substitutions (default: /bin/sh)
hooks:                                 # Lifecycle hooks
  pre: "echo 'starting'"
  post: "echo 'done'"

# === agentskills.io fields ===
version: 1.0.0
license: MIT
platforms: [macos, linux]              # OS platform filtering
prerequisites:
  env_vars:
    - name: OPENAI_API_KEY
      description: "Required for skill"
  python_packages: [requests]
  commands: [curl, jq]
setup:
  instructions: "Run: pip install requests"
  auto_install: true
tags: [research, productivity]
triggers: ["keyword1", "keyword2"]

# === EdgeCrab extensions ===
paths: ["src/**/*.rs", "docs/**/*.md"] # Auto-activate when these files are in context
priority: 50
enabled: true
metadata:
  hermes:
    related_skills: [peft, lora]
compatibility: "Requires Python 3.10+"
---
```

### 10.2 `$ARGUMENTS` Substitution

Claude Skills support `$ARGUMENTS` (full argument string), `$ARGUMENTS[0]`..`$ARGUMENTS[N]`
(indexed), and `$1`..`$N` (short form) substitutions in the skill body:

```rust
/// Substitute $ARGUMENTS, $ARGUMENTS[N], and $N placeholders
pub fn substitute_arguments(template: &str, args: &str) -> String {
    let parts: Vec<&str> = args.split_whitespace().collect();
    let mut result = template.replace("$ARGUMENTS", args);
    for (i, part) in parts.iter().enumerate() {
        result = result.replace(&format!("$ARGUMENTS[{}]", i), part);
        result = result.replace(&format!("${}", i + 1), part);
    }
    result
}
```

### 10.3 `!`command`` Shell Injection

Skills can dynamically inject command output into their body using shell backtick syntax:

```rust
/// Expand !`command` patterns by executing the shell command and substituting output.
/// SECURITY: Only allowed for builtin/trusted skills. Community skills are blocked.
pub async fn expand_shell_injections(
    content: &str,
    trust_level: TrustLevel,
    shell: &str,
) -> Result<String> {
    if !matches!(trust_level, TrustLevel::Builtin | TrustLevel::Trusted) {
        return Ok(content.to_string()); // No shell expansion for untrusted skills
    }
    let re = Regex::new(r"!`([^`]+)`").unwrap();
    let mut result = content.to_string();
    for cap in re.captures_iter(content) {
        let cmd = &cap[1];
        let output = tokio::process::Command::new(shell)
            .arg("-c")
            .arg(cmd)
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        result = result.replacen(&cap[0], stdout.trim(), 1);
    }
    Ok(result)
}
```

### 10.4 Skill Execution Modes

| Mode | Description | Implementation |
|------|-------------|---------------|
| **Prompt injection** | Skill body added to system prompt (default) | `SkillStore::build_system_prompt()` |
| **Fork context** | Skill runs in isolated subagent (`context: fork`) | Spawns new `Agent` with skill-scoped tools |
| **Script execution** | Skill runs Python/Node/Shell directly (`disable-model-invocation: true`) | `tokio::process::Command` with sandbox |

### 10.5 Script Skill Runtimes

Skills with `disable-model-invocation: true` can include executable scripts:

```
~/.edgecrab/skills/my-script-skill/
├── SKILL.md          # frontmatter with disable-model-invocation: true
├── run.py            # Python script
├── run.js            # Node.js script
└── run.sh            # Shell script
```

EdgeCrab detects the runtime from file extension and executes in a sandboxed subprocess:

```rust
pub async fn execute_script_skill(
    skill: &Skill,
    args: &str,
    ctx: &ToolContext,
) -> Result<String> {
    let script = skill.find_script()?; // looks for run.py, run.js, run.sh
    let (cmd, script_args) = match script.extension().and_then(|e| e.to_str()) {
        Some("py") => ("python3", vec![script.to_str().unwrap()]),
        Some("js") => ("node", vec![script.to_str().unwrap()]),
        Some("sh") => ("sh", vec![script.to_str().unwrap()]),
        _ => return Err(ToolError::UnsupportedRuntime(script.display().to_string())),
    };
    let output = tokio::process::Command::new(cmd)
        .args(&script_args)
        .arg(args)
        .current_dir(&ctx.cwd)
        .env("EDGECRAB_SESSION_ID", &ctx.session_id)
        .env("EDGECRAB_CWD", &ctx.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?
        .wait_with_output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
```
