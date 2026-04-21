# Round 9 — File-Output Enforcement for Open-Source Models

> **Trigger:** User reports: "I must ask explicitly to write [the document]" — after
> instructing the agent to "make a deep research audit document in ./audit_quanta.md",
> the agent researched the topic but did NOT call `write_file`. Only after an explicit
> follow-up did the agent produce the file.
>
> **Model at time of failure:** `openrouter/openai/gpt-oss-20b:free`
>
> **Scope:** System prompt guidance gaps — file-output enforcement, open-source model
> coverage, task-completion semantic definition, research-to-file task pattern.
>
> **Reference:** Hermes Agent (`agent/prompt_builder.py`), Claude Code (`context.ts`,
> `query.ts`), EdgeCrab v0.7.0.
>
> **Baseline:** 361 tests passing after Round 8.

---

## Brutal Honest Root Cause Analysis

```
OBSERVATION: "Make a deep research audit document in ./audit_quanta.md"
              |
              v
EXPECTED:    search → fetch → compose → write_file("./audit_quanta.md") → DONE
ACTUAL:      search → fetch → compose → [prints content in response] → STOPPED
              |
              v
USER MUST:   "now actually write it to the file"
              |
              v
ACTUAL-2:    write_file("./audit_quanta.md") → DONE
```

**This is a hard failure: the agent did not execute the mandatory side-effect.**

---

## Why It Happens — First Principles

### Principle 1: Open-source models lack implicit tool-intent mapping

Claude (Anthropic) is RLHF-trained to map "write X to path.md" → `write_file(path, content)`.
Open-source models (gpt-oss, llama, mistral, qwen, phi) are trained predominantly on
text-prediction tasks. They model "write X.md" as a **format directive** (produce content
formatted as markdown), NOT as a **tool invocation directive**.

```
TRAINING DISTRIBUTION (open-source):
  "write a README.md" → 90% of examples: produce markdown in response
                      →  5% of examples: show bash `cat > file.md << EOF`
                      →  5% of examples: actual file write tool call
                         ^--- This 5% is insufficient to overcome the default

TRAINING DISTRIBUTION (Claude):
  RLHF + Constitutional AI fine-tuning → "write X to path" = tool call required
```

**Fix required: explicit, unambiguous system prompt rule.**

---

### Principle 2: The guidance is present but ambiguous for file-output tasks

Current `TOOL_USE_ENFORCEMENT_GUIDANCE`:
```
"When the user asks you to do something that requires a tool, call the tool immediately."
```

Problem: Open-source models do NOT classify "write a document at X.md" as
"something that requires a tool". They see it as "something that requires text output".
The word "requires" is model-interpretation-dependent.

Current `code_editing_guidance()` says:
```
"Create new files directly when the request requires them"
```

Problem: This block is conceptually scoped to "code or file CHANGES" in the LLM's mind.
A "research audit document" is NOT perceived as a "code change". The guidance is
injected (write_file IS in the tool list), but the model doesn't apply it to research tasks.

---

### Principle 3: No output-artifact semantic in the system prompt

The system prompt defines WHAT tools to use, but does NOT define:
```
WHEN is a file-output task DONE?
  → Done = write_file has been called with the specified path
  → NOT done = content produced in response without writing file
```

This missing semantic is fine for Claude (it knows natively), disastrous for open-source.

---

### Principle 4: Task-completion verification is uncoupled from side-effects

`OPENAI_MODEL_EXECUTION_GUIDANCE` has `<verification>`:
```
"does the output satisfy every stated requirement?"
```

An open-source model evaluates this and answers YES — because it DID produce content
matching the requirement "write an audit document". It doesn't know the requirement
includes "call write_file to persist it to disk".

---

### Principle 5: `gpt-oss` is NOT a GPT model — TOOL_USE_ENFORCEMENT_MODELS is correct
                  but OPENAI_MODEL_EXECUTION_GUIDANCE is wrong

