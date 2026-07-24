# 014 — Apple Silicon Local Inference Landscape (July 2026)

**Status:** Research complete; **Wave D–F implemented** (2026-07-24)  
**Date:** 2026-07-24  
**Question:** Which local Mac providers *beyond* what EdgeCrab already first-classes are worth supporting, and how?  
**Method:** First principles (agent harness needs), competitive scan, DRY/SOLID cost vs wedge.

---

## 0. Already first-class or in the local family

| Id | Engine / notes | Citizenship today |
|----|----------------|-------------------|
| **ollama** | llama.cpp Metal; MLX path on recent Ollama | Full (catalog + discovery + factory) |
| **lmstudio** | GGUF + **MLX** engine | Full |
| **omlx** | MLX, SSD KV, multi-model | Full (023 Wave A) |
| **mtplx** | MLX + **MTP** speculative decode | Full (023 Wave B) |
| **vllm** | Name in local family | Policy only (generic Linux/CUDA vLLM) |
| **llamacpp** | llama-server Metal GGUF | **Full** (Wave D) |
| **vllm-mlx** | MLX continuous batching | **Full** (Wave E) |
| **mlx-lm** | `mlx_lm.server` | **Full** (Wave F) |
| **openai-compatible** | Generic any base URL | Exists in edgequake-llm; **not** a Mac product citizen |

**Hermes Mac guide** already pairs **llama.cpp** + **omlx**. EdgeCrab has omlx/mtplx named; **llama-server** is only a policy alias.

---

## 1. First principles: what makes a Mac provider “worth” first-class

A local server is worth a **named** EdgeCrab citizen only if **most** hold:

| # | Criterion | Why for agents |
|---|-----------|----------------|
| C1 | **OpenAI-compatible** chat + tools (`/v1/chat/completions`) | ReAct loop is OpenAI-shaped |
| C2 | **`GET /v1/models`** (or trustworthy inventory) | `/model` live discovery |
| C3 | **Agent-shaped** (prefix/KV, long prefill, tools) | Coding agents thrash prefixes |
| C4 | **Material Mac share** or unique capability | Avoid infinite one-off wrappers |
| C5 | **Distinct product identity** (not “another :8000”) | UX: setup, doctor, badges |
| C6 | **Settings/env discoverable** | Zero-config Mac dogfood |
| C7 | **Implementation cost** ≤ one thin `LocalOpenAi*` registration | DRY with 012/013 |

**Reject first-class** if:

- Only a GUI with no stable API  
- Pure research CLI without multi-turn tools  
- Duplicate of an already-named product (e.g. another thin wrap of the same oMLX fork)  
- Covered adequately by **`openai-compatible` + `/endpoint`** with no unique agent value  

---

## 2. Landscape scan (Apple Silicon–relevant, 2026)

### 2.1 Engines (runtime cores — usually *not* EdgeCrab “providers”)

| Engine | Role | EdgeCrab stance |
|--------|------|-----------------|
| **MLX / mlx-lm** | Apple’s array + LLM stack | Engine under omlx/mtplx/lmstudio/vllm-mlx |
| **llama.cpp (Metal)** | GGUF universal | Engine under ollama/lmstudio/`llama-server` |
| **MLC-LLM** | Compiler stack, on-device | Research; rare agent API usage |
| **PyTorch MPS** | Torch path | Not a product server for agents |

### 2.2 Product servers / apps

