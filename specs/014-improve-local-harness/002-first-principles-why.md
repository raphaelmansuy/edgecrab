# 002 — First Principles: WHY It Happens

Cross-ref: [003-official-references.md](./003-official-references.md) · [004-homelab-evidence.md](./004-homelab-evidence.md) · [005-code-anchors.md](./005-code-anchors.md)

---

## 1. One local tool turn is a single blocking HTTP transaction

EdgeCrab’s ReAct loop eventually calls:

```text
POST http://localhost:1234/v1/chat/completions
  messages: [...history..., tool results]
  tools:    [~30 tool schemas]
  tool_choice: required          (local policy)
  max_tokens: 2048               (local policy, mutation-aligned)
  reasoning_effort: none          (local policy, Qwen3)
  stream: false                   (local tool-turn policy)
```

**First principle:** The TUI “awaiting / negotiating” state is **one thread blocked on this HTTP response**. Tool handlers already returned.

```text
  EdgeCrab execute_loop                LM Studio server
  ─────────────────────                ────────────────
       │                                    │
       │  POST chat/completions             │
       │ ─────────────────────────────────► │
       │                                    │ Phase A: PREFILL
       │  (no bytes to client)              │  process ~34k prompt tokens
       │                                    │
       │                                    │ Phase B: GENERATE
       │  shelf: "composing tool call"      │  autoregress ≤ max_tokens
       │  LM Studio UI: GEN 1851/2048       │
       │ ◄───────────────────────────────── │
       │  200 OK + message.tool_calls?      │
       v                                    v
```