`gpt-oss-20b:free` contains "gpt" → gets `OPENAI_MODEL_EXECUTION_GUIDANCE`.
But `OPENAI_MODEL_EXECUTION_GUIDANCE` was written for GPT-4/GPT-5 behavior patterns.
`gpt-oss-20b` is likely a community fine-tune that may not exhibit the same
"GPT reasoning discipline" the guidance targets.

More critically: open-source models that don't match ANY of the known families
(gpt, codex, gemini, gemma, grok, mistral, qwen, llama) get ZERO model-specific guidance.
Examples: `phi`, `deepseek`, `cohere`, `falcon`, `yi`, `solar`, `openchat`, `wizardlm`.

**Fix: ALL non-Anthropic models should fall through to a base execution guidance.**

---

## Gap Matrix (EdgeCrab vs Hermes vs Claude Code)

```
+----------------------------------+-------------+-------------+-------------+
| Gap                              | EdgeCrab    | Hermes      | Claude Code |
|                                  | v0.7.0      | (reference) |             |
+----------------------------------+-------------+-------------+-------------+
| Explicit file-output enforcement | MISSING     | MISSING     | N/A (Claude)|
| (path in request → write_file)   |             |             |             |
+----------------------------------+-------------+-------------+-------------+
| Open-source model fallback       | Partial     | Same gap    | N/A         |
| (non-Claude, non-GPT guidance)   | (only known |             |             |
|                                  | families)   |             |             |
+----------------------------------+-------------+-------------+-------------+
| Task-done definition for         | MISSING     | MISSING     | N/A (Claude)|
| file-output tasks                |             |             |             |
+----------------------------------+-------------+-------------+-------------+
| Research-to-file pattern         | MISSING     | MISSING     | N/A         |
| (gather → compose → write → ack) |             |             |             |
+----------------------------------+-------------+-------------+-------------+
| Anthropic model detection        | Correct:    | Correct     | Always      |
| (skip enforcement for Claude)    | !contains   |             | Claude      |
|                                  | patterns    |             |             |
+----------------------------------+-------------+-------------+-------------+
| OPENAI_MODEL_EXECUTION_GUIDANCE  | "gpt",      | "gpt",      | N/A         |
| trigger families                 | "codex",    | "codex",    |             |
|                                  | "o1","o3"   |             |             |
+----------------------------------+-------------+-------------+-------------+
| Unknown model family fallback    | MISSING     | MISSING     | N/A         |
| (phi, deepseek, cohere, etc.)    |             |             |             |
+----------------------------------+-------------+-------------+-------------+
| Verification block includes      | MISSING     | MISSING     | N/A         |
| "was write_file called?"         |             |             |             |
+----------------------------------+-------------+-------------+-------------+
```

**Hermes has the same gaps because it primarily uses Claude.**
**EdgeCrab is worse because it actively promotes open-source model use via OpenRouter.**

---

## Why This Matters More for EdgeCrab

```
EdgeCrab architecture:
  Users → OpenRouter → gpt-oss-20b:free / llama-3.3-70b / mistral-24b / etc.
                        ^--- ALL of these are affected by this gap

Hermes architecture:
  Users → Nous API → claude-opus-4.6 (default)
                      ^--- Claude handles natively

Claude Code:
  Users → Anthropic → claude-* only
                      ^--- Never an issue
```

EdgeCrab's strategic differentiator is open-source model support via OpenRouter.
This gap directly undermines that differentiator.

---

## First Principles Improvements (FP34–FP38)

### FP34 — FILE_OUTPUT_ENFORCEMENT_GUIDANCE (CRITICAL)

**Problem:** No system prompt rule maps "user named a file path as output destination"
→ "write_file is mandatory before task completion".

**Root cause:** Missing semantic definition. The agent knows HOW to write files but
not WHEN it is REQUIRED to do so for research/content tasks.

**Fix:** New constant `FILE_OUTPUT_ENFORCEMENT_GUIDANCE`, injected when `write_file`
is in the tool list. Three explicit rules:

1. **Path-in-request → write_file mandatory**: If the user's message contains a file
   path as output target, `write_file` MUST be called.
2. **Content-in-response is NOT delivery**: Printing content in the response is preparation,
   not delivery. The file path is where the user expects the content to land.
