# First Principles of Effective Agent Prompt Engineering

> **From**: Analysis of Hermes Agent, Claude Code, and EdgeCrab architectures
> **For**: Engineers building or modifying EdgeCrab's prompt builder

---

## The Three Axes

Every prompt engineering decision trades off between three axes:

```
          RELIABILITY
              ▲
              │
              │     sweet
              │     spot
              │   ╔══════╗
              │   ║      ║
              ╟───╫──────╫───────► COST
              │   ║      ║
              │   ╚══════╝
              │
              └────────────────► STABILITY
```

| Axis | What breaks it | How to protect it |
|------|---------------|-------------------|
| **Reliability** | Model ignores guidance; wrong tool selected; hallucinated tool call | Model-specific enforcement blocks; explicit tool-selection rules; behavioral constants |
| **Cost** | Cache misses; redundant sections; over-long prompts | Static/dynamic split; section memoization; tool-gated injection |
| **Stability** | Volatile content early in prompt; timestamp near top; random section order | Composition order (see doc 06); DYNAMIC_BOUNDARY marker |

---

## First Principle 1 — The Stable Prefix Law

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ AXIOM: Every byte that changes in the prompt prefix invalidates     │
  │        ALL subsequent prompt caching for that session turn.         │
  │                                                                      │
  │ COROLLARY: Put the most volatile content LAST, not first.           │
  └──────────────────────────────────────────────────────────────────────┘
```

**Evidence from the codebases:**

- *Claude Code* uses `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` to mark where stable ends and dynamic begins.
  Everything before the marker gets `scope: 'global'` (cross-org Anthropic caching, 1h+ TTL).
  Everything after gets per-session ephemeral cache or no cache at all.

- *Hermes Agent* assembles the prompt in this order: identity → platform → model guidance →
  tool enforcement → date/time → context files → memory → skills.
  The **date/time** is near position 4 out of ~10 — every byte from position 4 onward
  is never cached because timestamps change every second.

- *EdgeCrab* (before this fix) mirrored Hermes' ordering: timestamp at position 3/18.
  After the fix (see doc 07), timestamp moves to position 6/18, after all static guidance.

**Quantified impact** (Anthropic prompt caching costs as of 2025):

```
Uncached prompt read:     $3.00 / Mtok   (Claude Sonnet)
Cached prompt read:       $0.30 / Mtok   (10× cheaper)
Cache write:              $3.75 / Mtok   (paid once, amortized)

Stable prefix = 10,000 tokens
Session = 50 turns

Cost WITHOUT fix: 50 × $3.00 × 10,000 / 1,000,000 = $1.50
Cost WITH fix:    $3.75 × 10,000/1,000,000           = $0.0375 (write once)
              + 50 × $0.30 × 10,000/1,000,000         = $0.15
              Total WITH fix:                           = $0.1875

Savings: 87% on the stable-prefix portion of the prompt.
```

---

## First Principle 2 — Tool-Gated Guidance

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ AXIOM: Guidance about a tool that isn't loaded is wasted tokens.    │
  │        Worse: it causes the model to hallucinate that tool calls.   │
  └──────────────────────────────────────────────────────────────────────┘
```

All three codebases implement this:

- *Hermes*: `if "memory_write" in tool_names: inject MEMORY_GUIDANCE`
- *Claude Code*: `feature('KAIROS')` gates certain sections; `mcp_instructions` only
  injected when MCP servers are connected
- *EdgeCrab*: `has_tool("memory_write")`, `has_tool("session_search")`, etc.

**Rule**: Every guidance constant must have a corresponding `has_tool()` guard unless it
is a universal behavioral directive (identity, platform, tool enforcement).

---

## First Principle 3 — Model-Specific Failure Modes

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ AXIOM: Each model family has distinct failure modes that require     │
  │        tailored prompt blocks. One-size-fits-all prompts leave      │
  │        recoverable failure modes unmitigated.                       │
  └──────────────────────────────────────────────────────────────────────┘
