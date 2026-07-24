# 013 — Local Prefix / KV Cache Optimization (July 2026)

**Status:** Implemented (2026-07-24)  
**Pack:** [023 local Apple Silicon providers](README.md)  
**Trigger forensics:** MTPLX session — TTFT **1m16s**, **8192 cached + 9017 new**, miss  
`prefix_divergence_at_token`, **13 tools**, MTP depth 3, verify thrash 365 calls.

---

## 1. First principles

### Two different “caches”

| Kind | Who | Mechanism | EdgeCrab today |
|------|-----|-----------|----------------|
| **Cloud prompt cache** | Anthropic / OpenRouter / … | `cache_control` breakpoints | `prompt_cache_policy.rs` |
| **Local KV / MTP prefix bank** | oMLX, MTPLX, Ollama, LM Studio | Byte-identical request prefix | **This doc** |

`decide_prompt_cache` correctly returns **false** for `omlx`/`mtplx`. That does **not** mean “don’t optimize cache” — it means **don’t use Anthropic markers**. Local servers need **prefix stability**.

### Law of the turn (from forensics)

```text
wall ≈ prefill(NEW) + decode + tools + verify
partial hit (8192) still left 9017 NEW → long TTFT
prefix_divergence_at_token → something changed mid-prefix (tools/system/history)
```

**P0 agent SLO for local Mac:** minimize **NEW prefill tokens** and eliminate **prefix_divergence** on multi-tool turns where only the tip should change.

---

## 2. July 2026 principles (L-PC)

| ID | Law |
|----|-----|
| L-PC1 | Prefix stability is a first-class harness SLO |
| L-PC2 | Stable system zone never rebuilds mid-session (except explicit compress) |
| L-PC3 | Dynamic zone is tip-append (goals/steers as user msgs — already) |
| L-PC4 | **Tool wire schemas are a cache key** — freeze after first send |
| L-PC5 | History rewrites invalidate KV — compress carefully |
| L-PC6 | Local KV ≠ cloud prompt cache — separate policy module |
| L-PC7 | Prefer smaller frozen CORE toolsets when local |
| L-PC8 | `reasoning=none` on local tool turns (already) |
| L-PC9 | Byte-identical wire via `normalize_api_messages_for_kv` |
| L-PC10 | E2E proves freeze, not anecdotes |

---

## 3. Architecture (SOLID)

```text
SessionState
  frozen_local_api_tools + fingerprint   ← single freeze point (S)
        ▲
local_prefix_cache::resolve_frozen_local_api_tools
        │  uses annotate_llm_definitions_for_local_turn once
        ▼
api_call_with_retry(..., api_tool_defs)
```

| Principle | Application |
|-----------|-------------|
| **S** | `local_prefix_cache.rs` owns freeze policy only |
| **O** | New local providers inherit via `is_local_inference_provider` |
| **L** | Frozen tools remain valid `ToolDefinition` wire |
| **I** | No cloud cache API on local path |
| **D** | Conversation depends on policy helper, not provider-specific ifs |

---

## 4. Implementation (code)

| Item | Location |
|------|----------|
| Policy + freeze | `edgecrab-core/src/local_prefix_cache.rs` |
| Session fields | `SessionState::frozen_local_api_tools*` |
| Wire point | `conversation.rs` before `api_call_with_retry` |
| Clear freeze | `tool_defs_dirty`, model transfer, session reset |
| KV normalize | existing `local_provider_policy` (unchanged) |
| Spec / tests | this file + `tests/local_prefix_cache_e2e.rs` |

### Freeze rule

1. Fingerprint = ordered tool names from `active_tool_defs`.  
2. If local provider + tools non-empty:  
   - if fingerprint matches freeze → return frozen defs  
   - else annotate once with current budget → store freeze → return  
3. Non-local → always annotate path (no freeze).  
4. Tool set change (`tool_defs_dirty`) → clear freeze.

**Why this fixes prefix_divergence:** local annotation appends  
`Local turn limit: max argument ~N bytes…` to mutation tools.  
`N` / max_tokens can change every iteration as context grows → **tool schema bytes change every API call** → mid-prefix divergence. Freezing locks those strings for the session while the tool set is stable.

---

## 5. E2E acceptance

| ID | Test |
|----|------|
| U-PC-01 | Fingerprint changes when tools add/remove |
| U-PC-02 | Second resolve reuses freeze (descriptions identical) despite different budget |
| U-PC-03 | Non-local never freezes |
| U-PC-04 | Clear freeze on tool set change |
| E-PC-01 | Multi-round: system+tools hash stable; only tip messages grow |

---

## 6. Non-goals (this wave)

- Parsing MTPLX `prefix_divergence_at_token` from server (no public API yet)  
- Changing default toolsets to minimal CORE (product decision)  
- Anthropic cache for local providers  

---

## 7. Follow-ups (P1)

- Local-default deferred tools / tool_search for Mac  
- Compaction: immutable summary prefix contract tests  
- Shelf line: `local prefix: tools frozen · N tools`  
- Verify-tool budget specifically for local providers  
