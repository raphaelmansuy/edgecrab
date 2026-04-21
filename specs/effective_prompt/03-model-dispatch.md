# Model-Specific Guidance Dispatch Matrix

> How EdgeCrab, Hermes Agent, and Claude Code route guidance to different
> model families, and the failure modes each block mitigates.

---

## The Problem

LLM models from different providers have wildly different default behaviors:

```
User: "Write a summary of these documents to summary.md"

Claude (Anthropic): calls write_file("summary.md", ...) ✓

GPT-4o (OpenAI): "Here is a summary of the documents:
                  [provides summary in plain text, no tool call] ✗
                  (interprets ".md" as format hint, not filename)

Gemini Pro (Google): calls web_search → read_file → ... → stops after 3 tools
                     "I've gathered the information..." [never writes file] ✗

Llama-3 (Meta): "```markdown\n# Summary\n..." [code block, no tool] ✗
```

Without model-specific guidance, open-source and non-Anthropic models fail
on common tasks that Claude handles natively.

---

## Dispatch Architecture

```
PromptBuilder::build()
       │
       ▼
needs_tool_use_enforcement(model_str)?
       │
   ┌───┴──────────────────────────────────────────────────────────────┐
   │  Checks model_str.to_lowercase() against TOOL_USE_ENFORCEMENT   │
   │  MODELS list (20+ families):                                     │
   │  gpt, codex, gemini, gemma, grok, mistral, mixtral, qwen,       │
   │  llama, phi, deepseek, cohere, falcon, yi, solar, openchat,     │
   │  vicuna, wizardlm, hermes, nemotron, internlm, baichuan, chatglm │
   └───────────────────────────────────────────────────────────────────┘
       │
       ▼ (true)
   inject TOOL_USE_ENFORCEMENT_GUIDANCE  ← universal override block
       │
       ▼
model_specific_guidance(model_str)?
       │
   ┌───┴─────────────────────────────────────────────────────────────┐
   │  GPT / Codex family?  ──► OPENAI_MODEL_EXECUTION_GUIDANCE      │
   │  Gemini / Gemma?      ──► GOOGLE_MODEL_OPERATIONAL_GUIDANCE    │
   │  Any other?           ──► GENERIC_EXECUTION_GUIDANCE           │
   │  Anthropic Claude?    ──► None (no enforcement needed)         │
   └─────────────────────────────────────────────────────────────────┘
```

---

## Guidance Matrix by Model Family

### Universal: TOOL_USE_ENFORCEMENT_GUIDANCE

Applied to: all 20+ non-Anthropic model families

```
Failure mode: Model produces narration ("I will now call the search tool...") instead
              of actually calling it. Or says "I can help with that" and stops.

Fix: Explicit directive that the model MUST use tools when they are the
     appropriate action, with specific examples of correct vs. incorrect behavior.
```

### GPT / Codex: OPENAI_MODEL_EXECUTION_GUIDANCE

FP-number: FP35 (side_effect_verification block)

```
GPT failure modes patched:
  1. MAIN LOOP DRIFT: GPT-4o tends to run 2-3 tool calls then produce a verbose
     summary rather than continuing to completion. Block requires the model to
     keep looping until explicitly done.

  2. STATE SIDE EFFECTS (FP35): After read_file → analyze → modify, GPT-4o
     often produces the modified content inline without calling write_file,
     then says "I've made the changes". The <side_effect_verification> block
     requires explicit confirmation of state-changing operations.

  3. PLANNING PARALYSIS: GPT-4 family produces detailed multi-step plans before
     acting. Block requires acting immediately, planning in parallel if needed.

  4. FILE OUTPUT (FP34): GPT-family interprets "write X to output.md" as
     a formatting instruction rather than a write_file call.
     FILE_OUTPUT_ENFORCEMENT_GUIDANCE patches this separately.
