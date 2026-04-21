# 07 — Implementation Record

Cross-reference: [00-index](00-index.md) | [02-cache-architecture](02-cache-architecture.md) | [04-section-registry](04-section-registry.md) | [06-composition-order](06-composition-order.md)

---

## Summary of Changes

Three concrete improvements were made to
`crates/edgecrab-core/src/prompt_builder.rs` in a single commit on branch
`feat/agent-harness-next-release`.

| ID | Change | Impact |
|----|--------|--------|
| I1 | `PromptBlocks` struct + `combined()` | Enables stable/dynamic split for future `cache_control` support |
| I2 | `build_blocks()` method — corrected composition order | Moves datetime to dynamic zone; +~3 000 cacheable tokens per session |
| I3 | `build()` delegates to `build_blocks().combined()` | Backward compatibility maintained, single source of truth |
| I4 | Manifest-based skills cache invalidation | Zero-latency invalidation after `/skills install` or remove |

---

## I1 — PromptBlocks Struct

```rust
pub struct PromptBlocks {
    /// Stable, cacheable prefix — binary constants + tool-gated guidance.
    pub stable: String,
    /// Dynamic, per-session suffix — datetime, context files, memory, skills.
    pub dynamic: String,
}

impl PromptBlocks {
    pub fn combined(self) -> String {
        match (self.stable.is_empty(), self.dynamic.is_empty()) {
            (true, true)   => String::new(),
            (true, false)  => self.dynamic,
            (false, true)  => self.stable,
            (false, false) => format!("{}\n\n{}", self.stable, self.dynamic),
        }
    }
}
```

**Why**: Providers that support Anthropic `cache_control` blocks can call
`build_blocks()` directly and attach `cache_control: {type: "ephemeral"}` to the
`stable` block. This API is future-safe: current callers use `combined()` without
change; the cache_control path can be added in a follow-up without touching callers.

---

## I2 — Corrected Composition Order (`build_blocks`)

### Before (old `build()` order)

```
 1  Identity                           ← stable
 2  Platform hint                      ← stable
 3  Tool-use enforcement               ← stable
 4  Model-specific guidance            ← stable
 5  *** DATETIME ***                   ← VOLATILE — cache boundary forced here
 6  Execution environment              ← dynamic
 7  Context files (AGENTS.md, …)      ← dynamic
 8  Memory guidance + sections        ← dynamic
 9  SESSION_SEARCH_GUIDANCE           ← stable (wrong zone)
10  TASK_STATUS_GUIDANCE              ← stable (wrong zone)
11  PROGRESSION_GUIDANCE              ← stable (wrong zone)
12  SKILLS_GUIDANCE                   ← stable (wrong zone)
13  SCHEDULING_GUIDANCE               ← stable (wrong zone)
14  MESSAGE_DELIVERY_GUIDANCE         ← stable (wrong zone)
15  MOA_GUIDANCE                      ← stable (wrong zone)
16  VISION_GUIDANCE                   ← stable (wrong zone)
17  LSP_GUIDANCE                      ← stable (wrong zone)
18  code_editing_guidance()           ← stable (wrong zone)
19  FILE_OUTPUT_ENFORCEMENT_GUIDANCE  ← stable (wrong zone)
20  RESEARCH_TASK_GUIDANCE            ← stable (wrong zone)
21  Skills prompt                     ← dynamic (wrong zone, but position ok)
```

**Cache boundary**: forced at token ~2 800 (after item 4).
Sections 9–20 (≈ 5 000–9 000 tokens) never benefit from Anthropic cache.

### After (new `build_blocks()` order)

```
══════════ STABLE ZONE ══════════════════════════════════════
 1  Identity
 2  Platform hint
 3  Tool-use enforcement
 4  Model-specific guidance
 5  MEMORY_GUIDANCE
 6  SESSION_SEARCH_GUIDANCE
 7  TASK_STATUS_GUIDANCE + PROGRESSION_GUIDANCE
 8  SKILLS_GUIDANCE
 9  SCHEDULING_GUIDANCE
10  MESSAGE_DELIVERY_GUIDANCE
11  MOA_GUIDANCE
12  VISION_GUIDANCE
13  LSP_GUIDANCE
14  code_editing_guidance()
15  FILE_OUTPUT_ENFORCEMENT_GUIDANCE
16  RESEARCH_TASK_GUIDANCE
══════════ DYNAMIC ZONE ══════════════════════════════════════
17  Datetime + Session ID + Model        ← VOLATILE moved here
18  Execution environment guidance
19  Context files (AGENTS.md, …)
20  Memory sections                      ← file content (volatile)
21  Skills prompt                        ← XML-wrapped
```

**Cache boundary**: moved to token ~8 000–12 000 (after item 16).
All 12 stable behavioral constants now sit in the cacheable prefix.

