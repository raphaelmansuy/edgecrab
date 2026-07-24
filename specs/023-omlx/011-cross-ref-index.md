# 011 — Cross-Reference Index

Quick anchors between this pack, code, and external systems.

---

## 1. Spec pack internal graph

```text
README  (family: oMLX shipped · MTPLX planned)
  ├─ 000 code-is-law          (oMLX evidence)
  ├─ 001 first-principles     (laws L1–L10 — both providers)
  ├─ 002 product-owner        (oMLX JTBD)
  ├─ 003 ai-engineer          (oMLX harness)
  ├─ 004 rust-expert          (oMLX types)
  ├─ 005 touchpoint-matrix    (every surface — both)
  ├─ 006 edgequake-llm plan   (oMLX EQL-*)
  ├─ 007 edgecrab plan        (oMLX WS-*)
  ├─ 008 edge-cases           (oMLX)
  ├─ 009 e2e tests            (oMLX)
  ├─ 010 implementation plan  (Wave A oMLX · Wave B MTPLX)
  ├─ 011 cross-ref-index      (this file)
  ├─ 012 mtplx-first-class    (MTPLX multi-lens + plan)
  ├─ 013 local-prefix-cache   (KV freeze · July 2026)
  └─ 014 apple-silicon-landscape (other Mac servers · P0–P3)
```

---

## 2. Code anchors (EdgeCrab)

| Concern | Path |
|---------|------|
| Catalog YAML | `crates/edgecrab-core/src/model_catalog_default.yaml` |
| Catalog resolve | `crates/edgecrab-core/src/model_catalog.rs` |
| Discovery | `crates/edgecrab-core/src/model_discovery.rs` |
| Local policy | `crates/edgecrab-core/src/local_provider_policy.rs` |
| Pricing | `crates/edgecrab-core/src/pricing.rs` |
| Conversation loop | `crates/edgecrab-core/src/conversation.rs` |
| Provider call | `crates/edgecrab-core/src/provider_call.rs` |
| Model transfer | `crates/edgecrab-core/src/model_transfer.rs` |
| Factory bridge | `crates/edgecrab-tools/src/provider_factory.rs` |
| Mutation policy | `crates/edgecrab-tools/src/mutation_turn_policy.rs` |
| Tool registry local annotate | `crates/edgecrab-tools/src/registry.rs` |
| Progress tail | `crates/edgecrab-tools/src/tool_progress_tail.rs` |
| Vision normalize | `crates/edgecrab-tools/src/vision_models.rs` |
| Setup | `crates/edgecrab-cli/src/setup.rs` |
| Doctor | `crates/edgecrab-cli/src/doctor.rs` |
| Commands help | `crates/edgecrab-cli/src/commands.rs` |
| App selector | `crates/edgecrab-cli/src/app.rs` |
| Feature docs | `docs/feature-docs/02-model-providers.md` |
| Site local | `site/src/content/docs/providers/local.md` |
| Local harness e2e | `crates/edgecrab-core/tests/local_harness_geometry_e2e.rs` |
| oMLX citizenship | `crates/edgecrab-core/tests/omlx_provider_citizenship.rs` |
| Provider endpoints TUI | `crates/edgecrab-cli/src/provider_endpoint_overlay.rs` |
| Shipped oMLX provider | `edgequake-llm/src/providers/omlx.rs` |
| Planned MTPLX provider | `edgequake-llm/src/providers/mtplx.rs` (012) |

---

## 3. Code anchors (edgequake-llm)

| Concern | Path |
|---------|------|
| LM Studio template | `src/providers/lmstudio.rs` |
| OpenAI compatible core | `src/providers/openai_compatible.rs` |
| Ollama | `src/providers/ollama.rs` |
| Factory | `src/factory.rs` |
| Provider catalog | `src/provider_catalog.rs` |
| Discovery registry | `src/discovery/` |
| LM Studio discovery | `src/discovery/providers/lmstudio.rs` |
| Providers doc | `docs/providers.md` |
| E2E LM Studio | `tests/e2e_lmstudio_*.rs` |
| E2E Ollama | `tests/e2e_ollama_*.rs` |

**Planned new:**

| Concern | Path |
|---------|------|
| Provider | `src/providers/omlx.rs` |
| Discovery | `src/discovery/providers/omlx.rs` |
| E2E | `tests/e2e_omlx_openai_compatible.rs` |

---

## 4. External systems

