# Prompt Builder 🦀

> **Verified against:** `crates/edgecrab-core/src/prompt_builder.rs`

---

## Why a centralised prompt builder exists

Hand-rolled system prompts are an antipattern in multi-frontend agents.
If the CLI, gateway, and ACP server each assemble their own prompt strings,
you get: three copies of the memory-injection logic, three places to update
when you add a new guidance block, and three diverging prompt formats
to debug.

`PromptBuilder` is the single point of assembly. All frontends call it once
at session start. The result is cached in `SessionState::cached_system_prompt`
and reused for every subsequent API call — prompt construction is not free.

---

## Context files scanned (in order)

```
  Global identity:
    ~/.edgecrab/SOUL.md          ← identity slot (never injected as generic context)

  Project instructions (scanned in cwd):
    .edgecrab.md                 ← primary project file
    EDGECRAB.md
    .hermes.md                   ← legacy compatibility
    HERMES.md
    AGENTS.md                    ← OpenAI Agents SDK standard
    CLAUDE.md                    ← Anthropic Claude Code standard
    .cursorrules                 ← Cursor compatibility
    .cursor/rules/*.mdc          ← Cursor rule files

  All project context files undergo injection checking before inclusion.
```

`SOUL.md` is treated specially as the persona/identity slot — it sets the
agent's character. Project files (`AGENTS.md`, `CLAUDE.md`, `.edgecrab.md`)
add project-specific instructions on top of the persona.

---

## Prompt assembly order

```
  ┌─────────────────────────────────────────────────────────────────┐
  │  Final system prompt                                             │
  │                                                                  │
  │  [1] SOUL.md content (if present)                                │
  │       └── "You are EdgeCrab, a powerful coding assistant..."     │
  │                                                                  │
  │  [2] Platform hint                                               │
  │       └── "You are running on platform: telegram"               │
  │                                                                  │
  │  [3] Timestamp                                                   │
  │       └── "The current time is 2026-04-05 14:32 UTC"            │
  │                                                                  │
  │  [4] Context files (AGENTS.md, CLAUDE.md, .edgecrab.md, ...)     │
  │       each injection-checked before inclusion                    │
  │                                                                  │
  │  [5] Memory guidance block (if memory tool enabled)              │
  │       └── "When you learn something, write it to memory..."      │
  │                                                                  │
  │  [6] Memory content (from ~/.edgecrab/memories/)                 │
  │       └── Contents of each memory section file                  │
  │                                                                  │
  │  [7] Session search guidance (if session tool enabled)           │
  │                                                                  │
  │  [8] Skills guidance (if skills tools enabled)                   │
  │       └── How to invoke, install, create skills                  │
  │                                                                  │
  │  [9] Skills summary (from ~/.edgecrab/skills/ scan)              │
  │       └── Brief description of each installed skill              │
  │                                                                  │
  │  [10] Tool-specific guidance blocks                              │
  │        cron guidance (if manage_cron_jobs enabled)               │
  │        messaging guidance (if send_message enabled)              │
  │        image analysis guidance (if vision_analyze enabled)       │
  └─────────────────────────────────────────────────────────────────┘
```

---

## Skills summary caching

The skills directory (`~/.edgecrab/skills/`) can contain thousands of files.
Scanning it on every session start would noticeably slow startup. The builder
uses an in-process `OnceLock`-based cache:

```
  First session:
    scan ~/.edgecrab/skills/, read frontmatter
    extract name + description per skill
    cache the summary string
    ↓
  All subsequent sessions this process:
    return cached string immediately (no disk I/O)

  Explicit invalidation:
    Agent::invalidate_system_prompt()
    → next build() rescans the skills directory
```

---

## Injection checking

Before any external content (context files, memory files, skill files) enters
the system prompt, it passes through:
```sh
edgecrab_security::injection::check_injection(content)
```

This scans for:
- Prompt injection patterns: `"ignore previous instructions"`,
  `"you are now"`, `"system prompt:"`, HTML comments `<!--`, etc.
- Invisible Unicode characters (zero-width spaces, directional overrides)
- Exfiltration patterns in memory: `curl`/`wget` with secret env vars,
  `cat ~/.ssh/`, etc.

Files that fail the check are **replaced** with a placeholder in the prompt
rather than silently dropped or used as-is.

🦀 *A rogue `AGENTS.md` containing injection instructions is a risk for any agent
that loads context files without scanning them. EdgeCrab's prompt builder blocks
it at the gate.*

