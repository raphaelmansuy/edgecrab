# 003.003 — Prompt Builder

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.001 Agent Struct](001_agent_struct.md) | [→ 007 Memory & Skills](../007_memory_skills/001_memory_skills.md)  
> **Source**: `edgecrab-core/src/prompt_builder.rs` — verified against real implementation  
> **Parity**: hermes-agent `agent/prompt_builder.py` + `agent/prompt_caching.py`

## 1. Prompt Assembly Pipeline

The system prompt is assembled from **~12 sources** in priority order. Built **once per session** and cached in `SessionState.cached_system_prompt` for Anthropic prompt caching stability.

```
PromptBuilder::new(platform)
    .skip_context_files(skip)
    .available_tools(tool_names)
    .build(override_identity, cwd, memory_sections, skill_prompt)
│
├── Slot 1: Identity (FROZEN at session start — never changes)
│       DEFAULT_IDENTITY (const) or global SOUL.md override
│
├── Slot 2: Platform hint (per-session, set at build)
│       CLI / Telegram / Discord / Slack / Cron / etc.
│
├── Slot 3: Date/time stamp
│       chrono::Local::now() — fresh each session
│
├── Slots 4-6: Context files (when skip_context_files=false)
│       ── SOUL.md  (walk cwd→parent→~/.edgecrab; .SOUL.md takes priority)
│       ── AGENTS.md  (project + ~/.edgecrab/AGENTS.md merged)
│       ── .cursorrules / CLAUDE.md / .edgecrab.md / .hermes.md
│       ── .cursor/rules/*.mdc  (Cursor IDE rules directory)
│       ── All scanned for prompt injection before injection
│       ── Truncated at CONTEXT_FILE_MAX_CHARS (20,000) via head/tail
│
├── Slot 7: Memory guidance + memory sections (gated on memory_write tool)
│       MEMORY_GUIDANCE constant + pre-loaded ~/.edgecrab/memories/*.md sections
│
├── Slot 8: Session search guidance (gated on session_search tool)
│       SESSION_SEARCH_GUIDANCE constant
│
├── Slot 9: Scheduling guidance (gated on manage_cron_jobs tool)
│       SCHEDULING_GUIDANCE constant
│
├── Slot 10: Skills guidance + skill summary (gated on skill_manage tool)
│        SKILLS_GUIDANCE constant + cached skill index (60s TTL)
│
├── Slot 11: Vision guidance (gated on vision_analyze tool)
│        VISION_GUIDANCE constant — tool selection rules
│
└── Slot 12: Personality addon (optional, from config.display.personality)
```

### Memory Snapshot Freeze

Memory is loaded **once at session start** and frozen into the cached system prompt. Mid-session `memory_write` calls update `~/.edgecrab/memories/` on disk but do **not** mutate `cached_system_prompt` — this is by design:

- **Why freeze**: Modifying the system prompt mid-session invalidates Anthropic's prompt cache at the system-prompt breakpoint, causing a full re-computation. The Anthropic cache-read discount disappears.
- **Consequence**: The agent's in-session memory writes take effect for **future sessions** only. The agent is told this explicitly in `MEMORY_GUIDANCE`.

### Context Files vs. API-Call-Time Layers

| Layer | When assembled | Frozen? |
|---|---|---|
| `cached_system_prompt` (all slots above) | First turn of session | ✅ Yes — never rebuilt |
| Context compression summary | Every turn (if threshold exceeded) | ❌ No — changes each turn |
| Prompt cache breakpoints | Every API call | ❌ No — applied in `build_chat_messages()` |

The compression summary is injected as a `Message::system_summary()` (role=`System`) into the **message list**, not into the cached system prompt. This keeps the cache-stable region stable while the summary evolves.

## 2. PromptBuilder Struct

```rust
// edgecrab-core/src/prompt_builder.rs

pub struct PromptBuilder {
    platform: Platform,
    skip_context_files: bool,
    /// When Some, guidance snippets are only injected when their gate tool is present.
    /// When None, all guidance is injected (backward compat / tests).
    available_tools: Option<Vec<String>>,
}

impl PromptBuilder {
    pub fn new(platform: Platform) -> Self { ... }
    pub fn skip_context_files(mut self, skip: bool) -> Self { ... }
    pub fn available_tools(mut self, tools: Vec<String>) -> Self { ... }

    /// Build the full system prompt — called once per session.
    /// Result is cached in SessionState for Anthropic prefix caching.
    pub fn build(
        &self,
        override_identity: Option<&str>,
        cwd: Option<&Path>,
        memory_sections: &[String],
        skill_prompt: Option<&str>,
    ) -> String { ... }
}
```