Official: LM Studio documents OpenAI-compatible `/v1/chat/completions` with `tools` and `max_tokens` ([Tool Use](https://lmstudio.ai/docs/developer/openai-compat/tools), [Chat Completions](https://lmstudio.ai/docs/developer/openai-compat/chat-completions)).

OpenAI semantics: `max_tokens` limits **generated** tokens, not prompt ([API reference](https://platform.openai.com/docs/api-reference/chat/create)). `finish_reason: "length"` means generation hit that cap ([finish_reason](https://platform.openai.com/docs/api-reference/chat/object#chat/object-choices)).

---

## 2. Three independent layers (do not conflate)

### Layer 1 — INPUT (prefill latency)

**WHY slow at 34–57k prompt:**

| Prompt component | ~Tokens (homelab PPT) | Shrinks how? |
|------------------|----------------------|--------------|
| System + skills + memory | ~12k | `/compress`, `/new` |
| Tool JSON schemas | ~8k | Toolset reduction (long-term) |
| `web_extract` / search bodies | ~10k+ | Structural prune / spill |
| Recovery + error messages | ~0.5–2k | Prune on length recovery |

**First principle:** Prefill cost grows with **prompt tokens**. It is normal for 34k to take tens of seconds on a 35B local model.

**Compression gap:** EdgeCrab compresses at `context_window × threshold` (default 50%). After LM Studio syncs **262k** ctx, threshold = **131k** — so **57k never compresses**. See [004-homelab-evidence.md](./004-homelab-evidence.md).

```text
  prompt tokens
  0        43.7k      57k              131k           262k
  ├─────────┼──────────┼─────────────────┼──────────────┤
            │ prefill   │ homelab failure │
            │ prune     │ band            │ LLM compress
            │ threshold │                 │ (never reached)
            └───────────┴─────────────────┘
```

### Layer 2 — OUTPUT (completion ceiling) ← binding at 34–37k

**WHY `finish_reason=length` with `tool_calls=[]`:**

A tool call is **generated text** — JSON in the `function.arguments` field. The entire completion (reasoning + tool JSON + any prose) shares **one** `max_tokens` budget.

Official / community consensus:

- OpenAI: `max_tokens` / `max_completion_tokens` caps **all** completion output including reasoning on reasoning models ([community: O4-mini empty + length](https://community.openai.com/t/o4-mini-returns-empty-response-because-reasoning-token-used-all-the-completion-token/1359002)).
- OpenAI community: truncating function JSON at `max_tokens` yields **ill-formed** arguments — parser may emit **no** `tool_calls` ([functions + max_tokens](https://community.openai.com/t/issue-with-max-tokens-in-case-of-using-functions/434887)).
- LM Studio: if tool calls cannot be parsed, content falls back to `message.content` ([Tool Use — parsing](https://lmstudio.ai/docs/developer/openai-compat/tools)).

EdgeCrab derives arg limit deterministically:

```text
  max_arg_bytes = min(config_limit, max_tokens × 4 chars/tok × 0.85)
                ≈ min(32 KiB, 6963 B)   for max_tokens=2048
```

See `mutation_turn_policy.rs` — [005-code-anchors.md](./005-code-anchors.md).

**WHY a full PPT Python script cannot succeed in one `write_file`:**

```text
  Desired: write_file(content = 8–15 KiB script)
  Pipe:    max_tokens=2048  →  ~6963 B arg budget
  Result:  model generates until token 2047
           JSON truncated / unparseable
           finish_reason=length, tool_calls=[]
           content_len=0 (homelab logs)
```

**First principle:** This is **geometry**, not flakiness. No amount of waiting completes an argument that needs more tokens than the pipe allows.

### Layer 3 — CONTROL (recovery without shrink)

**WHY the loop repeats:**

On length failure, harness injects `stream_interrupted_recovery_message` (~374 chars) and `continue`s the loop **without** shrinking history (pre-fix).

```text
  Turn N:   prompt=37,233  →  length, no tools
  inject:   +recovery user message
  Turn N+1: prompt=37,271  (+38)
  Turn N+2: prompt=37,358  (+87)
```

Pre-dispatch `check_tool_argument_budget` only runs **after** the API returns **parsed** `tool_calls`. When the API returns none, the guard never fires — failure is at **composition**, not dispatch.

---

## 3. Qwen3-specific: reasoning competes with tool JSON

Qwen3 family defaults to thinking/reasoning in many serving stacks ([Qwen function call docs](https://github.com/QwenLM/Qwen3/blob/main/docs/source/framework/function_call.md); Qwen3.5 notes on `enable_thinking` / `reasoning_effort` in [003-official-references.md](./003-official-references.md)).

**WHY `reasoning_effort: none` matters:**

If reasoning tokens consume the 2048 budget, **zero tokens remain** for tool JSON — same signature as OpenAI O4-mini reports (`content=""`, `finish_reason=length`).

Homelab: early failures showed `thinking_tokens=324`; post-wire-fix failures show `thinking_tokens=0` but **still** `2047` completion tokens and no tools — confirming Layer 2 dominates after reasoning is disabled.

---

## 4. Non-streaming is intentional (not the bug)

EdgeCrab forces non-streaming tool turns for `lmstudio` / `ollama` because:

1. Streaming tool args arrive in chunks; partial assembly fails on large payloads ([LM Studio streaming tools](https://lmstudio.ai/docs/developer/openai-compat/tools)).
2. Retrying after timeout starts a **second** server generation (LM Studio queues jobs).

See `prefers_nonstreaming_tool_turns` in [005-code-anchors.md](./005-code-anchors.md).

**Trade-off:** User sees silence until HTTP completes; shelf heartbeats + LM Studio GEN counter are the liveness signals.

---

## 5. Secondary failure: path semantics (`create_dirs`)

Homelab screenshot: `write_file tmp/pptx_builder.py` → `No such file or directory`; then `mkdir -p`.

**WHY:** `write_file` defaults `create_dirs=false`; parent `tmp/` must exist ([file_write.rs](../../crates/edgecrab-tools/src/tools/file_write.rs)).

This adds error round-trips to context and triggers **another** monolithic write attempt — amplifying Layer 2.

---

## 6. Unified causality diagram

```text
                    USER: "Build PPT"
                           │
                           v
              ┌────────────────────────┐
              │  Research tools fast   │  web_search, web_extract (1–4s each)
              │  Fat tool results      │  ~5k chars × N  →  prompt bloat
              └───────────┬────────────┘
                          v
              ┌────────────────────────┐
              │  Build phase           │  write_file / execute_code intended
              └───────────┬────────────┘
                          v
         ┌────────────────────────────────────┐
         │  Local tool turn (non-streaming)     │
         │  prompt 34–57k                       │
         └────────────────┬───────────────────┘
                          │
          ┌───────────────┼───────────────┐
          v               v               v
     [Prefill slow] [Generate 2048] [Parse tool_calls]
          │               │               │
          │               │               ├── OK → dispatch (fast)
          │               │               │
          │               └── hits length ──┤
          │                   no tools    │
          v                               v
     User waits                      Recovery msg (+tokens)
     30–187s                        Loop (Layer 3)
                                    unless prune / steer / incremental
```

---

## 7. What “fix” means in first-principles terms

| Goal | Mechanism | Layer | Verified plan |
|------|-----------|-------|---------------|
| Faster tool-turn API wait | Prune/spill fat tool outputs before HTTP | L1 | **P1a** LH-06..09 |
| Stop length/no-tool loops | Prune on length recovery + length-specific recovery text | L3 | **P1b** LH-11, **P2** LH-20 |
| Prevent reasoning eating budget | `reasoning_effort: none` + edgequake wire | L2 | **P0** LH-01, LH-10 |
| Never duplicate GEN on timeout | No transport retry (local) | control | **P0** LH-03 |
| Incremental mutation (operator/model) | Recovery text + pre-dispatch budget | L2 | LH-05 dispatch only |

**Verified plan (CI):** [006-solution-plan.md](./006-solution-plan.md) — P0–P5 complete. Optional docs backlog B6 only.