| Product | Engine | API | Default port (typical) | Agent value | First-class? |
|---------|--------|-----|------------------------|-------------|--------------|
| **Ollama** | llama.cpp / MLX | OpenAI + native | 11434 | Convenience, ubiquity | **Have** |
| **LM Studio** | GGUF + MLX | OpenAI + native | 1234 | GUI + MLX speed | **Have** |
| **oMLX** | MLX | OpenAI + Anthropic | **9050** | SSD KV, multi-model, agent TTFT | **Have** |
| **MTPLX** | MLX + MTP | OpenAI | settings (8000/8002…) | Speculative decode | **Have** |
| **llama-server** (`llama.cpp`) | Metal GGUF | OpenAI | **8080** | Hermes Mac path; control | **P0 promote** |
| **mlx_lm.server** | MLX | OpenAI | **8080** | Official Apple path | **P1 thin** or generic |
| **vLLM-MLX** | MLX vLLM-style | OpenAI + Anthropic | **8000**/8010 | Batching, paged KV, prefix cache | **P1 named** |
| **mlx-openai-server** | MLX FastAPI | OpenAI | varies | Multimodal server | **P2 / generic** |
| **Maic** | MLX | OpenAI-claim | TBD | Early community | **Watch / generic** |
| **Jan** | llama.cpp | OpenAI | 1337-ish | Desktop chat | **P3 / generic** |
| **Msty** | multi | varies | varies | Desktop | **P3 / generic** |
| **KoboldCPP** | llama.cpp | OpenAI-ish | 5001 | Roleplay | **Out** for coding agents |
| **exo** | cluster | OpenAI-ish | varies | Multi-Mac | **P2 research** |
| **Docker Model Runner** | container | OpenAI | varies | Linux/Mac Docker | **P2 generic** |
| **Apple Foundation Models / on-device** | system | not OpenAI ReAct | — | OS features | **Out** (no agent tools API) |

**Note:** oMLX’s lineage includes **vllm-mlx**-class ideas (batching, paged SSD cache). Naming `vllm-mlx` separately still helps users who install the open-source server without the oMLX app.

---

## 3. Deep dive: candidates worth action

### 3.1 P0 — `llamacpp` / `llama-server` (promote to full citizen)

**Why worth it**

- Hermes Mac guide’s primary non-MLX path  
- Already in `LOCAL_INFERENCE_PROVIDERS` as `"llamacpp"` but **invisible** in catalog/setup/factory  
- Power users prefer GGUF + quantized KV flags for RAM-constrained Macs  
- Default OpenAI API on **:8080** is stable and documented  

**Gaps today**

| Gap | Detail |
|-----|--------|
| No catalog seed | `/model` never shows `llamacpp/…` |
| No factory type | Must abuse `openai-compatible` or env hacks |
| No discovery adapter | No live `/v1/models` |
| No setup/doctor | Port 8080 not probed as “llama-server” |
| Name ambiguity | `llamacpp` vs `llama-server` vs `llama.cpp` |

**Canonical identity (locked for implementation)**

| Concept | Value |
|---------|--------|
| Id | `llamacpp` |
| Aliases | `llama-server`, `llama.cpp`, `llamacpp-server` |
| Default host | `http://127.0.0.1:8080` |
| Env | `LLAMACPP_HOST` / `LLAMA_SERVER_HOST` / `LLAMACPP_BASE_URL` |
| Timeout | `LLAMACPP_TIMEOUT_SECONDS` (600) |
| Settings | None standard — env + `/endpoint` only |
| Models | Live `/v1/models`; static seed `default` |

**How to implement (DRY — copy MTPLX/oMLX pattern)**

```text
1. edgequake-llm
   - LlamaCppProvider thin wrap (or OpenAICompatible with name "llamacpp")
   - Optional: reuse local_openai_common only
   - ProviderType::LlamaCpp | factory from_str
   - Discovery: GET /v1/models (dynamic, local TTL)
2. EdgeCrab
   - catalog YAML seed
   - Omlx-style discovery adapter (already have OpenAICompatibleDiscovery pattern)
   - pricing zero-cost
   - setup + doctor port 8080
   - /endpoint row (already generic list — add spec)
   - prefers_nonstreaming + stall copy
3. Tests
   - citizenship offline
   - e2e LLAMACPP_E2E=1 against llama-server
```

**Estimate:** 0.5–1 day (less than MTPLX: no app settings file).  
**Priority:** **P0** — closes Hermes parity hole with almost no new architecture.

---

### 3.2 P1 — `vllm-mlx` (named thin citizen)

**Why worth it**

- Explicit **prefix caching + paged KV + continuous batching** (agent-relevant)  
- Dual OpenAI + Anthropic APIs  
- Developer-install path for people who won’t use oMLX.app  
- Overlaps oMLX features but different binary/install UX  

**Risks**

- Port collision with many servers on **8000**  
- Project maturity / fork churn  
- Overlap messaging with oMLX (“why both?”)

**Canonical identity (proposal)**

