# 000 — Code Is Law: oMLX Evidence Ledger

**Status:** Verified against trees on 2026-07-24 (oMLX implemented; see pack README)  
**EdgeCrab:** this repo  
**edgequake-llm:** `/Users/raphaelmansuy/Github/03-working/edgequake-llm`  
**oMLX upstream:** https://github.com/jundot/omlx (runtime default often **:9050** via settings)  
**Sibling (not in this ledger):** **MTPLX** — full multi-lens plan in [012-mtplx-first-class.md](012-mtplx-first-class.md)

---

## 1. What oMLX is (external truth)

| Fact | Source |
|------|--------|
| Apple Silicon MLX inference server + menu bar app | omlx.ai, README |
| Default listen | `http://localhost:8000` (`/v1/...`) |
| OpenAI endpoints | `POST /v1/chat/completions`, `GET /v1/models`, `POST /v1/embeddings`, `POST /v1/rerank` |
| Anthropic endpoint | `POST /v1/messages` |
| Optional API key | `omlx serve --api-key …` |
| Multi-model + profiles | `/v1/models` may list `model:profile` |
| Tool calling | mlx-lm parsers (JSON, Qwen, Gemma, GLM, MiniMax, …) |
| VLM | Supported (v0.2.0+) with image inputs |
| Env knobs | `OMLX_MODEL_DIR`, `OMLX_PORT`, settings in `~/.omlx/settings.json` |

Hermes already treats oMLX as a **local Mac backend** (docs + long-prefill timeout behavior), not as a first-class named provider enum in the same way EdgeCrab names `ollama`/`lmstudio`.

---

## 2. EdgeCrab: oMLX presence

```text
rg -i 'omlx'  →  ZERO matches in application code / catalog (as of 2026-07-24)
```

**Conclusion:** oMLX is **not** a citizen. Users must abuse generic OpenAI-compatible config or point something else at port 8000 — neither is discoverable in `/model`, setup, or doctor.

---

## 3. EdgeCrab: local provider citizens (reference implementation)

### 3.1 Catalog (static seed)

| Path | Role |
|------|------|
| `crates/edgecrab-core/src/model_catalog_default.yaml` | `ollama:` (lines ~739+), `lmstudio:` (lines ~763+) |
| `crates/edgecrab-core/src/model_catalog.rs` | Resolve `provider/model`, aliases (`lm-studio` → `lmstudio`), lenient resolve for discovered models |

### 3.2 Live discovery

| Path | Role |
|------|------|
| `crates/edgecrab-core/src/model_discovery.rs` | `OllamaDiscovery`, `LMStudioDiscovery` adapters; local cache TTL |
| Same file | `fetch_openai_compatible_models` used by LM Studio |

**Live-discovery allowlist today:** OpenRouter, Ollama, LM Studio, Gemini, Copilot, Bedrock. **No oMLX.**

### 3.3 Local inference harness (policy)

| Path | Role |
|------|------|
| `crates/edgecrab-core/src/local_provider_policy.rs` | `is_local_inference_provider` = `lmstudio \| ollama \| vllm \| llamacpp` |
| Same | non-streaming tool turns, no transport retry, timeouts, prefill prune |
| `crates/edgecrab-tools/src/mutation_turn_policy.rs` | local write/create-dirs + tool arg ceilings for `lmstudio \| ollama` |
| `crates/edgecrab-tools/src/registry.rs` | `annotate_llm_definitions_for_local_turn` gated on `lmstudio \| ollama` |
| `crates/edgecrab-tools/src/tool_progress_tail.rs` | stall labels, timeout env names per provider |
| `crates/edgecrab-core/src/pricing.rs` | `ZERO_COST_PROVIDERS = ["copilot", "ollama", "lmstudio"]` |

### 3.4 Factory / routing

| Path | Role |
|------|------|
| `crates/edgecrab-tools/src/provider_factory.rs` | `create_provider_for_model` → `edgequake_llm::ProviderFactory` |
| `crates/edgecrab-core/src/model_router.rs` | smart routing / fallback |
| `crates/edgecrab-core/src/conversation.rs` | hot-swap via `create_provider_for_model` |

### 3.5 Operator surfaces

| Path | Role |
|------|------|
| `crates/edgecrab-cli/src/setup.rs` | provider list includes `ollama`, `lmstudio` only among locals |
| `crates/edgecrab-cli/src/doctor.rs` | port probes **11434** (Ollama), **1234** (LM Studio) — **not 8000** |
| `crates/edgecrab-cli/src/commands.rs` | `/provider` help text lists ollama/lmstudio discovery notes |
| `crates/edgecrab-cli/src/app.rs` | model selector refresh filters local providers |
| `crates/edgecrab-cli/src/main.rs` | auth hint `"local, no key"` for ollama/lmstudio |
| `docs/feature-docs/02-model-providers.md` | documents Ollama + LMStudio, not oMLX |
| `site/src/content/docs/providers/local.md` | local setup docs |

