# Effective Prompt Engineering for AI Agents — Specification Index

> **Scope**: First-principles analysis of prompt architecture in Hermes Agent (Python),
> Claude Code (TypeScript), and EdgeCrab (Rust). Derives the most effective patterns for
> EdgeCrab and documents the implementation.
>
> **Audience**: Engineers modifying `edgecrab-core/src/prompt_builder.rs`

---

## Document Map

```
specs/effective_prompt/
 00-index.md              ← YOU ARE HERE — master index + cross-ref map
 01-first-principles.md   ← Why prompts matter; stability, cost, reliability axes
 02-cache-architecture.md ← LLM cache strategy: static/dynamic split, breakpoints
 03-model-dispatch.md     ← Model-specific guidance dispatch matrix
 04-section-registry.md   ← Memoized section registry pattern (Claude Code → EdgeCrab)
 05-injection-security.md ← Prompt injection mitigations across all three codebases
 06-composition-order.md  ← Section ordering for maximum cache prefix stability
 07-implementation.md     ← Concrete changes made to prompt_builder.rs
```

---

## Cross-Reference Map

```
  CONCEPT                   Hermes Agent (Py)         Claude Code (TS)           EdgeCrab (Rust)
  ─────────────────────────────────────────────────────────────────────────────────────────────
  Static/dynamic split      No explicit split         SYSTEM_PROMPT_DYNAMIC      FP-BOUNDARY added
                                                       _BOUNDARY marker           (07-implementation)

  Section memoization       None                      systemPromptSection()      PromptBlocks added
                                                       (Map<name,string>)         (07-implementation)

  Volatile sections         None                      DANGEROUS_uncached         DateTime + ctx files
                                                       SystemPromptSection()      (06-composition-order)

  Model dispatch            5 families                feature() flags +          20+ families,
                            TOOL_USE_ENFORCEMENT       USER_TYPE=ant              FP-numbered (03-model-dispatch)

  Skills cache              2-layer: LRU +            Not applicable             TTL 60s → manifest
                            disk manifest              (no skills concept)        upgrade (05, 07)

  Injection scanning        10 regex patterns         None (Anthropic only)      14 patterns +
                            + 9 invisible chars                                   homoglyphs (05)

  Composition order         datetime near top         datetime in dynamic        FIXED: datetime
                            (breaks cache prefix)      section (after boundary)   moved to dynamic
                                                                                  section (06, 07)

  Context files             .hermes.md / HERMES.md    Not loaded (clean build)   .edgecrab.md /
                            AGENTS.md, CLAUDE.md,                                AGENTS.md / SOUL.md
                            .cursorrules                                          .cursor/rules/

  Cache API integration     N/A (LiteLLM handles)     cache_control:{type:       PromptBlocks struct
                                                       ephemeral,scope:global}    ready for API (04)
```

---

## Quick-Start Checklist for Prompt Changes

Before modifying `prompt_builder.rs`, ask:

1. **Is this section stable across sessions?**
   - Yes → place it in the STABLE zone (before the DYNAMIC_BOUNDARY comment)
   - No  → place it in the DYNAMIC zone (after the DYNAMIC_BOUNDARY comment)

2. **Should this section be memoized per conversation?**
   - Yes → use `PromptBlocks::stable_section(name, content)` (rebuilds only on `/clear`)
   - No  → use `PromptBlocks::volatile_section(name, content)` (rebuilds each session)
   - Rebuild every turn → document the reason with a `VOLATILE_REASON:` comment

3. **Does this section depend on tool availability?**
   - Yes → gate with `has_tool("tool_name")` before adding the section
   - See: §09-11 in `build()` for existing examples

4. **Does this section contain user-sourced content?**
   - Yes → run `scan_for_injection()` before injecting
   - See: context file loading in `build()` for the pattern

5. **Does changing this section break Anthropic prompt caching?**
   - Yes → reconsider. Each cache miss costs ≈10× the read price for that section
   - Rule: one DYNAMIC_BOUNDARY per session; sections after it are uncached

---

## Key Constants (edgecrab-core/src/prompt_builder.rs)

| Constant | Purpose | Zone |
|----------|---------|------|
| `DEFAULT_IDENTITY` | Agent's baseline persona | STABLE |
| `TOOL_USE_ENFORCEMENT_GUIDANCE` | Non-Anthropic model action discipline | STABLE |
| `OPENAI_MODEL_EXECUTION_GUIDANCE` | GPT-family execution rules | STABLE |
| `GOOGLE_MODEL_OPERATIONAL_GUIDANCE` | Gemini-family rules | STABLE |
| `MEMORY_GUIDANCE` | memory_write tool instructions | STABLE |
| `SESSION_SEARCH_GUIDANCE` | session_search tool instructions | STABLE |
| `TASK_STATUS_GUIDANCE` | report_task_status instructions | STABLE |
| `PROGRESSION_GUIDANCE` | iteration discipline | STABLE |
| `SCHEDULING_GUIDANCE` | manage_cron_jobs instructions | STABLE |
| `MESSAGE_DELIVERY_GUIDANCE` | send_message instructions | STABLE |
| `MOA_GUIDANCE` | moa tool instructions | STABLE |
| `VISION_GUIDANCE` | vision_analyze vs browser_vision | STABLE |
| `LSP_GUIDANCE` | LSP semantic-navigation | STABLE |
| `code_editing_guidance()` | Code mutation rules (fn, not const) | STABLE |
| `FILE_OUTPUT_ENFORCEMENT_GUIDANCE` | write_file discipline | STABLE |
| `RESEARCH_TASK_GUIDANCE` | research→write pattern | STABLE |
| `SKILLS_GUIDANCE` | skill_manage instructions | STABLE |
| DateTime block | Current date/time + session ID | DYNAMIC |
| Context files | AGENTS.md, .edgecrab.md, etc. | DYNAMIC |
| Memory sections | ~/.edgecrab/memories/ content | DYNAMIC |
| Skills prompt | ~/.edgecrab/skills/ summary | DYNAMIC |
