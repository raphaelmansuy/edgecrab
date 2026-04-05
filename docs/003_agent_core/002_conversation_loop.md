# Conversation Loop 🦀

> **Verified against:** `crates/edgecrab-core/src/conversation.rs`

---

## Why the loop matters

The ReAct (Reason + Act) pattern is the conceptual foundation, but understanding
the actual code loop is what lets you debug agent behaviour. When EdgeCrab ignores
a tool result, loops unexpectedly, or runs out of budget, the answer is always
inside `execute_loop`.

**Reference:** [ReAct: Synergizing Reasoning and Acting in Language Models](https://arxiv.org/abs/2210.03629)

---

## Loop constants (from source)

```rust
// conversation.rs
const MAX_RETRIES: u32 = 3;
const BASE_BACKOFF: Duration = Duration::from_millis(500);
const SKILL_REFLECTION_THRESHOLD: u32 = 5;  // learning reflection fires when ≥5 tool calls in a turn
```

---

## Full annotated loop

```
  execute_loop(user_message, ...)
  ══════════════════════════════════════════════════════════════

  [SETUP]
    snapshot config + provider  (RwLock read, drop guard before await)
    resolve cwd and enabled toolsets
    reset CancellationToken for this turn

  ──────────────────────────────────────────────────────────────

  [EXPANSION]
    expand_context_refs(user_message)
      "@./src/lib.rs" → inline file content
      "@http://..."   → fetched page
      "@session:id"   → session search result

  ──────────────────────────────────────────────────────────────

  [FIRST TURN: build system prompt]
    if cached_system_prompt is None:
      PromptBuilder::build()
        → SOUL.md / EDGECRAB.md / AGENTS.md / CLAUDE.md
        → memory file sections (if skip_memory=false)
        → skill summaries (if skills exist)
        → tool-specific guidance blocks
        → injection-check all external content
      store in SessionState::cached_system_prompt

  ──────────────────────────────────────────────────────────────

  LOOP (up to max_iterations = 90):
  ┌──────────────────────────────────────────────────────────┐
  │                                                          │
  │  [BUDGET CHECK]                                          │
  │    budget.try_consume() → false → BudgetExhausted        │
  │                                                          │
  │  [CANCEL CHECK]                                          │
  │    is_cancelled() → true → break with interrupted=true   │
  │                                                          │
  │  [ROUTING]  (if smart routing enabled)                   │
  │    classify_message(last_user_msg)                       │
  │    TurnRoute::Cheap  → swap to cheap_model               │
  │    TurnRoute::Primary → keep primary model               │
  │                                                          │
  │  [COMPRESSION]                                           │
  │    check_compression_status(messages, params, ctx_len)   │
  │    PressureWarning → emit StreamEvent::ContextPressure   │
  │    Compressed      → compress_with_llm() → 5-pass pipe   │
  │                                                          │
  │  [PROVIDER CALL]   (up to MAX_RETRIES=3, backoff 500ms)  │
  │    provider.chat(messages, tools, streaming)             │
  │                                                          │
  │    RateLimited → sleep(retry_after_ms) → retry           │
  │    ContextLimit → trigger compression → retry            │
  │    Other error  → AgentError propagates to caller        │
  │                                                          │
  │  ┌─────────────────────────────────────────────┐         │
  │  │ Response: tool_calls?               │ text? │         │
  │  └────────────────┬──────────────────────┬──────┘         │
  │                   │ YES                  │ NO             │
  │  ┌────────────────▼──────┐   ┌───────────▼──────────┐    │
  │  │  TOOL DISPATCH        │   │  FINAL RESPONSE      │    │
  │  │                       │   │                      │    │
  │  │  for each tool_call:  │   │  emit Token events   │    │
  │  │   1. security check   │   │  extract reasoning   │    │
  │  │   2. approval gate    │   │  trim <think> tags   │    │
  │  │   3. resolve toolset  │   │                      │    │
  │  │   4. emit ToolExec    │   │  persist session     │    │
  │  │   5. execute()        │   │  to SQLite           │    │
  │  │   6. emit ToolDone    │   │                      │    │
  │  │   7. append result    │   │  return              │    │
  │  │      to messages      │   │  ConversationResult  │    │
  │  └────────────────┬──────┘   └──────────────────────┘    │
  │                   │ loop ◄─────────────────────────────   │
  │                   │                                       │
  └──────────────────-┼───────────────────────────────────────┘

  [POST-LOOP]
    if tool_call_count >= SKILL_REFLECTION_THRESHOLD (5):
      learning_reflection()  ← closed learning loop
    persist to SQLite (if state_db present)
    return ConversationResult
```

---

## Tool dispatch in detail

Each tool in a response is dispatched sequentially by default.
`parallel_safe=true` tools may be dispatched concurrently
(see [Concurrency Model](../002_architecture/003_concurrency_model.md)).

For each tool call:

```
  1. security gate
       edgecrab-security::command_scan (for terminal tool)
       edgecrab-security::path_jail    (for file tools)

  2. approval gate
       ApprovalPolicy::check(tool_name, args, session_id)
       if needs_approval:
         emit StreamEvent::Approval { command, tx }
         await user response on oneshot channel
         Once / Session / Always / Deny

  3. ToolRegistry::dispatch(name, args, ctx)
       exact match → handler.check_fn(&ctx) → handler.execute(args, &ctx)
       no match    → fuzzy match (Levenshtein ≤ 3) → ToolError::NotFound

  4. result handling
       Ok(string)   → Message::tool_result(id, name, string) → append
       Err(ToolError) → serialise to ToolErrorResponse JSON → append
                        (model reads it, adapts next iteration)
```

---

## Termination conditions

| Condition | How it exits | `ConversationResult` field |
|---|---|---|
| Model returns text (no tool calls) | `break` with response | `final_response` |
| Iteration budget exhausted | `break` from budget check | `budget_exhausted = true` |
| User cancellation | `break` from cancel check | `interrupted = true` |
| Max retries exceeded | `Err(AgentError::Llm)` propagated | — |
| Compression failed 3× | `Err(AgentError::CompressionFailed)` | — |

---

## Message history invariant

The conversation history always uses the OpenAI-compatible message format:

```
  system     (built by PromptBuilder, cached)
  user       (original user message)
  assistant  (model response, may contain tool_calls)
  tool       (tool result, one per tool_call)
  tool       (...)
  assistant  (next model response)
  ...
```

This shape is what compression, persistence, and recovery all rely on.
Breaking it — e.g., appending an `assistant` message immediately after
another `assistant` — produces provider API errors.

---

## `ConversationResult`

```rust
pub struct ConversationResult {
    pub final_response:   String,
    pub messages:         Vec<Message>,   // full turn history
    pub session_id:       String,
    pub api_calls:        u32,
    pub interrupted:      bool,
    pub budget_exhausted: bool,
    pub model:            String,
    pub usage:            Usage,           // input/output/cache/reasoning tokens
    pub cost:             Cost,            // USD estimated cost
    pub tool_errors:      Vec<ToolErrorRecord>,  // all failures this turn
}
```

---

## Learning reflection (≥5 tool calls)

When a turn uses 5 or more tool calls, `learning_reflection()` runs:

```
  turn ends with tool_call_count >= 5
        │
        ▼
  learning_reflection(messages, model, provider)
        │
        ▼
  LLM call: "What patterns should inform future skill creation?"
        │
        ▼
  Optional: write learnings to ~/.edgecrab/memories/session_insights.md
```

This implements a closed learning loop: long, complex turns teach EdgeCrab
about useful patterns without requiring explicit user instruction.

🦀 *`hermes-agent` and OpenClaw never did this. EdgeCrab gets smarter from every
combat session automatically.*

---

## Debugging tips

> **Tip: Enable `RUST_LOG=edgecrab_core=debug` to trace every iteration.**
> Each budget check, routing decision, compression trigger, and tool dispatch
> emits a structured log entry.

> **Tip: `ConversationResult::tool_errors` is your post-mortem log.**
> Every failed tool call is recorded with the exact arguments it was called with.
> If the agent seemed to "give up" or do something unexpected, inspect this first.

> **Tip: `budget_exhausted = true` means the model needed more than 90 iterations.**
> Either the task is genuinely complex (raise `max_iterations` in config), or the
> model is in a loop (check `tool_errors` for repeated identical calls).

---

## FAQ

**Q: Why does the loop retry the provider up to 3 times?**
Transient API errors (rate limits, network blips) are common in production.
Exponential backoff (500 ms base, doubling) handles most of them without
bothering the user. After 3 failures the error propagates.

**Q: Can I add a custom hook between iterations?**
Yes. `StreamEvent::HookEvent { event, context_json }` is emitted at key
points. Implement a native hook in `edgecrab-gateway/src/hooks.rs` or a
file-based script hook in `~/.edgecrab/hooks/`. See [Hooks](../hooks.md).

**Q: Does the loop support multi-step tool chains? E.g. search → read → write?**
Yes. Each iteration appends tool results and re-calls the model. The model
naturally chains: "I searched and found X, now I'll read Y, now I'll write Z"
across multiple iterations. Each step costs one iteration of the budget.

---

## Cross-references

- `Agent` struct that owns the loop → [Agent Struct](./001_agent_struct.md)
- Context compression triggered in the loop → [Context Compression](./004_context_compression.md)
- Smart model routing called from the loop → [Smart Model Routing](./005_smart_model_routing.md)
- Tool dispatch implementation → [Tool Registry](../004_tools_system/001_tool_registry.md)
- Error variants handled in the loop → [Error Handling](../002_architecture/004_error_handling.md)
