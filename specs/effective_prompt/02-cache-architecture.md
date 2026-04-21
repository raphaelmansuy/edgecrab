# LLM Cache Architecture for Agent Prompts

> **Key insight**: Anthropic's prompt caching reduces costs 10× for cached tokens.
> EdgeCrab's `prompt_builder.rs` now produces `PromptBlocks` that expose a stable
> prefix and a dynamic suffix, enabling the provider layer to set `cache_control`.

---

## How Anthropic Prompt Caching Works

```
API Request:
┌─────────────────────────────────────────────────────────────┐
│ system: [                                                   │
│   {                                                         │
│     type: "text",                                           │
│     text: "...stable 10K tokens...",                        │
│     cache_control: { type: "ephemeral" }  ← write to cache │
│   },                                                        │
│   {                                                         │
│     type: "text",                                           │
│     text: "...dynamic 2K tokens..."       ← no cache       │
│   }                                                         │
│ ]                                                           │
└─────────────────────────────────────────────────────────────┘

Turn 1: cache WRITE  — 10K stable tokens cached ($3.75/Mtok write cost)
Turn 2: cache READ   — 10K tokens read from cache ($0.30/Mtok = 10× cheaper)
Turn 3: cache READ   — same
...
Turn N: cache READ   — cache stays hot as long as prefix is identical
```

Cache is invalidated when:
1. Any byte in the stable prefix changes
2. The `cache_control` position changes in the array
3. The model is changed
4. The TTL expires (5 min default, 1h with `ttl: "1h"` beta)

---

## Claude Code's Static/Dynamic Boundary Architecture

```
getSystemPrompt() returns string[] (array of sections):

STATIC ZONE (scope: 'global', cross-org cacheable)
┌──────────────────────────────────────────────────────────────┐
│  getSimpleIntroSection()        │ identity                   │
│  getSimpleSystemSection()       │ behavioral rules           │
│  getSimpleDoingTasksSection()   │ task execution rules       │
│  getActionsSection()            │ action guidelines          │
│  getUsingYourToolsSection()     │ tool usage rules           │
│  getSimpleToneAndStyleSection() │ output format              │
│  getOutputEfficiencySection()   │ token efficiency rules     │
└──────────────────────────────────────────────────────────────┘
         │
         ▼  SYSTEM_PROMPT_DYNAMIC_BOUNDARY  marker
         │  ('__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__')
         │
DYNAMIC ZONE (per-session, ephemeral cache or no cache)
┌──────────────────────────────────────────────────────────────┐
│  session_guidance               │ memoized per conversation  │
│  memory                         │ memoized per conversation  │
│  env_info_simple                │ memoized per conversation  │
│  language                       │ memoized per conversation  │
│  output_style                   │ memoized per conversation  │
│  mcp_instructions               │ ← DANGEROUS: rebuilt/turn  │
│  scratchpad                     │ memoized per conversation  │
│  frc                            │ memoized per conversation  │
└──────────────────────────────────────────────────────────────┘
```

**`systemPromptSection(name, fn)`** — memoized. The `fn` is called ONCE per conversation
and the result stored in `STATE.systemPromptSectionCache`. Subsequent turns reuse the cached
string. Invalidated only by `/clear` or `/compact`.

