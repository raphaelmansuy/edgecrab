# 005 — Touchpoint Matrix: Every Place Models Play a Role

**Purpose:** Exhaustive cross-ref of surfaces that must learn **`omlx`** (and later **`mtplx`**) for first-class citizenship.  
**Legend:** **R** = required P0 · **S** = should P1 · **C** = could · **—** = N/A  
**Status:** oMLX column reflects post-implementation (2026-07-24). MTPLX = [012](012-mtplx-first-class.md) (not shipped).

When implementing MTPLX, apply the **same row set** with id `mtplx` — do not invent new surface categories.

| Surface family | oMLX | MTPLX |
|----------------|------|-------|
| Factory / catalog / discovery | **done** | planned 012 |
| Local harness / pricing | **done** | planned 012 |
| Setup / doctor / `/model` / `/endpoint` | **done** | planned 012 |
| Docs / e2e | **done** | planned 012 |

---

## 1. Identity & factory

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| ProviderType enum | `edgequake-llm/src/factory.rs` | Add `Omlx` | R | missing |
| from_str / aliases | same | `omlx`, `o-mlx`, `o_mlx` | R | missing |
| create_llm_provider | same | wire create_omlx | R | missing |
| ProviderCatalog | `edgequake-llm/src/provider_catalog.rs` | descriptor + features | R | missing |
| Module + re-export | `providers/omlx.rs`, `lib.rs` | new | R | missing |
| create_provider_for_model | `edgecrab-tools/src/provider_factory.rs` | inherit via factory | R | works if factory has omlx |
| Vision normalize | `edgecrab-tools/src/vision_models.rs` | alias normalize | S | missing |
| Model transfer resolve | `edgecrab-core/src/model_transfer.rs` | alias tests | S | missing |

---

## 2. Catalog & discovery

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| Static catalog | `model_catalog_default.yaml` | `omlx:` seed | R | missing |
| Alias normalize | `model_catalog.rs` | o-mlx → omlx | R | missing |
| resolve_spec_lenient | same | multi-segment + profiles | R | pattern exists for lmstudio |
| Live discovery adapter | `model_discovery.rs` | `OmlxDiscovery` | R | missing |
| Discovery allowlist / registry | same | register adapter | R | missing |
| Discovery cache | `~/.edgecrab/model_discovery_cache.json` | key `omlx` | R | automatic if adapter works |
| eq-llm discovery | `edgequake-llm/src/discovery/providers/omlx.rs` | Dynamic | R | missing |
| User override models.yaml | docs | document omlx block | S | missing |

---

## 3. Local harness & loop physics

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| is_local_inference_provider | `local_provider_policy.rs` | include omlx + DRY set | R | missing |
| prefers_nonstreaming_tool_turns | same | include omlx | R | missing |
| blocks_transport_retry | same | via is_local | R | inherits if membership fixed |
| local_http_timeout_secs | same | `OMLX_TIMEOUT_SECONDS` | R | missing |
| transport_stall_error_suffix | same | omlx copy | R | missing |
| log_local_harness_activated | same | message mentions omlx family | S | wording |
| mutation_turn_policy local match | `mutation_turn_policy.rs` | use shared is_local | R | hardcoded lmstudio\|ollama |
| annotate_llm_definitions_for_local_turn | `registry.rs` | use shared is_local | R | hardcoded |
| tool_progress_tail labels | `tool_progress_tail.rs` | omlx strings + timeout env | R | missing |
| conversation local branches | `conversation.rs` | via policy only | R | OK if policy fixed |
| provider_call local | `provider_call.rs` | via policy | R | OK if policy fixed |
| agent hot-swap local full | `agent.rs` | via policy | R | OK if policy fixed |

---

## 4. Pricing, cost, routing

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| ZERO_COST_PROVIDERS | `pricing.rs` | add `omlx` | R | missing |
| model_router tiers | `model_router.rs` | treat like other locals | S | verify |
| Fallback config | `config.rs` | allow omlx in examples | S | optional |
| SDK free providers list | `specs/sdk-v2/11-EVENT-SYSTEM.md` (doc only) | update if published | C | doc |

---

## 5. CLI / TUI / setup / doctor

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| setup provider list | `edgecrab-cli/src/setup.rs` | add omlx row + default_model | R | missing |
| setup default_model() | same | `omlx/default` or first | R | missing |
| doctor port probe | `doctor.rs` | 8000 / OMLX_HOST | R | missing |
| doctor messaging | same | up/down strings | R | missing |
| /provider help | `commands.rs` | document omlx + discovery | R | missing |
| /models discovery_note | same | discovery_note("omlx") | R | missing |
| main auth hint | `main.rs` | local optional key | R | missing |
| app model refresh filter | `app.rs` | include omlx in local refresh | R | filter ollama\|lmstudio only |
| setup overlays detail | `setup_overlays.rs` | provider detail if needed | S | verify |
| proxy upstreams | `edgecrab-proxy` | **not** required for inbound oMLX | — | N/A |
| slash /model selector | app model discovery | live rows | R | via discovery |