| Concept | Value |
|---------|--------|
| Id | `vllm-mlx` |
| Aliases | `vllm_mlx`, `vllmm lx` → normalize `vllm-mlx` |
| Default host | `http://127.0.0.1:8000` (document collision with mtplx) |
| Env | `VLLM_MLX_HOST`, `VLLM_MLX_API_KEY` optional |
| Discovery | `/v1/models` |

**Implementation:** same thin local OpenAI registration as llamacpp; **do not** fork conversation logic.  
**Estimate:** 0.5 day after llamacpp template.  
**Priority:** **P1** after llamacpp.

**Product copy:** “vLLM-MLX (local MLX server · continuous batching)” — distinct from oMLX app.

---

### 3.3 P1 — `mlx-lm` / `mlx_lm.server` (thin or “preset” only)

**Why**

- Official Apple MLX-LM OpenAI server  
- One-liner for researchers: `mlx_lm.server --model … --port 8080`  

**Why not full P0**

- Single loaded model; weaker multi-model admin  
- Port/host always user-chosen  
- Largely covered by **named thin provider** *or* by **`openai-compatible` + `/endpoint` preset**

**Recommendation**

| Option | When |
|--------|------|
| **A. Named `mlx-lm` citizen** | If we want setup row + doctor probe + docs parity with Hermes “mlx_lm.server” |
| **B. Preset only** | `/endpoint` preset “mlx_lm.server (:8080)” bound to `openai-compatible` or `llamacpp`-style thin provider |

**Prefer A** if cost stays &lt; 0.5 day using shared template; else **B**.

Default: `http://127.0.0.1:8080` — **collides with llama-server**; discovery + doctor must label by product, not port alone.

---

### 3.4 P2 — Generic improvements (high leverage, multi-product)

These help *all* remaining servers without new brands:

| Work | Value |
|------|--------|
| **Doctor: multi-port local scan** | 11434, 1234, 9050, 8000–8010, 8080 with HTTP `/v1/models` fingerprint |
| **`/endpoint` presets** | One-click: llama-server, mlx_lm.server, vllm-mlx, Docker |
| **Fingerprint banner** | “Likely oMLX / MTPLX / llama-server” from response headers/body if available |
| **Promote `vllm` alias** | Map to openai-compatible local if used for non-Apple vLLM too |

**Priority:** **P2** alongside P1 named providers.

---

### 3.5 Watch / reject for first-class (for now)

| Product | Decision | Rationale |
|---------|----------|-----------|
| **Maic** | Watch | Early; reassess when stable install + tools |
| **Jan / Msty** | Generic only | Desktop UX; API is generic OpenAI-compat |
| **KoboldCPP** | Reject agent first-class | Wrong product niche |
| **exo** | Research note | Cluster; needs multi-node story |
| **Apple FM API** | Reject | Not ReAct tool wire |
| **Another oMLX fork** | Reject | Duplicate citizen |

---

## 4. Decision matrix (score 0–5)

| Candidate | C1 API | C3 Agent | C4 Share | C5 Identity | C7 Cost | **Total** | Decision |
|-----------|--------|----------|----------|-------------|---------|-----------|----------|
| **llama-server** | 5 | 4 | 5 | 5 | 5 | **24** | **Implement P0** |
| **vllm-mlx** | 5 | 5 | 3 | 4 | 4 | **21** | **Implement P1** |
| **mlx_lm.server** | 5 | 3 | 4 | 3 | 4 | **19** | **P1 thin or preset** |
| **mlx-openai-server** | 5 | 3 | 2 | 2 | 3 | **15** | Generic / P2 |
| **Jan** | 4 | 2 | 3 | 3 | 3 | **15** | Generic |
| **Maic** | 4 | 3 | 1 | 2 | 2 | **12** | Watch |
| **exo** | 3 | 3 | 2 | 4 | 2 | **14** | Later |

---

## 5. Implementation architecture (shared)

Do **not** invent a third HTTP stack. Extend the family:

```text
LocalOpenAiServerSpec {
  id, aliases, default_host, env_host[], env_key[],
  settings_loader: Option<fn() -> RuntimeConfig>,
  fs_model_fallback: Option<fn() -> Vec<String>>,
}
```

Already partially real:

