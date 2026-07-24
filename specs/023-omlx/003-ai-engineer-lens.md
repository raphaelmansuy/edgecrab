# 003 — AI Engineer Lens (Harness Physics)

**Focus:** How oMLX behaves inside the ReAct loop, tool turns, streaming, discovery, and recovery — not the menu bar app.

---

## 1. Why oMLX matters for agent harnesses

Coding agents **invalidate prefixes constantly** (tool results, re-prompts, steers). oMLX’s SSD-tier KV cache is built for that pattern. EdgeCrab’s local harness already assumes:

| Physics | EdgeCrab policy today | oMLX implication |
|---------|----------------------|------------------|
| Server keeps generating after client timeout | `blocks_transport_retry` | **Must include omlx** |
| Large tool JSON buffers poorly under SSE | `prefers_nonstreaming_tool_turns` | **Must include omlx** |
| Prefill is slow on large context | prefill prune + structural compress | Same thresholds apply |
| Completion budget must leave room for tools | local `max_tokens` + arg ceilings | Same mutation policy |
| Reasoning burns budget | `reasoning_effort: none` on tool turns | Apply when model is reasoning-family |

**Hermes signal:** local endpoints disable stale-stream detectors for 300s+ prefill. EdgeCrab uses long HTTP timeouts + progress labels — oMLX must get the same treatment (`OMLX_TIMEOUT_SECONDS`, default 600).

---

## 2. Wire protocol choice (agent-facing)

```text
EdgeCrab ReAct ──OpenAI messages/tools──▶ edgequake-llm OmlxProvider
                                              │
                                              ▼
                                    POST {host}/v1/chat/completions
                                    GET  {host}/v1/models
                                    POST {host}/v1/embeddings   (optional)
```

| Mode | P0 | Notes |
|------|----|-------|
| Chat + tools | **Yes** | Primary agent path |
| Streaming assistant text | **Yes** | Prefer non-stream when tools present (local policy) |
| Vision content parts | **P1** | When VLM loaded; capability may be model-dependent |
| Anthropic `/v1/messages` | **P1** | Not required for EdgeCrab loop |
| Rerank | **Out** | Separate feature |

---

## 3. Tool-calling contract

oMLX supports multiple tool markup families (JSON, Qwen, Gemma, GLM, MiniMax, …). EdgeCrab must **not** parse those formats itself when using OpenAI-compatible mode — it relies on the server returning OpenAI-shaped `tool_calls`.

| Risk | Mitigation |
|------|------------|
| Server returns prose + raw XML tools | Local `tool_choice` + non-stream + schema annotation (existing local path) |
| Model lacks tools in template | Discovery may not know; user-facing error + catalog note |
| Oversized tool results | Existing artifact spill + tool result trim (server also trims) |

**Harness gate (must):**

```text
local_tool_harness_active("omlx", has_tools=true) == true
```

Includes: schema annotation for OpenAI-compatible validators, mutation turn ceilings, write_create_dirs auto-on.

---

## 4. Discovery strategy

| Property | Value |
|----------|--------|
| Strategy | **Dynamic** (local) |
| Endpoint | `GET {OMLX_HOST}/v1/models` |
| Cache TTL | Local TTL (same as ollama/lmstudio in `model_discovery.rs`) |
| Empty list | Server up but no models / unreachable → empty live + static seed |
| Id opacity | Preserve full id including `owner/name` paths and `name:profile` |

**Do not** strip after first `/` in the model id — oMLX and HF-style ids often contain `/`. Selector format is:

```text
omlx/<opaque-model-id>
```

`ModelCatalog::resolve_spec_lenient` already supports multi-segment models for lmstudio; oMLX must use the same path.

---

## 5. Context, compression, goals

| Concern | Behavior |
|---------|----------|
| Context window | Prefer live metadata if exposed; else catalog default (128k seed) or provider-reported |
| Compression | Existing loop; local structural mid-band still applies |
| Goals / steers | Unchanged — inject into messages, not system prompt |
| Prompt cache (Anthropic) | N/A for oMLX OpenAI path; local SSD cache is **server-side** |

**Important:** Server-side KV cache is invisible to EdgeCrab. Do not disable client-side compression assuming infinite free prefill — memory and latency still matter.

---

## 6. Streaming & cancellation

| Event | Correct behavior |
|-------|------------------|
| User interrupt | Cancel client request; **do not** auto-retry (orphan gen) |
| Timeout | Surface stall notice with oMLX-specific copy |
| Stream stall mid-tool | Prefer non-stream tool path so less common |
| Hot-swap model mid-session | Rebuild provider via factory; new model id |

---

## 7. Aux routing (vision / judge / mixture)

| Subsystem | oMLX role |
|-----------|-----------|
| Primary chat | First-class |
| Vision tool | Prefer native if model is VLM and multimodal parts accepted; else aux vision (Hermes tests use explicit `supports_vision` for omlx) |
| Shadow judge / MoA | May point members at `omlx/...` via same factory |
| Image gen | **Not** oMLX (different subsystem) |

Add `omlx` to vision provider normalization; capability detection should be **conservative** (unknown → no vision claim) unless discovery or catalog flags it.

---

## 8. Failover & routing

| Case | Policy |
|------|--------|
| oMLX down | Fail clearly; optional user-configured fallback model (existing router) |
| Rate limit | Unlikely; treat as local error |
| Context exceeded | Existing length recovery / compress |
| Wrong model id | 404-like from server → actionable message |

Do **not** auto-fallback oMLX → cloud without user config (privacy violation).

---

## 9. Observability signals

Emit/preserve:

- `provider=omlx` in traces / usage  
- model id full string  
- latency, prompt/completion tokens if returned  
- local harness activation log once per process  

Redact `OMLX_API_KEY` always.

---

## 10. AI Engineer acceptance tests (behavioral)

| # | Behavior |
|---|----------|
| AE-T1 | Tool turn on oMLX uses non-streaming path when tools present |
| AE-T2 | Timeout does not issue a second concurrent chat request |
| AE-T3 | Discovered `model:profile` selectable and sent as model field unchanged |
| AE-T4 | Prefill prune triggers under large local context (same as lmstudio) |
| AE-T5 | Zero cost recorded for successful turns |
| AE-T6 | Multi-iteration ReAct (≥3 tool rounds) completes on a tool-capable MLX model |

See [009-e2e-test-plan.md](009-e2e-test-plan.md) for harness details.