3. **Verify after write**: After `write_file`, report the actual path and byte count
   to confirm delivery.

**Applies to:** ALL models (even Claude, as a defensive belt-and-suspenders rule).
**Why all models:** The rule is semantically correct regardless of model family.
Defensive injection costs ~200 tokens but prevents the failure mode completely.

**Hermes gap:** Hermes has the same gap. This is a genuine improvement over Hermes.

---

### FP35 — Extended Verification for Side-Effect Tasks

**Problem:** `OPENAI_MODEL_EXECUTION_GUIDANCE`'s `<verification>` block checks:
"does the output satisfy every stated requirement?" — but doesn't define
"output" to include mandatory file writes.

**Fix:** Add a `<side_effect_verification>` sub-block:
```
After completing any task that required writing, saving, or creating a file:
- Confirm write_file was called (not just content was prepared in your reasoning).
- Check the write_file return value confirms success.
- Report the file path and size to the user.
```

**Applies to:** `OPENAI_MODEL_EXECUTION_GUIDANCE` (used by gpt, codex, o1-o4).
Also add to the new generic `GENERIC_EXECUTION_GUIDANCE` (FP38).

---

### FP36 — RESEARCH_TASK_GUIDANCE

**Problem:** The research-to-file pattern has no explicit guidance:
"gather information → compose document → write to path → confirm".

Open-source models interrupt this pipeline after "compose" — they deliver the
composed content in the response and consider themselves done.

**Fix:** New constant `RESEARCH_TASK_GUIDANCE` injected when BOTH `write_file`
AND any web/search tool (`web_search`, `fetch_url`, `browser_navigate`) are present.

Content:
```
When a task asks you to research a topic AND save the result to a file:
1. Gather information using search/fetch tools.
2. Compose the document content in your reasoning (not in your final response).
3. Call write_file with the full composed content. This is MANDATORY.
4. Your final response should only confirm: path written, size, what was produced.
   Do NOT include the full document content in your response text.
```

**Why rule 4 matters:** Dual output (response text + file) wastes tokens and creates
confusion. The authoritative output is the file.

---

### FP37 — Extended TOOL_USE_ENFORCEMENT_MODELS

**Problem:** TOOL_USE_ENFORCEMENT_MODELS covers: gpt, codex, gemini, gemma, grok,
mistral, qwen, llama. Missing families that need explicit enforcement:

| Model family | Example | Why needs enforcement |
|---|---|---|
| `phi` | microsoft/phi-4 | Tends to describe actions |
| `deepseek` | deepseek-chat-v3 | Mixed tool-use reliability |
| `cohere` | command-r-plus | Narration-first tendency |
| `falcon` | tiiuae/falcon-40b | Low tool-use RLHF |
| `yi` | 01-ai/yi-34b | Narration-first |
| `solar` | upstage/solar-10.7b | Limited tool-use training |
| `openchat` | openchat/openchat-3.5 | Community fine-tune |
| `vicuna` | lmsys/vicuna-13b | Community fine-tune |

**Fix:** Add these patterns to `TOOL_USE_ENFORCEMENT_MODELS`.

---

### FP38 — Generic Execution Guidance Fallback for Unknown Models

**Problem:** `model_specific_guidance()` returns `None` for unknown model families.
A model like `openrouter/upstage/solar-pro` or `openrouter/nousresearch/hermes-3-llama-3.1-405b`
gets TOOL_USE_ENFORCEMENT_GUIDANCE (from the catch-all) but no execution discipline block.

**Fix:** Add `GENERIC_EXECUTION_GUIDANCE` — a stripped-down version of
`OPENAI_MODEL_EXECUTION_GUIDANCE` without GPT-specific mentions. Used as the fallback
when no model-specific guidance matches.

`model_specific_guidance()` returns:
- `Some(OPENAI_MODEL_EXECUTION_GUIDANCE)` for gpt/codex/o-series
- `Some(GOOGLE_MODEL_OPERATIONAL_GUIDANCE)` for gemini/gemma
- `Some(GENERIC_EXECUTION_GUIDANCE)` for ALL other non-Anthropic models (NEW)
- `None` only for Anthropic/claude models