- `local_openai_common.rs` (normalize, list models)  
- `local_prefix_cache.rs` (freeze tools)  
- `LOCAL_INFERENCE_PROVIDERS`  
- `provider_endpoints` TUI  

**New work for each P0/P1 id:** factory + catalog + discovery adapter + setup/doctor strings + e2e citizenship — **template from mtplx**.

### 5.1 Port collision policy

| Port | Common occupants |
|------|------------------|
| 11434 | Ollama |
| 1234 | LM Studio |
| 9050 | oMLX |
| 8000–8002 | MTPLX, vllm-mlx, many custom |
| 8080 | llama-server, mlx_lm.server |

**Doctor law:** TCP open is weak; prefer `GET /v1/models` + label by **configured provider**, not “something on 8080”.

---

## 6. Wave plan (pack 023 continuation)

| Wave | Scope | Est. |
|------|--------|------|
| **A–C** | oMLX, MTPLX, prefix freeze | **Done** |
| **D** | **llamacpp / llama-server full citizen** | 0.5–1 d |
| **E** | **vllm-mlx** named citizen + docs | 0.5 d |
| **F** | mlx_lm.server thin *or* `/endpoint` presets + multi-port doctor | 0.5 d |
| **G** | Watchlist re-score (Maic, exo) | quarterly |

### Wave D–F acceptance

- [x] `llamacpp/<id>` in catalog + `/model`  
- [x] Live discovery on :8080  
- [x] Local harness + prefix freeze  
- [x] setup + doctor  
- [x] `/endpoint`  
- [x] Offline citizenship tests + `LLAMACPP_E2E=1`  
- [x] `vllm-mlx` + `mlx-lm` same bar (`VLLM_MLX_E2E=1` / `MLX_LM_E2E=1`)  
- [x] DRY shell: `LocalOpenAiIdentity` / `LocalOpenAiProvider` in edgequake-llm

---

## 7. Product messaging

| User goal | Recommend |
|-----------|-----------|
| Easiest install | Ollama |
| GUI + MLX speed | LM Studio |
| Best agent TTFT / SSD KV | **oMLX** |
| Fastest speculative decode | **MTPLX** |
| Hermes-style GGUF control | **llama-server** (`llamacpp`) |
| Dev server batching without oMLX app | **vllm-mlx** |
| Research one-liner | **mlx_lm.server** |

EdgeCrab should **not** pick one default forever — first-class names + `/endpoint` + doctor honesty.

---

## 8. Explicit non-goals

- Bundle any Mac app binary  
- Implement MLX kernels or MTP in Rust  
- First-class every desktop chat UI  
- Treat `openai-compatible` alone as “Apple Silicon support” in marketing  

---

## 9. Cross-refs

| Doc | Role |
|-----|------|
| [README](README.md) | Pack index |
| [012](012-mtplx-first-class.md) | Template for named Mac server |
| [013](013-local-prefix-cache-july-2026.md) | Why prefix stability matters for all of the above |
| [010](010-implementation-plan.md) | Wave D–F checklist (update when implementing) |
| Hermes | `website/docs/guides/local-llm-on-mac.md` (llama.cpp + omlx) |

---

## 10. Recommendation (executive)

1. **Implement next:** full **`llamacpp` / llama-server** citizen (closes policy-only gap + Hermes parity).  
2. **Then:** **`vllm-mlx`** if Mac power-user demand continues (prefix-cache story overlaps 013).  
3. **mlx_lm.server:** thin citizen *or* endpoint preset — do not over-invest.  
4. **Everything else:** `openai-compatible` + `/endpoint` + improved doctor scanning.  
5. **Keep investing** in **shared local family + prefix freeze** — that multiplies every new Mac server.

---

## 11. Evidence sources (web, 2026)

- Ollama MLX backend / Mac local guides (2026)  
- LM Studio MLX vs Ollama throughput comparisons  
- vllm-mlx (waybarrios) — continuous batching, prefix cache, dual APIs  
- mlx-lm.server official OpenAI-compatible path  
- Hermes local-LLM-on-Mac (llama.cpp + omlx)  
- arXiv comparative study: MLX, MLC-LLM, llama.cpp, Ollama on Apple Silicon (2025)  