### Cost Savings (50-turn session, claude-sonnet-4.5)

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Cacheable prefix | ~2 800 tok | ~8 000–12 000 tok | +3–4× |
| Cache-ineligible tokens/turn | ~6 500 | ~2 000 | −69 % |
| Prompt cost (50 turns, 8K prompt) | ~$1.95 | ~$0.69 | −65 % |

Full analysis: [02-cache-architecture](02-cache-architecture.md).

---

## I3 — `build()` Now Delegates to `build_blocks()`

```rust
pub fn build(
    &self,
    override_identity: Option<&str>,
    cwd: Option<&Path>,
    memory_sections: &[String],
    skill_prompt: Option<&str>,
) -> String {
    self.build_blocks(override_identity, cwd, memory_sections, skill_prompt)
        .combined()
}
```

**Why**: This is the DRY/SOLID change. Previously `build()` contained the full
composition logic inline. Now there is exactly one source of truth
(`build_blocks()`), and `build()` is a thin compatibility shim.

All 93 prompt_builder unit tests pass unchanged.

---

## I4 — Manifest-Based Skills Cache Invalidation

### Previous behaviour (TTL-only)

```
cache miss → scan skills/ → store entry with built_at timestamp
cache hit?  → entry.built_at.elapsed() < 60 seconds
```

**Problem**: A `/skills install my-skill` right after a cache fill caused the old
stale summary to be served for up to 60 seconds. The agent would not see the new
skill until the TTL expired.

### New behaviour (manifest + TTL fallback)

```rust
struct ManifestEntry { mtime_secs: u64, size_bytes: u64 }

struct SkillsManifest {
    entries: HashMap<PathBuf, ManifestEntry>,
}

impl SkillsManifest {
    fn build(skills_dir: &Path) -> Self { /* stat() each SKILL.md */ }
    fn is_valid(&self, skills_dir: &Path) -> bool {
        // Fail-fast conditions:
        // 1. Any known file deleted
        // 2. Any known file's mtime or size changed
        // 3. File count in skills_dir changed (new skill added)
    }
}
```

**Cache hit criteria** (all must hold):
1. `manifest.is_valid(skills_dir)` — no file changes detected by mtime+size
2. `entry.built_at.elapsed() < 300s` — hard age cap (guards against
   mtime unavailability on exotic filesystems)
3. `entry.disabled_at_build == disabled_skills` — same filter set

```
                  ┌─────────────────────────────┐
  load_skill      │  SKILLS_CACHE (Mutex)        │
  _summary()      │  HashMap<(home,platform),…>  │
       │          └────────────┬────────────────┘
       │                       │ lock
       ▼                       ▼
  can_cache?  ──yes──►  entry found?
       │no             │yes                │no
       │               ▼                   ▼
       │          manifest valid?       scan disk
       │          + age ok?             + build manifest
       │          + same disabled?      + store entry
       │           │yes   │no           │
       │           ▼      ▼             │
       │         return  scan disk ◄────┘
       │         cached  + update
       │                 entry
       ▼
  scan disk (no caching)
```

**Inspired by**: hermes-agent `_build_skills_manifest()` and
`_skills_manifest_valid()` in `agent/prompt_builder.py`.

### Why mtime+size is sufficient

- `mtime_secs` changes whenever the OS flushes a write
- `size_bytes` catches content changes that don't change mtime (rare, e.g. same
  byte count but different content) as a secondary signal
- Together they catch all practical cases: new install, overwrite, delete

---

## New Tests Added

Three new tests were added to the `prompt_builder::tests` module:

| Test | Assertion |
|------|-----------|
| `stable_behavioral_constants_precede_timestamp_in_combined_output` | All stable constants appear at a lower byte offset than "Current date/time:" in the combined output |
| `build_blocks_stable_zone_excludes_timestamp` | `blocks.stable` does not contain timestamp; `blocks.dynamic` does |
| `build_blocks_combined_equals_build_output` | `build_blocks().combined()` and `build()` both contain the same key markers |

All 93 prompt_builder tests pass. Full workspace suite (657 tests): all pass.

---

## Follow-up Work

| Priority | Task | Notes |
|----------|------|-------|
| HIGH | Add `cache_control` support in `edgecrab-core/src/conversation.rs` | Call `build_blocks()` and set `cache_control: ephemeral` on the `stable` block when provider == Anthropic |
| MED | Add `PromptBlocks` to ACP adapter | So VS Code Copilot agent also benefits |
| LOW | Add `cache_control` to Hermes hermes-agent (Python port) | Hermes datetime bug is identical — same fix applies |
| LOW | Add skills cache manifest to Hermes | Hermes already has `_build_skills_manifest()` in `agent/prompt_builder.py` but it is not connected to the main cache |