```

Observed failure modes and their mitigations:

| Model Family | Failure Mode | Mitigation Block |
|-------------|-------------|-----------------|
| GPT / Codex | Narrates instead of acting; creates `output.md` verbally | `TOOL_USE_ENFORCEMENT_GUIDANCE` + `OPENAI_MODEL_EXECUTION_GUIDANCE` |
| GPT (FP35) | Executes commands that modify state without verification | `<side_effect_verification>` block in `OPENAI_MODEL_EXECUTION_GUIDANCE` |
| Gemini / Gemma | Starts tool chains, then gives up mid-chain | `GOOGLE_MODEL_OPERATIONAL_GUIDANCE` with persistence rules |
| Llama / Mistral | Ignores `write_file` in favour of inline code blocks | `FILE_OUTPUT_ENFORCEMENT_GUIDANCE` |
| All open-source | Treats "write X to path" as format hint, not tool call | `RESEARCH_TASK_GUIDANCE` |
| Qwen3 / Llama3 | Picks `browser_vision` for local image files | `VISION_GUIDANCE` with explicit decision rule |
| Anthropic Claude | Handles all of the above natively | No enforcement blocks needed |

---

## First Principle 4 — Security as a First-Class Concern

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ AXIOM: Any content injected into the system prompt that originates  │
  │        from outside the binary is a potential injection vector.     │
  │                                                                      │
  │ ALL such content MUST be scanned before injection.                  │
  └──────────────────────────────────────────────────────────────────────┘
```

Injection surfaces:
1. **Context files** (AGENTS.md, .edgecrab.md, SOUL.md) — scanned by `scan_for_injection()`
2. **Memory files** (~/.edgecrab/memories/) — written by the agent itself, should be re-scanned
3. **Skills** (~/.edgecrab/skills/) — scanned by `skills_guard::scan_skill()` at install time
4. **MCP server outputs** — treated as untrusted; never injected into system prompt directly

See doc [05-injection-security.md](05-injection-security.md) for the full threat model.

---

## First Principle 5 — Compression Must Not Invalidate the Stable Prefix

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │ AXIOM: Context compression reshapes the message HISTORY, not the    │
  │        system prompt. Rebuilding the system prompt during compression│
  │        busts the entire Anthropic prompt cache.                     │
  └──────────────────────────────────────────────────────────────────────┘
```

*Claude Code* explicitly documents: "The system prompt is assembled once per session and
cached in `STATE.systemPromptSectionCache`. Do NOT rebuild or mutate it mid-conversation."

*EdgeCrab* follows the same contract: `session.cached_system_prompt` is immutable after the
first turn. The only exception is FP33 (appending a compression note), which is
a targeted append, not a full rebuild.

---

## Summary: The Ideal Prompt Assembly Algorithm

```
1. BUILD_STABLE_PREFIX:
   a. Identity (SOUL.md → DEFAULT_IDENTITY)
   b. Platform hint (Telegram / CLI / Discord / ...)
   c. Tool-use enforcement (non-Anthropic families only)
   d. Model-specific execution guidance (GPT / Gemini / generic)
   e. All behavioral constants (gated by has_tool()):
      - MEMORY_GUIDANCE
      - SESSION_SEARCH_GUIDANCE
      - TASK_STATUS_GUIDANCE + PROGRESSION_GUIDANCE
      - SKILLS_GUIDANCE
      - SCHEDULING_GUIDANCE
      - MESSAGE_DELIVERY_GUIDANCE
      - MOA_GUIDANCE
      - VISION_GUIDANCE
      - LSP_GUIDANCE
      - code_editing_guidance()
      - FILE_OUTPUT_ENFORCEMENT_GUIDANCE
      - RESEARCH_TASK_GUIDANCE
   ← STATIC/DYNAMIC BOUNDARY (cache here with cache_control: ephemeral) →
   
2. BUILD_DYNAMIC_SECTION:
   f. Date/time stamp + session ID + model name
   g. Execution environment (cwd, allowed paths — changes per session)
   h. Context files (AGENTS.md, .edgecrab.md — changes with project)
   i. Memory sections (user memories — grows over time)
   j. Skills prompt (installed skills — changes after skill install/remove)

3. CACHE:
   - Pass stable_prefix as first system block with cache_control: ephemeral
   - Pass dynamic_section as second system block (no cache_control)
   - NEVER rebuild stable_prefix mid-conversation
```

This algorithm is implemented in EdgeCrab via `PromptBuilder::build_blocks()` — see
[07-implementation.md](07-implementation.md) for the Rust code.