---

## Conditional guidance blocks

Guidance is only injected when the relevant tools are active. This prevents
the system prompt from growing unbounded for minimal toolset configurations:

| Guidance block | Injected when |
|---|---|
| Memory guidance | `memory_read` or `memory_write` in active toolset |
| Session search guidance | `session_search` in active toolset |
| Skills guidance | `skills_list` or `skill_manage` in active toolset |
| Cron guidance | `manage_cron_jobs` in active toolset |
| Messaging guidance | `send_message` in active toolset |
| Image analysis guidance | `vision_analyze` in active toolset |

---

## Key public functions

```rust
// Build the full system prompt for a session
pub async fn build(
    config: &AgentConfig,
    tool_registry: Option<&ToolRegistry>,
    cwd: &Path,
    platform: Platform,
) -> String

// Extract the `name:` field from a skill file's YAML frontmatter
pub fn extract_frontmatter_name(content: &str) -> Option<String>

// Extract the `description:` field from skill frontmatter
pub fn extract_skill_description(content: &str) -> Option<String>

// Load all memory sections from ~/.edgecrab/memories/ (or profile dir)
pub fn load_memory_sections(config: &AgentConfig) -> Vec<(String, String)>

// Return pre-loaded skill names with brief descriptions (cached)
pub fn load_preloaded_skills(config: &AgentConfig) -> String

// Summarise skills directory for prompt injection
pub fn load_skill_summary(config: &AgentConfig) -> String
```

---

## The caching rule

```
  DO NOT rebuild the prompt mid-conversation unless you intend to.

  Calling Agent::invalidate_system_prompt() is the correct way to
  trigger a rebuild. It sets cached_system_prompt = None; the next
  execute_loop() call will rebuild.

  Rebuilding unnecessarily:
    - Evicts Claude's system-prompt cache prefix (costs cache_write tokens)
    - Reloads all memory and skill files (disk I/O)
    - May change guidance mid-session if files changed
```

---

## Example: minimal and maximal prompts

**Minimal** (`edgecrab --toolset safe "what is 2+2"`):
```
  SOUL.md content
  Platform: cli
  Timestamp: ...
  (no context files found)
  (memory tools absent → no memory block)
  (skill tools absent → no skills block)
```

**Maximal** (full gateway session with all tools):
```
  SOUL.md content
  Platform: telegram
  Timestamp: ...
  .edgecrab.md project instructions
  AGENTS.md additional context
  Memory guidance
  [memory file 1]: key facts from past sessions
  [memory file 2]: code patterns
  Session search guidance
  Skills guidance
  Skills summary: 12 installed skills
  Cron scheduling guidance
  Messaging guidance
  Image analysis guidance
```

---

## Tips

> **Tip: Put project-specific instructions in `.edgecrab.md` at the repo root.**
> This file is guaranteed to be picked up before `AGENTS.md` or `CLAUDE.md`.
> It is ideal for code style rules, test commands, and architecture constraints.

> **Tip: Memory files in `~/.edgecrab/memories/` survive across all sessions.**
> Tools `memory_write` and `memory_read` operate on these files. Anything written
> there will appear in the system prompt of every subsequent session on this machine.

> **Tip: Test your context files with `edgecrab doctor`.**
> Doctor scans for context files, checks for injection patterns, and reports which
> files would be included in the system prompt for the current directory.

---

## FAQ

**Q: Can I add my own guidance block?**
Yes. Add a conditional block in `PromptBuilder::build()` keyed on whether
a specific tool name is in `tool_registry.tool_names()`. The block is injected
only when the tool is active.

**Q: Does changing `SOUL.md` require restarting EdgeCrab?**
No. Call `agent.invalidate_system_prompt()` (or `/refresh` in the TUI). The
next turn will rebuild from the updated file.

**Q: What happens if `SOUL.md` does not exist?**
The builder skips the identity slot silently. The LLM uses its default
persona. This is normal for quick one-shot invocations.

**Q: Are memory files size-limited?**
Not enforced in code, but large memory files contribute to the system prompt's
token count. If context compression starts triggering frequently, consider
trimming old memory entries.

---

## Cross-references

- Memory files location and format → [Memory and Skills](../007_memory_skills/001_memory_skills.md)
- Injection patterns checked → [Security](../011_security/001_security.md)
- When the prompt is rebuilt → [Conversation Loop](./002_conversation_loop.md)
- Skill file format → [Creating Skills](../007_memory_skills/002_creating_skills.md)