| System | Link / note |
|--------|-------------|
| oMLX product | https://omlx.ai |
| oMLX GitHub | https://github.com/jundot/omlx |
| Default API | `http://localhost:8000/v1` |
| Hermes Mac guide | `hermes-agent/website/docs/guides/local-llm-on-mac.md` |
| Hermes local timeout | `hermes-agent/agent/chat_completion_helpers.py` (~local endpoint stale timeout) |
| Hermes vision omlx fixture | `hermes-agent/tests/tools/test_computer_use_vision_routing.py` |

---

## 5. Related EdgeCrab specs

| Spec | Why |
|------|-----|
| `specs/dynamic_model/*` | Discovery + TUI truthfulness |
| `specs/014-improve-local-harness/*` | Local tool harness physics |
| `specs/022-ai-agent-gap/*` | Strategy vs Hermes |
| `docs/feature-docs/02-model-providers.md` | Provider architecture deep dive |

---

## 6. Requirement ID quick index

| ID prefix | Meaning | Home |
|-----------|---------|------|
| GAP-* | Current gaps | 000 |
| L1–L10 | Design laws | 001 |
| D1–D10 | Locked decisions (oMLX) | 001 |
| PO-M/S/C/W | Product MoSCoW (oMLX) | 002 |
| PO-MTP-* | Product MoSCoW (MTPLX) | 012 |
| AE-T* | AI engineer acceptance (oMLX) | 003 |
| EQL-* | edgequake-llm tasks (oMLX) | 006 |
| EQL-MTP-* | edgequake-llm tasks (MTPLX) | 012 |
| WS-* / EC-MTP-* | EdgeCrab workstreams | 007 / 012 |
| EC-* | Edge cases (oMLX) | 008 |
| U-*/I-*/L-* | Tests (oMLX) | 009 |
| U-MTP-* / L-MTP-* | Tests (MTPLX) | 012 |
| PR-EQ*/PR-EC* | PRs (oMLX) | 010 |
| PR-EQ-MTP / PR-EC-MTP* | PRs (MTPLX) | 012 |
| D-MTP-* | Locked decisions (MTPLX) | 012 |

---

## 7. Env var cheat sheet

### oMLX (shipped)

| Var | Default | Layer |
|-----|---------|-------|
| `OMLX_HOST` / `OMLX_BASE_URL` | settings or `http://127.0.0.1:9050` | both |
| `OMLX_MODEL` | settings / default | eq-llm |
| `OMLX_API_KEY` | settings `auth.api_key` or unset | eq-llm |
| `OMLX_TIMEOUT_SECONDS` | `600` | both |
| `OMLX_E2E` | unset | tests |

### MTPLX (planned — 012)

| Var | Default | Layer |
|-----|---------|-------|
| `MTPLX_HOST` / `MTPLX_BASE_URL` | settings.port or `http://127.0.0.1:8000` | both |
| `MTPLX_MODEL` | settings `model` | eq-llm |
| `MTPLX_API_KEY` | optional | eq-llm |
| `MTPLX_TIMEOUT_SECONDS` | `600` | both |
| `MTPLX_E2E` | unset | tests |
| `MTPLX_SETTINGS` | override settings path | both |

---

## 8. Citizen checklist

### oMLX

- [x] `ProviderType::Omlx` + `name() == "omlx"`
- [x] Catalog YAML `omlx`
- [x] Live discovery (+ settings key/port)
- [x] `is_local_inference_provider("omlx")`
- [x] Zero cost
- [x] setup + doctor + /model + /endpoint
- [x] Offline units
- [ ] Live dogfood tools (operator)
- [x] Docs

### MTPLX ([012](012-mtplx-first-class.md))

- [x] `ProviderType::Mtplx` + `name() == "mtplx"`
- [x] Catalog YAML `mtplx`
- [x] Live discovery (+ settings host/port + FS cache fallback)
- [x] `is_local_inference_provider("mtplx")`
- [x] Zero cost
- [x] setup + doctor + /model + /endpoint
- [x] Offline units + optional live e2e (`MTPLX_E2E=1`)
- [ ] Dogfood multi-tool turn (operator — needs `mtplx quickstart`)
- [x] Docs (side-by-side with oMLX)

---

## 9. External product anchors

| Product | Path / command |
|---------|----------------|
| oMLX settings | `~/.omlx/settings.json` |
| MTPLX settings | `~/Library/Application Support/MTPLX/settings.json` |
| MTPLX CLI | `…/MTPLX/runtime-venv/bin/mtplx` |
| MTPLX models | `~/.mtplx/models` |
| Start MTPLX API | `mtplx quickstart --host 127.0.0.1 --port <P>` |
| Hermes via MTPLX | `mtplx start hermes --port 18085` |