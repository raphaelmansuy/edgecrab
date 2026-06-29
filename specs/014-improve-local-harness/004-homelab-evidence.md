# 004 — Homelab Evidence (Logs + Sessions)

Cross-ref: [002-first-principles-why.md](./002-first-principles-why.md) · [005-code-anchors.md](./005-code-anchors.md)

**Profile:** `~/.edgecrab/profiles/homelab`  
**Logs:** `logs/agent.jsonl` (last updated 2026-06-14 16:40 local)  
**State DB:** `state.db`

---

## 1. Task under study

From `history` and session titles:

```text
  "What is the best AI models to run on M4 Max 128Go for coding as june 2026"
  "Can you create a nice and sofisticated PPT presentation on the topic"
```

Partial artifact: `tmp/files/local_llm_ppt.py` (python-pptx scaffold, ~100+ lines).

---

## 2. Length-failure signature (structured logs)

Target: `edgecrab::local_llm` · message: `max_tokens exhausted without tool_calls`

**Count on 2026-06-14:** 14 events (10 logged under updated message format after 08:08 UTC).

### Invariant fields (every event)

| Field | Value | Meaning |
|-------|-------|---------|
| `finish_reason` | `length` | Hit `max_tokens` ([OpenAI semantics](./003-official-references.md)) |
| `completion_tokens` | `2047` | One token shy of cap — generation exhausted |
| `max_tokens` | `2048` | Local policy cap |
| `tool_choice_required` | `true` | Model instructed to emit tools |
| `tool_call_count` | `0` (implicit) | LM Studio returned no parseable `tool_calls` |

### Prompt token timeline (selected)

```text
  UTC time     prompt_tokens  thinking_tokens  content_len  Notes
  ─────────────────────────────────────────────────────────────────
  07:52:43        59,669           519            —       pre-reasoning-fix band
  08:08:15        56,902           324           119       reasoning still present
  08:14:34        57,214            77           171       +87 tok/failure loop
  08:19:41        50,877             0            82       reasoning=0
  08:35:33        39,673             0             0       worst: empty content
  08:38:21        37,233             0             0       refutes "only 57k problem"
  08:39:30        37,271             0             0       +38 tok recovery
  08:40:31        37,358             0            50       +87 tok recovery
```

**First-principles conclusion:** After `thinking_tokens=0`, failures persist → **output geometry (Layer 2)**, not reasoning alone.

### Inter-failure gaps (blocked HTTP time)

Sum of gaps between consecutive length failures ≈ **43 minutes** on 2026-06-14 morning pass.

Individual gaps: **61s – 584s** — consistent with prefill + 2048-token generation on local 35B.

---

## 3. Compression never fired in failure band

Warnings (same day):

```json
{"message":"context approaching compression threshold",
 "estimated_tokens":55688,"threshold_tokens":64000}
```

```text
  estimated 55–57k  <  threshold 64k (128k window × 0.5)
  estimated 37–57k  << threshold 131k (262k synced × 0.5)
```

**WHY compress missed:** LM Studio metadata sync raises `context_window` to 262k → compress threshold jumps to 131k while research prompts stay ~37–57k.

Cross-ref: `CompressionParams` + `refresh_model_metadata` in [005-code-anchors.md](./005-code-anchors.md).

---

## 4. Prefill prune (implemented) — not observed in logs yet

No log lines matching `local_llm: structural prefill prune` through 16:40.

**Interpretations:**

1. Binary running in homelab may predate harness merge, **or**
2. Prompt **37k < 43,690** threshold → preflight prune correctly **does not fire**

See `should_structural_prefill_prune` tests in `local_provider_policy.rs`.

---

## 5. Session archaeology (SQLite)

**PPT sessions on 2026-06-14:** 7 sessions, same title prefix.

| Session ID (prefix) | Started (UTC) | Msgs | Tool calls | Input tokens (cum.) |
|---------------------|---------------|------|------------|---------------------|
| `fbac111d` | 07:48 | 53 | 29 | 1,281,847 |
| `1a2e0f0b` | 08:03 | 34 | 18 | 749,246 |
| `f3000f53` | 08:17 | 24 | 12 | 520,759 |
| `3e2ace8c` | 08:32 | 0 | 0 | 0 (`/new`) |

**Aggregate PPT attempts:** ~**4.2M** cumulative input tokens, **110** tool calls — high re-prefill cost, low deliverable progress.

### Tool mix (`fbac111d` — heaviest session)

```text
  web_extract  ████████  8
  terminal     ███████   7
  web_search   ████      4
  read_file    ████      4
  execute_code ███       3
  write_file   █         1   ← only one write in 29 tool calls
```

**WHY stuck in research/build loop:** Model spends tool budget on web I/O; build phase hits Layer 2 on first large mutation.

### Message-level (`f3000f53`)

```text
  1. user
  2–5. web_search results (~2k each)
  6–12. web_extract results (~5k each)
  14. read_file (~16k chars — skill/template)
  16. terminal (pip install)
  17–18,23–24. recovery user msgs (~374 chars each)
  20. write_file OK (107 B ack — tiny scaffold)
  22. read_file (ppt script)
```

Recovery text (verbatim from DB):

```text
[System: Your previous tool-call draft was interrupted ... Keep each tool call under 6963 bytes (~2048 completion tokens).]
```

---

## 6. GEN=0 — schema rejection (2026-06-14, gemma-4-e4b)

**Symptom:** LM Studio **GEN 0** for 26–88s+; EdgeCrab shelf shows `composing tool call`.

**Log signature (40× in `agent.jsonl`):**

```text
lmstudio API 400 — invalid_union_discriminator
path: [N, "function", "parameters", "type"]
Expected 'object'
```

**Cause:** `patch` tool exported top-level `oneOf` instead of `type: "object"`. LM Studio Zod validator rejects the request **before** the model runs.

**Fix:** P6 — flat object schema + `openai_compatible_tool_parameters()` safety net (**LH-64**).

**Distinguish from slow prefill:** After fix, GEN climbs within seconds once prefill completes; 400 errors disappear from logs.

---

## 7. Screenshot correlation (2026-06-14 ~16:35 local)

| UI signal | Evidence interpretation |
|-----------|-------------------------|
| `~34k/262k ctx` | Below prefill prune (43.7k); moderate prefill |
| `composing tool call 24–38s (non-streaming)` | Phase A+B; normal for local, not tool stall |
| `(non-str 187s)` | Single HTTP call elapsed (heartbeat) |
| LM Studio **GEN 1,851 tok** | Near **2048 cliff** — high probability of next length failure |
| `write tmp/pptx_builder.py` failed | Missing `create_dirs` / parent path |
| `mkdir -p` OK | Correct recovery; next turn likely oversized `write_file` |

---

## 8. Evidence → layer mapping

```text
  OBSERVATION                          LAYER
  ───────────                          ─────
  tools 1–4s                           (not harness — fast path OK)
  30–187s composing                      L1 + L2
  completion_tokens=2047               L2
  prompt 37k still fails                 L2 (not L1-only)
  prompt +87 per recovery                L3
  no compress at 57k                     L3 (threshold gap)
  4.2M input tokens, 1 write_file        task + policy gap
```

Raw log query (operator):

```bash
rg 'local_llm: max_tokens exhausted' ~/.edgecrab/profiles/homelab/logs/agent.jsonl
sqlite3 ~/.edgecrab/profiles/homelab/state.db \
  "SELECT id, message_count, tool_call_count, input_tokens FROM sessions ORDER BY started_at DESC LIMIT 5;"
```