```

### Gemini / Gemma: GOOGLE_MODEL_OPERATIONAL_GUIDANCE

```
Gemini failure modes patched:
  1. PREMATURE TOOL CHAIN TERMINATION: Gemini Pro starts a tool chain (search →
     read → analyze) but gives up after 3-4 tools with "Based on my research..."
     even when the task requires more steps. Block requires explicit task
     completion criteria before stopping.

  2. CONTEXT WINDOW MANAGEMENT: Gemini sometimes repeats large amounts of
     previously-seen content in its responses. Block constrains response length.

  3. TOOL RESULT HANDLING: Gemini occasionally ignores tool results and
     falls back to parametric knowledge. Block requires integrating tool results.
```

### Generic: GENERIC_EXECUTION_GUIDANCE (FP38)

```
Applied to: llama, mistral, phi, deepseek, qwen, yi, solar, openchat, vicuna,
            wizardlm, nemotron, internlm, baichuan, chatglm, and any unknown family

Patches the most universal failures:
  1. No tool calls despite tools being available
  2. Code-block responses instead of tool calls
  3. Early stopping without completing multi-step tasks
  4. Verbose narration mode

Does NOT include GPT/Gemini-specific failures (FP35, FP36) — those are
too model-family-specific and may confuse other families.
```

### Anthropic Claude: None

```
Claude (claude-3-*, claude-opus-*, claude-sonnet-*, etc.) does not receive
any enforcement blocks. Anthropic models:
  - Natively use tools as the primary action mechanism
  - Handle multi-step task completion reliably
  - Respect write_file semantics without guidance

Detection: model_str starts with "claude" OR model_str is empty
           (empty means model not yet known — skip enforcement to avoid
            injecting unnecessary tokens for Claude deployments)
```

---

## Hermes Agent Dispatch

```python
TOOL_USE_ENFORCEMENT_MODELS = ("gpt", "codex", "gemini", "gemma", "grok")
DEVELOPER_ROLE_MODELS = ("gpt-5", "codex")  # use 'developer' role instead of 'system'

if any(model.startswith(m) for m in TOOL_USE_ENFORCEMENT_MODELS):
    inject TOOL_USE_ENFORCEMENT_GUIDANCE
    
if model.startswith(("gpt", "codex")):
    inject OPENAI_MODEL_EXECUTION_GUIDANCE
elif model.startswith(("gemini", "gemma")):
    inject GOOGLE_MODEL_OPERATIONAL_GUIDANCE
```

5 families vs EdgeCrab's 20+ — Hermes is less comprehensive because it
primarily targets OpenAI and Google models via their native APIs. EdgeCrab
targets the full open-source model ecosystem via Ollama/LM Studio/etc.

---

## Claude Code Dispatch

Claude Code has no model dispatch because it only uses Anthropic Claude models.
Instead, it uses feature flags:

```typescript
feature('PROACTIVE')      // enables proactive suggestions
feature('KAIROS')         // timing-aware response system
feature('TOKEN_BUDGET')   // token budget tracking
feature('VERIFICATION_AGENT') // separate verification agent

process.env.USER_TYPE === 'ant'  // Anthropic-internal model launch guidance
```

This is not model dispatch but *capability* dispatch — different Claude variants
(Haiku, Sonnet, Opus) all get the same base prompt, with only Anthropic-internal
features gated on `USER_TYPE=ant`.

---

## Adding a New Model Family

1. Add the model name prefix to `TOOL_USE_ENFORCEMENT_MODELS` in `prompt_builder.rs`
2. If the family has unique failure modes, add a new `const MY_MODEL_GUIDANCE: &str`
3. Add a match arm in `model_specific_guidance()`:
   ```rust
   fn model_specific_guidance(model: &str) -> Option<&'static str> {
       let m = model.to_lowercase();
       if m.contains("mymodel") {
           return Some(MY_MODEL_GUIDANCE);
       }
       // ... existing arms
   }
   ```
4. Write a test in `#[cfg(test)] mod tests` verifying the dispatch
5. Document the failure mode patched with a FP-numbered comment

**FP numbering**: Use the next available FP number (check existing comments
in `prompt_builder.rs` for the highest current number).
