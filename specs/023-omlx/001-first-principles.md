# 001 — First Principles: Local Apple Silicon Providers

---

## 0. Problem restated from first causes

**Users run agents on Macs.** Several local servers expose OpenAI-compatible APIs optimized for Apple Silicon:

| Server | Agent-shaped strength |
|--------|------------------------|
| **oMLX** | SSD KV cache across agent context thrash, multi-model admin |
| **MTPLX** | Native **MTP** speculative decode (speed), first-class Hermes connect |

EdgeCrab already has a **local inference family** (Ollama, LM Studio, …) with a full harness. Each Mac server must become a **named citizen** (`omlx/…`, `mtplx/…`), not a generic base-URL hack.

The work is not “add another HTTP client.” The work is:

> Make `omlx/<model>` and `mtplx/<model>` first-class **names** that flow through every subsystem that keys behavior on provider identity — via **one local OpenAI-compatible family**, not N forks.

**MTPLX full plan:** [012-mtplx-first-class.md](012-mtplx-first-class.md).

---

## 1. Ontology (what exists)

```text
┌──────────────────────────────────────────────────────────────────┐
│  Physical: Apple Silicon process serving MLX weights (oMLX)       │
└───────────────────────────────┬──────────────────────────────────┘
                                │ HTTP (OpenAI-shaped + optional Anthropic)
┌───────────────────────────────▼──────────────────────────────────┐
│  edgequake-llm: LLMProvider citizen (name, chat, tools, stream)  │
└───────────────────────────────┬──────────────────────────────────┘
                                │ Arc<dyn LLMProvider>
┌───────────────────────────────▼──────────────────────────────────┐
│  EdgeCrab: catalog · discovery · policy · pricing · TUI · setup  │
└──────────────────────────────────────────────────────────────────┘
```

| Layer | Responsibility (SRP) | Must NOT do |
|-------|----------------------|-------------|
| **oMLX server** | Inference, cache, model load | — (external) |
| **edgequake-llm** | Transport + wire adapt + health/list | Agent loop, TUI, catalog UX |
| **EdgeCrab core** | Catalog, discovery adapters, local harness policy | Duplicate HTTP clients |
| **EdgeCrab CLI/gateway** | Surface choices, doctor, config | Reimplement provider |

---

## 2. Design laws (must hold after the change)

### L1 — One canonical identity
`omlx` is the only runtime id. Aliases normalize at the **boundary** (parse/resolve), never branch deep code on aliases.

### L2 — Open/Closed for local family
Adding oMLX must be **registration + thin adapter**, not N new `if provider ==` forks. Prefer:

```text
const LOCAL_INFERENCE_PROVIDERS: &[&str] = &[
    "ollama", "lmstudio", "omlx", "mtplx", "vllm", "llamacpp",
];
fn is_local_inference_provider(name: &str) -> bool {
    LOCAL_INFERENCE_PROVIDERS.contains(&name)
}
```

…or a small shared helper re-exported from one crate (see [004](004-rust-expert-lens.md)).

### L3 — Composition over duplication (DRY)
`OmlxProvider` = configuration + optional health/metadata + **inner** `OpenAICompatibleProvider`  
(LM Studio is the proven pattern — copy the **shape**, not the LM Studio-specific CLI auto-load.)

### L4 — Fail closed on reachability, fail open on catalog
- Unreachable oMLX → empty live list + doctor warning (not crash).  
- Static catalog always offers `omlx` so the selector never pretends the provider does not exist.  
- `resolve_spec_lenient` accepts discovered ids not in static YAML (same as lmstudio).

### L5 — Local harness defaults are safety, not cosmetics
For local servers, EdgeCrab **must not** dual-request after timeout. oMLX inherits:

- block transport retry / streaming fallback on timeout  
- prefer non-streaming tool turns  
- long HTTP timeout (600s default)  
- tool_choice / max_tokens / reasoning=none local tool policy  

Missing this makes oMLX “supported” but **harmful under load**.

### L6 — Protocol minimalism (P0)
P0 ships **OpenAI-compatible only**. Dual Anthropic `/v1/messages` is P1: only if product needs Claude-shaped clients **through** EdgeCrab-to-oMLX (EdgeCrab already speaks many wires to models; forcing Anthropic wire to oMLX is optional).