> **Tool-gated guidance**: Each guidance constant is only injected when its tool is available in the session. This saves tokens on configurations without those tools (e.g., ACP mode doesn't inject scheduling guidance).

## 3. Key Constants

```rust
const DEFAULT_IDENTITY: &str = "\
You are EdgeCrab, an intelligent AI agent built with Rust for speed and safety. \
You are helpful, knowledgeable, and direct. You assist users with a wide range of \
tasks including answering questions, writing and debugging code, code review, \
architecture design, analysing information, creative work, and executing actions \
via your tools. You communicate clearly, admit uncertainty when appropriate, and \
prioritise being genuinely useful over being verbose unless otherwise directed. \
Be targeted and efficient in your exploration and investigations.";

const CLI_HINT: &str = "\
You are a CLI AI Agent. Use markdown formatting with code blocks where helpful. \
ANSI colors are supported.";

// Additional platform hints: TELEGRAM_HINT, DISCORD_HINT, WHATSAPP_HINT,
// SLACK_HINT, SIGNAL_HINT, EMAIL_HINT, SMS_HINT, WEBHOOK_HINT, API_HINT, CRON_HINT
```

## 4. Context File Discovery

```
SOUL.md (ordered, first-found wins):
  1. <cwd>/.SOUL.md          (hidden file takes priority)
  2. <cwd>/SOUL.md
  3. <parent>/.SOUL.md       (walk up to /)
  4. <parent>/SOUL.md
  ... recursive until /
  5. ~/.edgecrab/SOUL.md     (global fallback)

AGENTS.md (both merged if both exist):
  1. <cwd>/AGENTS.md         (project-level)
  2. ~/.edgecrab/AGENTS.md   (global agent instructions)

Other context files (cwd, first-found):
  .cursorrules | CLAUDE.md | .edgecrab.md | .hermes.md | .cursor/rules/*.mdc
```

### Context File Truncation

Files exceeding `CONTEXT_FILE_MAX_CHARS` (20,000) are truncated using a head/tail strategy:

```rust
const CONTEXT_FILE_MAX_CHARS: usize = 20_000;
const TRUNCATION_HEAD_RATIO: f64 = 0.70;  // 70% head, 30% tail
```

### Injection Scanning

All context files are scanned for prompt injection patterns before injection into the system prompt:

```rust
pub fn scan_for_injection(text: &str) -> Vec<InjectionThreat> {
    // Checks for:
    // - Text patterns: "ignore previous", "you are now", "new instructions:", etc.
    // - Invisible Unicode characters (zero-width space, BOM, etc.)
    // - Homoglyph characters (Cyrillic/Greek lookalikes for Latin)
}

pub enum ThreatSeverity { Low, Medium, High }
```

Files that contain high-severity threats are replaced with `[BLOCKED: prompt injection detected in <filename>]`.

### YAML Frontmatter Stripping

Context files containing YAML frontmatter (e.g., `---\ntitle: ...\n---\n`) are stripped before injection:

```rust
pub fn strip_yaml_frontmatter(text: &str) -> &str { ... }
```

## 5. Skills Cache

Module-level in-memory cache avoids re-scanning `~/.edgecrab/skills/` on every session start:

```rust
struct SkillsCacheEntry {
    summary: Option<String>,
    disabled_at_build: Vec<String>,
    built_at: std::time::Instant,
}

const SKILLS_CACHE_TTL: Duration = Duration::from_secs(60);

/// Invalidated after skill_manage mutations (create/edit/patch/delete)
pub fn invalidate_skills_cache()
```

## 6. Standalone Loader Functions

```rust
/// Load SOUL.md from global ~/.edgecrab/ directory
pub fn load_global_soul(edgecrab_home: &Path) -> Option<String>

/// Load MEMORY.md and USER.md from memories/ subdirectory
pub fn load_memory_sections(edgecrab_home: &Path) -> Vec<String>

/// Load full SKILL.md content for preloaded skills (via -S flag)
pub fn load_preloaded_skills(edgecrab_home: &Path, skill_names: &[String]) -> String

/// Load compact skill index for system prompt injection (60s cached)
pub fn load_skill_summary(
    edgecrab_home: &Path,
    disabled_skills: &[String],
    extra_skill_dirs: &[String],
) -> Option<String>
```

## 7. Anthropic Prompt Caching Policy

**CRITICAL — do not violate:**

> The system prompt is built **once per session** and stored in `SessionState.cached_system_prompt`. Do **NOT** rebuild or mutate it mid-conversation — this invalidates Anthropic's prompt cache and dramatically increases costs.

Cache control breakpoints are applied at API-call time (not build time), handled by `edgequake_llm::apply_cache_control()`:

```
System message    → always cacheable (stable prefix)
Last user message → cache write point (for cache continuations)
Other messages    → no cache control
```

## 8. Platform Hints

| Platform | Key Guidance |
|----------|-------------|
| CLI | Full markdown, code blocks, ANSI colors |
| Telegram | No markdown, max 4096 chars, MEDIA: prefix for files |
| Discord | Max 2000 chars, MEDIA: prefix for attachments |
| WhatsApp | No markdown, MEDIA: for native attachments |
| Slack | Slack mrkdwn format, MEDIA: for uploads |
| Signal | No markdown, MEDIA: for attachments |
| Email | Plain text, subject threading, MEDIA: for attachments |
| SMS | Plain text only, ~1600 char limit |
| Webhook | JSON-friendly structured responses |
| API | Markdown, structured for programmatic consumption |
| Cron | Autonomous mode, no user interaction possible |