**How to detect Anthropic:** `lower.contains("claude") || lower.contains("anthropic")`.

---

## Implementation Plan

```
Files to change:
  crates/edgecrab-core/src/prompt_builder.rs  (main changes)

New constants:
  FILE_OUTPUT_ENFORCEMENT_GUIDANCE    (FP34)
  RESEARCH_TASK_GUIDANCE              (FP36)
  GENERIC_EXECUTION_GUIDANCE          (FP38)

Modified constants:
  OPENAI_MODEL_EXECUTION_GUIDANCE     (FP35: add <side_effect_verification>)
  TOOL_USE_ENFORCEMENT_MODELS         (FP37: add phi, deepseek, cohere, etc.)

Modified functions:
  model_specific_guidance()           (FP38: return GENERIC_EXECUTION_GUIDANCE as fallback)
  PromptBuilder::build()              (FP34, FP36: inject new guidance blocks)

New tests:
  file_output_enforcement_injected_when_write_file_present
  file_output_enforcement_not_injected_without_write_file
  research_task_guidance_injected_with_write_and_search
  research_task_guidance_not_injected_without_search
  generic_guidance_for_unknown_model
  no_guidance_for_anthropic_model
  tool_enforcement_for_phi_model
  tool_enforcement_for_deepseek_model
```

---

## Signal Cross-References

| This spec | Cross-refs |
|---|---|
| FP34 file-output | `code_editing_guidance()` (write_file already mentioned; FP34 is additive) |
| FP34 file-output | `TOOL_USE_ENFORCEMENT_GUIDANCE` (FP34 makes the enforcement concrete) |
| FP35 verification | `OPENAI_MODEL_EXECUTION_GUIDANCE` `<verification>` (extends it) |
| FP36 research task | `RESEARCH_TASK_GUIDANCE` complements `TASK_STATUS_GUIDANCE` |
| FP37 models | `TOOL_USE_ENFORCEMENT_MODELS` slice (additive) |
| FP38 fallback | `model_specific_guidance()` return contract |
| FP38 fallback | Rounds 1-8: all tool-use enforcement was model-family specific; FP38 closes the gap |

---

## DRY / SOLID Checklist

- [x] **DRY**: `FILE_OUTPUT_ENFORCEMENT_GUIDANCE` is one constant, referenced in one
  injection point. No duplication with `code_editing_guidance()` (different scope).
- [x] **Single Responsibility**: Each constant addresses one failure mode.
- [x] **Open/Closed**: `TOOL_USE_ENFORCEMENT_MODELS` is extended, not rewritten.
  `model_specific_guidance()` gets a new arm, existing arms unchanged.
- [x] **Liskov/Interface**: `PromptBuilder::build()` contract unchanged; new blocks
  are additive injections controlled by existing `has_tool()` pattern.
- [x] **Defense-in-depth**: FP34 + FP35 + FP36 are independent layers. Even if one
  is ignored by the model, others may catch it.

---

## Edge Cases

1. **User asks to write to STDOUT (no file path):** FP34 detection pattern looks for
   a file path in the request. No path → FP34 rule doesn't fire. No false positive.

2. **User asks for code change AND a doc file:** Both `code_editing_guidance()` and
   `FILE_OUTPUT_ENFORCEMENT_GUIDANCE` apply. No conflict — they address different
   failure modes. Total prompt cost: ~400 extra tokens, acceptable.

3. **Anthropic Claude gets FP34:** Yes, by design. The rule is semantically correct.
   Claude already does this, so the extra text is redundant but not harmful.
   Alternative (skip for Anthropic) saves ~200 tokens but risks missing edge cases.

4. **Model string unknown at build time:** `model_str.is_empty()` → tool enforcement
   is injected (existing behavior), generic guidance is also injected (FP38 new).
   Conservative fail-safe.

5. **Very long system prompt:** Each new block is ~200-400 tokens. Total new tokens:
   ~800-1200. For a 128K context window this is 0.9% overhead — acceptable.
   For extremely constrained models (4K context), consider a `--compact-system-prompt`
   mode in a future round.
