# Memoized Section Registry Pattern

> Derived from Claude Code's `systemPromptSection` / `DANGEROUS_uncachedSystemPromptSection`
> architecture. Documents the EdgeCrab `PromptBlocks` implementation.

---

## The Problem with Single-String Prompts

All three codebases started with (or still use) a single-string prompt assembly:

```python
# Hermes: one big string
def _build_system_prompt(...) -> str:
    parts = []
    parts.append(DEFAULT_IDENTITY)
    parts.append(platform_hint)
    parts.append(str(datetime.now()))  ← volatile, breaks caching
    parts.append(memory_sections)
    return "\n\n".join(parts)
```

Problems:
1. No section-level granularity — can't mark individual sections as cacheable
2. No memoization — every turn rebuilds from scratch even if nothing changed
3. No explicit volatility annotation — future engineers don't know which sections break cache

---

## Claude Code's Registry Pattern

```typescript
// ─── Memoized (stable per conversation) ───────────────────────────────
function systemPromptSection(name: string, compute: () => string | null): string | null {
    const cache = STATE.systemPromptSectionCache;
    if (cache.has(name)) {
        return cache.get(name) ?? null;
    }
    const result = compute();
    cache.set(name, result ?? null);
    return result;
}

// ─── Volatile (rebuilt every turn) ────────────────────────────────────
function DANGEROUS_uncachedSystemPromptSection(
    name: string, 
    compute: () => string | null,
    reason: string        ← mandatory documentation
): string | null {
    // INTENTIONALLY does not cache — always calls compute()
    return compute();
}

// ─── Usage ────────────────────────────────────────────────────────────
const sections = [
    // STABLE — memoized
    systemPromptSection('session_guidance', () => getSessionGuidance()),
    systemPromptSection('memory', () => getMemorySections()),
    
    // VOLATILE — rebuilt every turn because MCP can connect/disconnect
    DANGEROUS_uncachedSystemPromptSection(
        'mcp_instructions',
        () => getMcpInstructions(),
        'MCP servers can connect/disconnect between turns'
    ),
];
```

**Key insight**: The `DANGEROUS_` prefix is a code-review forcing function. Any
engineer adding a new `DANGEROUS_uncachedSystemPromptSection` must document WHY
it's volatile. This prevents silent cache-breaking additions.

---

## EdgeCrab's PromptBlocks Implementation

```
EdgeCrab doesn't have a per-section registry (too complex for the current 
single-threaded session model), but does expose stable/dynamic split:

PromptBlocks {
    stable: String,   // all static guidance — can be Anthropic-cached
    dynamic: String,  // datetime + context + memory + skills — per session
}

build_blocks() → PromptBlocks   // for cache-aware providers
build()        → String          // stable + "\n\n" + dynamic — backward compat
```

### How PromptBlocks Maps to Claude Code's Registry

```
Claude Code registry concept    EdgeCrab equivalent
─────────────────────────────   ─────────────────────────────────────────────
systemPromptSection             All content in PromptBlocks::stable
  (memoized per conversation)   (built once per session, same semantics)

DANGEROUS_uncachedSystemPrompt  No direct equivalent — EdgeCrab rebuilds the
  Section (volatile, per turn)  entire prompt only once per session anyway.
                                 If per-turn rebuilds were needed, volatile
                                 content would go in a separate "turn_header"
                                 rather than the system prompt.

STATE.systemPromptSectionCache  session.cached_system_prompt (in conversation.rs)
  (per-conversation Map)         (String — entire prompt frozen after first turn)
```

---

## Section Classification Table

```
SECTION                      CLASSIFICATION   REASON IF VOLATILE
─────────────────────────────────────────────────────────────────
DEFAULT_IDENTITY              STABLE          Never changes (binary constant)
platform_hint()               STABLE          Fixed per session (CLI, Telegram, etc.)
TOOL_USE_ENFORCEMENT          STABLE          Fixed per session (model family)
model_specific_guidance()     STABLE          Fixed per session (model family)
MEMORY_GUIDANCE               STABLE          Text is a binary constant
SESSION_SEARCH_GUIDANCE       STABLE          Text is a binary constant
TASK_STATUS_GUIDANCE          STABLE          Text is a binary constant
PROGRESSION_GUIDANCE          STABLE          Text is a binary constant
SCHEDULING_GUIDANCE           STABLE          Text is a binary constant
MESSAGE_DELIVERY_GUIDANCE     STABLE          Text is a binary constant
MOA_GUIDANCE                  STABLE          Text is a binary constant
VISION_GUIDANCE               STABLE          Text is a binary constant
LSP_GUIDANCE                  STABLE          Text is a binary constant
code_editing_guidance()       STABLE          Fn result is deterministic (constant params)
FILE_OUTPUT_ENFORCEMENT       STABLE          Text is a binary constant
RESEARCH_TASK_GUIDANCE        STABLE          Text is a binary constant
SKILLS_GUIDANCE               STABLE          Text is a binary constant

DateTime block                DYNAMIC         Changes each session (current time)
execution_environment         DYNAMIC         cwd changes per session
Context files                 DYNAMIC         AGENTS.md content changes with project
Memory sections               DYNAMIC         ~/.edgecrab/memories/ grows over time
Skills prompt                 DYNAMIC         ~/.edgecrab/skills/ changes with installs
Personality addon             DYNAMIC         config.personality_addon per session
```

---

## Volatility Rules for New Sections

When adding a new section to `prompt_builder.rs`:

```
Decision tree:

Does the section content change between sessions?
│
├── NO  → STABLE zone (add before DYNAMIC_BOUNDARY comment in build_blocks())
│         Example: behavioral constants, model-specific guidance
│
└── YES → Does it change between turns within a session?
           │
           ├── NO  → DYNAMIC zone (add after DYNAMIC_BOUNDARY comment)
           │         Example: datetime, context files, memory
           │         These are already "stable" in EdgeCrab's model since
           │         the prompt is built once per session.
           │
           └── YES → Per-turn injection (NOT in system prompt)
                     Inject as a user message prefix instead.
                     Example: MCP tool list changes (Claude Code's case)
                     In EdgeCrab: add to the user message, not system prompt.
                     DOCUMENT with: // VOLATILE_REASON: <why>
```

---

## Why EdgeCrab Uses PromptBlocks Instead of a Full Registry

Claude Code's registry pattern (`Map<string, string | null>`) is suitable because:
- Claude Code makes per-turn API calls with fresh system prompts
- Different sections need different TTLs
- MCP server connections change between turns

EdgeCrab's model is simpler:
- System prompt is built ONCE per session (first turn)
- No per-turn rebuilds except for compression notes (FP33)
- Skills cache is the only content that needs invalidation between sessions

Therefore, a two-zone split (stable + dynamic) captures 95% of the value of
Claude Code's registry with 5% of the complexity.

The `PromptBlocks` struct is the right abstraction for EdgeCrab:
1. Enables cache_control breakpoint for Anthropic providers
2. Doesn't require changing the `build()` API (backward compatible)
3. Makes the stable/dynamic split explicit and testable