**`DANGEROUS_uncachedSystemPromptSection(name, fn, reason)`** — recomputed EVERY turn.
The `reason` string is mandatory documentation of WHY this section breaks cache.
Currently only `mcp_instructions` uses this (reason: "MCP servers can connect/disconnect
between turns, so the tool list can change").

---

## Hermes Agent Cache Architecture

```
Two-layer skills cache:

Layer 1: In-process LRU (OrderedDict, max 8 entries)
  Key: (skills_dir, external_dirs_tuple, sorted_tools, sorted_toolsets, platform_hint)
  Value: skills_prompt string
  Eviction: LRU when max_size reached

Layer 2: Disk snapshot (~/.hermes/.skills_prompt_snapshot.json)
  Validated by: mtime + size manifest for each skills file
  Invalidation: any file mtime or size change invalidates the snapshot
  Purpose: survives process restart

System prompt is rebuilt:
  - First turn of each conversation
  - After explicit /compress (context compression)
  - After /clear
  NOT rebuilt mid-conversation (preserves Anthropic cache)
```

**Hermes limitation**: The timestamp is at position ~4/10 in the prompt, meaning
the entire rest of the prompt (skills, memory, etc.) is never Anthropic-cached.
This is a known issue and the reason Claude Code moved datetime to the dynamic section.

---

## EdgeCrab Cache Architecture (After Optimization)

```
PromptBlocks struct:
┌──────────────────────────────────────────────────────────────┐
│  stable: String                                              │
│    ┌────────────────────────────────────────────────────┐   │
│    │ Identity + Platform + Enforcement + ModelGuidance  │   │
│    │ + all behavioral constants (gated by has_tool())   │   │
│    │ ≈ 8,000 – 12,000 tokens (highly stable)           │   │
│    └────────────────────────────────────────────────────┘   │
│                                                              │
│  dynamic: String                                             │
│    ┌────────────────────────────────────────────────────┐   │
│    │ DateTime + Session ID + Model name                 │   │
│    │ + Execution environment (cwd, allowed paths)       │   │
│    │ + Context files (AGENTS.md, .edgecrab.md)         │   │
│    │ + Memory sections (~/.edgecrab/memories/)          │   │
│    │ + Skills prompt (~/.edgecrab/skills/)              │   │
│    │ ≈ 1,000 – 5,000 tokens (session-specific)        │   │
│    └────────────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────────────────┘

build_blocks() returns PromptBlocks
build()        returns String (stable ++ "\n\n" ++ dynamic) — backward compat
```

### Provider Integration (Future)

When the LLMProvider trait is updated to accept `Vec<SystemBlock>`:

```rust
// Provider sends:
vec![
    SystemBlock {
        text: prompt_blocks.stable.clone(),
        cache_control: Some(CacheControl::Ephemeral),  // ← Anthropic caches this
    },
    SystemBlock {
        text: prompt_blocks.dynamic.clone(),
        cache_control: None,
    },
]
```

Until the provider is updated, `build()` returns a single string (stable + dynamic)
which is still correct — it just misses the caching optimization. The split is implemented
now so providers can opt in without touching `prompt_builder.rs`.

---

## Skills Cache: TTL → Manifest Upgrade

```
BEFORE (TTL-based):
  cache key: (home_path, platform)
  invalidation: 60 seconds after last build
  problem: cache can be stale for 60s after skill install/remove

AFTER (Manifest-based):
  cache key: (home_path, platform)
  invalidation: when any skills file mtime OR size changes
  validation: mtime+size manifest stored with the cache entry
  inspiration: Hermes Agent's _build_skills_manifest() pattern

SkillsCacheEntry {
    summary: String,
    disabled_at_build: Vec<String>,
    built_at: Instant,           ← kept for max-age fallback
    manifest: SkillsManifest,    ← NEW: invalidate on file changes
}

SkillsManifest {
    entries: HashMap<PathBuf, ManifestEntry>,
}

ManifestEntry {
    mtime_secs: u64,
    size_bytes: u64,
}
```

This ensures the skills prompt reflects the actual state of the skills directory
with zero latency after `invalidate_skills_cache()` — no 60-second wait.

---

## Cache Cost Model

For a typical EdgeCrab session with Anthropic claude-sonnet-4-5:

```
Prompt composition:
  stable section:  ≈ 10,000 tokens (identity + all guidance constants)
  dynamic section: ≈  3,000 tokens (datetime + context + memory + skills)
  Total per turn:  ≈ 13,000 tokens

WITHOUT cache_control breakpoint:
  Every turn: 13,000 tokens @ $3.00/Mtok = $0.039 per turn
  50 turns:   $0.039 × 50 = $1.95 in prompt read costs

WITH cache_control breakpoint on stable section:
  Turn 1 (write): 10,000 tokens @ $3.75/Mtok = $0.0375 (cache write)
                 + 3,000 tokens @ $3.00/Mtok = $0.009  (dynamic, uncached)
  Turns 2-50 (read): 10,000 tokens @ $0.30/Mtok = $0.003 per turn
                    + 3,000 tokens @ $3.00/Mtok = $0.009 per turn
                    = $0.012 per turn × 49 turns = $0.588

  Total WITH: $0.0375 + $0.009 + $0.588 = $0.6345
  Total WITHOUT: $1.95

  Savings: 67% on prompt input costs for a 50-turn session.
```

The reordering change (moving datetime out of the stable prefix) is thus worth
approximately a 2/3 reduction in prompt token costs for Anthropic users.