### L7 — Platform honesty
- Document: server requires Apple Silicon / macOS 15+.  
- Client code must compile and run on Linux/CI; health fails gracefully.  
- Never `#[cfg(target_os = "macos")]` the provider type itself.

### L8 — Zero cost is a product signal
Local = free tokens in `/cost` and pricing. oMLX joins `ZERO_COST_PROVIDERS`.

### L9 — Discovery is provider-scoped, not generic cloud `/v1/models`
Local OpenAI-compat `/v1/models` is trustworthy inventory (loaded + available). Same rationale as LM Studio/Ollama. Do not invent cloud-style entitlement filtering for oMLX.

### L10 — Tests define citizenship
A provider is first-class when **offline unit tests** prove identity/policy and **optional e2e** prove chat+tools against a live server — not when a README mentions it.

---

## 3. SOLID mapping

| Principle | Application |
|-----------|-------------|
| **S** | One module `omlx.rs` for oMLX-specifics; policy stays in `local_provider_policy`; discovery stays in discovery adapters |
| **O** | Extend local family set + catalog YAML + factory match arms; avoid editing conversation loop per provider |
| **L** | `OmlxProvider: LLMProvider` must substitute for any other provider in the ReAct loop |
| **I** | Do not force EmbeddingProvider if embeddings optional; implement when `/v1/embeddings` is available |
| **D** | EdgeCrab depends on `LLMProvider` + factory, not concrete oMLX types |

---

## 4. What “first-class” means (definition of done)

A provider is first-class iff **all** are true:

1. **Named** — canonical id in factory, catalog, and user-facing help  
2. **Selectable** — appears in setup + `/model` static rows  
3. **Discoverable** — live model list when server up  
4. **Runnable** — chat + tools + stream through ReAct  
5. **Policed** — local harness membership  
6. **Diagnosable** — doctor port/health  
7. **Priced** — zero-cost accounting  
8. **Documented** — feature doc + site local providers page  
9. **Tested** — unit + e2e plan executed  

Anything less is a **half-citizen** (the current state for “point OpenAI-compatible at :8000”).

---

## 5. Explicit non-goals

| Non-goal | Why |
|----------|-----|
| Bundle oMLX binary in EdgeCrab | Separate product lifecycle |
| Auto-download MLX weights | oMLX admin already does this |
| Prefer oMLX over Ollama as default model globally | Default stays product-owned (`ollama/gemma4:latest` today); oMLX is opt-in first-class |
| Full Anthropic wire parity in P0 | OpenAI path is enough for EdgeCrab agent loop |
| Rerank provider in EdgeCrab P0 | Out of scope unless a tool already needs it |

---

## 6. Decision record (locked)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| D1 Provider style | Thin OpenAI-compatible wrapper | DRY with LM Studio; oMLX advertises OpenAI drop-in |
| D2 Canonical id | `omlx` | Matches product name + Hermes test fixtures |
| D3 Default URL | `http://127.0.0.1:8000` | oMLX README default |
| D4 Local family | Yes | Same physics as ollama/lmstudio |
| D5 Discovery | Dynamic `/v1/models` | Local inventory trustworthy |
| D6 API key | Optional env `OMLX_API_KEY` | Server may require |
| D7 Catalog seed | Minimal `default` + live override | Models are user-local |
| D8 Local family refactor | Single shared set | Stop scattershot matches |
| D9 EdgeQuake first | Land provider in edgequake-llm, then EdgeCrab | Dependency order |
| D10 Anthropic path | P1 | Not required for EdgeCrab ReAct |

---

## 7. Success metrics

| Metric | Target |
|--------|--------|
| Time-to-first successful local Mac agent turn via oMLX | ≤ setup steps for LM Studio (host + model) |
| Code paths with hard-coded `lmstudio \| ollama` only | **Decreasing** after refactor (ideally zero for family checks) |
| Offline tests covering omlx identity | ≥ factory + catalog + local policy + discovery parse |
| Live e2e | Gated `#[ignore]` or env `OMLX_E2E=1` chat + tools |