### 3.6 Secondary / SDK / site

| Area | Note |
|------|------|
| SDK examples / pypi-cli / npm-cli READMEs | list ollama/lmstudio; omit oMLX |
| Vision routing | `vision_models.rs` normalizes lmstudio; ollama vision heuristics |
| Proxy | OpenAI-compatible **outbound** bridge; not a substitute for inbound oMLX provider |

---

## 4. edgequake-llm: local provider pattern (law for implementation)

| Component | LM Studio (template) | Ollama | oMLX today |
|-----------|----------------------|--------|------------|
| Provider module | `src/providers/lmstudio.rs` | `src/providers/ollama.rs` | **missing** |
| Thin wrap of OpenAI-compat | **Yes** (inner `OpenAICompatibleProvider`) | Partial (native + OpenAI) | — |
| `ProviderType` enum | `LMStudio` | `Ollama` | — |
| `ProviderType::from_str` | `lmstudio \| lm-studio \| lm_studio` | `ollama` | — |
| `ProviderCatalog` descriptor | id `lmstudio`, discovery features | id `ollama` | — |
| Discovery module | `src/discovery/providers/lmstudio.rs` | `…/ollama.rs` | — |
| Factory create_* | `create_lmstudio[_with_model]` | `create_ollama[_with_model]` | — |
| Default host | `http://localhost:1234` | `http://localhost:11434` | **should be :8000** |
| Env prefix | `LMSTUDIO_*` | `OLLAMA_*` | **should be `OMLX_*`** |
| E2E tests | `tests/e2e_lmstudio_*.rs` | `tests/e2e_ollama_*.rs` | — |
| Docs | `docs/providers.md` table | same | — |

**Architecture law (from LM Studio module header):** standard chat/tools/stream/embed **delegate** to `OpenAICompatibleProvider`; only **provider-specific** behavior lives in the wrapper.

---

## 5. Gap statement (law)

```text
GAP-0  No canonical id `omlx` in edgequake-llm ProviderType / catalog / factory
GAP-1  No EdgeCrab catalog entry → invisible in /model static path
GAP-2  No discovery adapter → live inventory never lists loaded MLX models
GAP-3  Local harness allowlists omit omlx → wrong retries, streaming, timeouts, tool ceilings
GAP-4  Setup / doctor / help / pricing / zero-cost / docs omit omlx
GAP-5  Vision / tool progress / mutation policies hardcode lmstudio|ollama pairs
GAP-6  Tests do not assert omlx factory, discovery, or local-policy membership
```

Closing **GAP-0..2** without **GAP-3** is a product footgun (stacked GEN / dual requests). Closing **GAP-3** without **GAP-0** is dead code. **Order is non-negotiable** — see [010-implementation-plan.md](010-implementation-plan.md).

---

## 6. Anti-patterns already present (do not extend)

| Anti-pattern | Where | Fix in this work |
|--------------|-------|------------------|
| Hardcoded `matches!(…, "lmstudio" \| "ollama")` in **many** crates | policy, tools, CLI | **One** local-family source of truth |
| Duplicate alias tables | catalog, discovery, vision_models | Normalize once; re-export |
| “Generic openai-compatible only” for local Mac users | setup UX | Named first-class `omlx` with defaults |

---

## 7. Related specs (do not duplicate)

| Spec | Relationship |
|------|----------------|
| `docs/feature-docs/02-model-providers.md` | Parent architecture doc — update after ship |
| `specs/dynamic_model/*` | Discovery + TUI truthfulness ADRs |
| `specs/014-improve-local-harness/*` | Local tool harness physics |
| `specs/022-ai-agent-gap/*` | Strategy; local Mac is a wedge vs Hermes docs |
| edgequake-llm `specs/002-upgrade-support/*` | Provider audit pattern |

---

## 8. Acceptance evidence checklist (post-implementation)

- [ ] `rg -i 'omlx' crates edgequake-llm` shows intentional provider paths  
- [ ] `ModelCatalog::resolve_spec_lenient("omlx/…")` works for discovered ids  
- [ ] `create_provider_for_model("omlx", "…")` returns provider named `"omlx"`  
- [ ] `is_local_inference_provider("omlx") == true`  
- [ ] `doctor` probes port **8000** (or `OMLX_HOST`)  
- [ ] `setup` lists oMLX; default model string `omlx/…`  
- [ ] Unit tests green offline; e2e gated on live oMLX server  