---

## 6. Gateway / ACP / multi-surface

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| Gateway agent model | config / session | uses same factory | R | automatic |
| ACP agent | edgecrab-acp | same AgentBuilder path | R | automatic |
| API server model field | gateway api_server | pass-through model string | R | automatic |
| Cron jobs model | edgecrab-cron | if model configurable | S | verify |
| Subagent delegate model | sub_agent_runner / delegate_task | parent or override | S | verify string parse |
| Shadow judge model | shadow_judge.rs | factory | S | automatic |
| Mixture of agents | mixture_of_agents.rs | factory | S | automatic |
| Vision tool provider | vision.rs | create_provider | S | after normalize |

---

## 7. Config, env, migrate

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| AppConfig model string | `config.rs` DEFAULT | no change to global default | — | keep product default |
| Env EDGECRAB / OMLX_* | docs + doctor | document | R | missing |
| migrate from hermes | edgecrab-migrate | map custom base_url :8000 → omlx? | C | optional heuristic |
| .env example | README | OMLX_HOST | S | missing |

---

## 8. Docs & site

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| Feature doc providers | `docs/feature-docs/02-model-providers.md` | add oMLX + discovery | R | missing |
| Project summary | `docs/001_overview/*` | table if lists providers | S | check |
| Site local.md | `site/src/content/docs/providers/local.md` | oMLX section | R | missing |
| README providers table | `README.md` | row | R | missing |
| CHANGELOG | `CHANGELOG.md` | entry on ship | R | pending |
| AGENTS.md | optional note | local family | C | optional |
| edgequake-llm providers.md | docs table | row | R | missing |
| Hermes parity note | site guide | optional “vs Hermes Mac guide” | C | — |

---

## 9. SDKs & examples

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| sdk-examples rust | if list providers | add omlx | S | check |
| pypi-cli README | provider list | add omlx | S | missing |
| npm-cli README | provider list | add omlx | S | missing |
| node native examples | local ollama bias | optional omlx example | C | — |

---

## 10. Tests (see also 009)

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| factory unit | edgequake-llm | from_str, create | R | missing |
| discovery unit | both | parse / empty | R | missing |
| local policy unit | edgecrab-core | is_local omlx | R | missing |
| catalog unit | edgecrab-core | resolve omlx/… | R | missing |
| pricing unit | edgecrab-core | zero cost | R | missing |
| CLI setup list | edgecrab-cli tests if any | contains omlx | S | missing |
| e2e live chat | edgequake-llm | ignore by default | R | missing |
| e2e live tools | edgecrab-core or tools | ignore by default | R | missing |
| geometry e2e local | `local_harness_geometry_e2e.rs` | param omlx or shared | S | lmstudio only |

---

## 11. Security

| Touchpoint | Path | Action | Pri | Status |
|------------|------|--------|-----|--------|
| SSRF | security crate | default host is loopback; custom OMLX_HOST must remain user-intent local — do not open arbitrary SSRF via model id | R | review |
| Secret redaction | redaction pipeline | OMLX_API_KEY | R | verify patterns |
| Path tools | unchanged | local agent still path-safe | — | existing |

**Note:** User-configured `OMLX_HOST` pointing at non-local is a **user choice** (like `OLLAMA_HOST`); document risk; no need to block LAN hosts for homelab.

---

## 12. Priority roll-up (implementation order)

```text
Wave A — edgequake-llm identity + OpenAI chat/tools/stream + list_models
Wave B — EdgeCrab local family DRY + catalog + discovery + pricing
Wave C — CLI setup/doctor/help/selector filters
Wave D — docs/site/README + SDKs
Wave E — e2e live + vision/embeddings polish (P1)
```

Cross-ref: [010-implementation-plan.md](010-implementation-plan.md).

---

## 13. “Done when” matrix (compact)

| Family | Done when |
|--------|-----------|
| Factory | `ProviderFactory::create_llm_provider("omlx", "x")?.name() == "omlx"` |
| Catalog | `/model` lists omlx without live server |
| Discovery | with server, rows = `/v1/models` ids |
| Harness | timeout does not dual-request; tools non-stream |
| Ops | doctor green/red correct |
| Product | setup path works without hand-edited YAML |
