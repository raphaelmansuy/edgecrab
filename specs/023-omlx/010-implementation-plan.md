# 010 — Implementation Plan (DRY / SOLID / First Principles)

**Status:** Ready to execute  
**Order is mandatory** — later waves assume earlier merges.

---

## 0. North star

```text
User types:  edgecrab --model omlx/<id>
System:      factory builds OmlxProvider
             local harness on
             discovery lists models
             doctor/setup know oMLX
             cost = $0
```

No new agent loop branches. No second HTTP stack. **Register + thin adapter + local family membership.**

---

## 1. Architecture (final shape)

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         EdgeCrab surfaces                                │
│  setup · doctor · /model · /models · pricing · conversation · gateway   │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ is_local_inference_provider / catalog
                                 │ create_provider_for_model
┌────────────────────────────────▼────────────────────────────────────────┐
│ edgecrab-core: catalog YAML · OmlxDiscovery · local_provider_policy      │
│ edgecrab-tools: mutation/registry/progress (call shared is_local)        │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ ProviderFactory::create_llm_provider
┌────────────────────────────────▼────────────────────────────────────────┐
│ edgequake-llm: OmlxProvider { inner: OpenAICompatibleProvider }          │
│                ProviderType::Omlx · ProviderCatalog · OmlxDiscovery      │
└────────────────────────────────┬────────────────────────────────────────┘
                                 │ HTTP OpenAI-compatible
┌────────────────────────────────▼────────────────────────────────────────┐
│ oMLX server :8000  /v1/chat/completions  /v1/models  (/v1/embeddings)   │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 2. PR DAG

```text
PR-EQ1  edgequake-llm: OmlxProvider + factory + catalog + unit tests
    │
    ▼
PR-EQ2  edgequake-llm: discovery + docs + optional live e2e
    │
    ▼
PR-EC1  edgecrab: bump edgequake-llm + local family DRY + omlx membership
    │
    ├──────────────┐
    ▼              ▼
PR-EC2           PR-EC3
catalog+         CLI setup/doctor/
discovery+       help/selector +
pricing          main hints
    │              │
    └──────┬───────┘
           ▼
PR-EC4  docs/site/README/CHANGELOG + dogfood sign-off
```

P1 (later): vision heuristics, embeddings polish, Anthropic wire.

---

## 3. Phase detail

### Phase 0 — Spec freeze (this pack)

- [x] Multi-lens specs written under `specs/023-omlx/`  
- [ ] PO accepts canonical id, default port, P0 scope  
- [ ] Confirm edgequake-llm release vs path-dep for EC development  

### Phase 1 — edgequake-llm (PR-EQ1 + PR-EQ2)

| Task | Ref | DoD |
|------|-----|-----|
| `providers/omlx.rs` thin wrap | [006](006-edgequake-llm-plan.md) | chat/tools/stream delegate |
| Factory + ProviderType | 006 | from_str + create |
| ProviderCatalog entry | 006 | resolve_id |
| Discovery module | 006 | Dynamic /v1/models |
| Unit tests offline | [009](009-e2e-test-plan.md) U-EQ-* | green |
| Docs row | 006 | providers.md |
| Version bump | 006 | publish or tag |

**SOLID check:** no edgecrab imports; no LM Studio CLI auto-load.

### Phase 2 — EdgeCrab policy foundation (PR-EC1)

| Task | Ref | DoD |
|------|-----|-----|
| Bump edgequake-llm dep | 007 | builds |
| `LOCAL_INFERENCE_PROVIDERS` + omlx | [007 WS-A](007-edgecrab-plan.md), [001 L2](001-first-principles.md) | is_local("omlx") |
| prefers_nonstreaming includes omlx | 003 | unit |
| timeout env OMLX_* | 004 | unit |
| stall copy omlx | 005 | unit |
| Replace scatter matches in tools | 005 | uses is_local |
| Regression: ollama/lmstudio tests | 009 | still green |

### Phase 3 — Catalog & discovery (PR-EC2)

| Task | Ref | DoD |
|------|-----|-----|
| YAML seed `omlx` | 007 WS-B | catalog loads |
| Aliases | 005 | normalize |
| OmlxDiscovery adapter | 005 | live list |
| ZERO_COST | 005 | pricing |
| Units U-EC-05..10 | 009 | green |

### Phase 4 — Operator surfaces (PR-EC3)

| Task | Ref | DoD |
|------|-----|-----|
| setup list + default | 002 Journey A | selectable |
| doctor HTTP/port | 008 EC-UX-01 | prefer GET /v1/models |
| /provider help | 005 | documented |
| app local refresh filter | 005 | omlx included |
| main key hint | 002 copy | optional key |

### Phase 5 — Docs & launch (PR-EC4)

| Task | Ref | DoD |
|------|-----|-----|
| feature-docs + site local | 007 WS-E | published |
| README table | 002 | row |
| CHANGELOG | — | entry |
| Dogfood L-EC-01..10 | 009 | signed |

### Phase 6 — P1 polish (optional follow-up)

- Vision normalize + routing  
- Embeddings  
- Anthropic `/v1/messages` adapter (only if needed)  
- migrate heuristic from generic :8000  

---

## 4. DRY refactor rules (during PR-EC1)

**Allowed duplicate:** human-readable stall strings per provider.  
**Forbidden duplicate:** separate booleans for “is this local?” in each crate.

Preferred dependency for tools → core:

- If tools cannot depend on core, put `is_local_inference_provider` in **edgecrab-types** or a tiny shared module both can use.  
- Today `local_provider_policy` lives in **core** and tools has its own matches — **unifying is part of this work’s value**.

Minimal acceptable fix if crate cycles block ideal placement:

```text
// edgecrab-types or edgecrab-tools/local_ids.rs
pub fn is_local_inference_provider(name: &str) -> bool { … }

// edgecrab-core re-exports or calls the same function
```

Do not leave tools and core with divergent lists.

---

## 5. First principles checklist (per PR)

| Law | PR must preserve |
|-----|------------------|
| L1 One id | only `"omlx"` in runtime name() |
| L2 Open/Closed | family list extended, not loop forked |
| L3 Composition | OpenAICompatible inner |
| L4 Fail closed net / open catalog | discovery empty ≠ remove provider |
| L5 Harness safety | no dual-request on timeout |
| L6 Protocol minimal | OpenAI path only in P0 |
| L7 Platform honesty | no macos-only compile of provider |
| L8 Zero cost | pricing updated same PR as catalog or before launch |
| L9 Local discovery OK | /v1/models |
| L10 Tests | offline units in same PR as code |

---

## 6. Risk register

| Risk | Mitigation | Owner |
|------|------------|-------|
| edgequake-llm release lag | path dep short-term | eng |
| Tool models flaky on MLX | document families; non-stream tools | AI eng |
| Doctor false positive on :8000 | HTTP health not TCP-only | eng |
| Scatter matches miss a site | touchpoint matrix + rg audit | eng |
| Profile `:` breaks split | opaque model after first `/` | eng |

**rg audit (exit criteria):**

```bash
# Should only appear for intentional strings/copy after refactor:
rg -n 'lmstudio" \| "ollama|"ollama" \| "lmstudio' crates/
# omlx should appear in catalog, policy, setup, doctor, discovery, factory path
rg -n 'omlx' crates/ edgequake-llm/src/
```

---

## 7. Definition of Done (initiative)

### Wave A — oMLX (primary)

- [x] All P0 rows in [005-touchpoint-matrix.md](005-touchpoint-matrix.md) for oMLX  
- [x] Offline tests U-EQ + U-EC green  
- [ ] Dogfood checklist 009 L-EC-01..10 on a Mac with oMLX (operator)  
- [ ] crates.io edgequake-llm bump published (path dep OK for local dev)  
- [x] Spec README oMLX status **Implemented**

### Wave B — MTPLX (same citizenship bar)

- [x] Execute [012-mtplx-first-class.md](012-mtplx-first-class.md)  
- [x] `mtplx` in factory, catalog, discovery, local family, pricing  
- [x] Settings-aware port (`Application Support/MTPLX/settings.json`)  
- [x] `/model` lists live models when server up (+ FS cache fallback)  
- [x] `/endpoint` + doctor + setup  
- [x] Offline citizenship tests + optional `MTPLX_E2E=1` live  
- [x] Docs: oMLX **and** MTPLX in local providers guide  
- [x] Shared `local_openai_common` helpers (DRY with oMLX)

### Wave C — Local prefix / KV stability (July 2026)

- [x] Spec [013-local-prefix-cache-july-2026.md](013-local-prefix-cache-july-2026.md)  
- [x] `local_prefix_cache.rs` — freeze annotated tool wire schemas for local providers  
- [x] Wire `conversation.rs` + clear on tool-set change / model transfer  
- [x] E2E `tests/local_prefix_cache_e2e.rs`  
- [ ] Operator dogfood: confirm MTPLX miss reason ≠ `prefix_divergence_at_token` on multi-tool turns with fixed tool set  
- [ ] P1: deferred tools / smaller CORE on local; shelf “tools frozen” badge  

### Wave D–F — More Mac citizens ([014](014-apple-silicon-local-landscape.md))

- [x] **D P0:** full **`llamacpp` / llama-server** citizen (catalog + factory + discovery :8080 + setup/doctor)  
- [x] **E P1:** **`vllm-mlx`** named citizen (prefix/batching story; port docs)  
- [x] **F P1:** **`mlx-lm`** thin citizen + multi-port doctor + `/endpoint` rows  
- [x] Shared **`LocalOpenAiProvider` + `LocalOpenAiIdentity`** in edgequake-llm  
- [x] Citizenship e2e: `local_mac_providers_citizenship.rs` (+ optional `LLAMACPP_E2E` / `VLLM_MLX_E2E` / `MLX_LM_E2E`)  
- [x] Reject/watch list per 014 remains non-first-class (Jan, Maic, Kobold, Apple FM, …)

**DRY gate for Wave B:** if `mtplx.rs` is a near-copy of `omlx.rs` beyond ~80 LOC of deltas, extract shared `LocalOpenAiServer` resolve/list helpers first.  
**DRY gate for D–F:** new ids must be thin registrations on `local_openai_common` + local family — no new ReAct branches. **Met** via `LocalOpenAiIdentity`.

---

## 8. Suggested commit/PR titles

```text
feat(edgequake-llm): add OmlxProvider (OpenAI-compatible local MLX)
feat(edgequake-llm): omlx model discovery + docs
feat(edgecrab): treat omlx as local inference family (DRY)
feat(edgecrab): catalog + live discovery for omlx
feat(edgecrab): setup/doctor/TUI surfaces for omlx
docs: oMLX first-class provider guide
```

---

## 9. Cross-lens acceptance (sign-off)

| Lens | Sign-off question | Doc |
|------|-------------------|-----|
| Product Owner | Can a Mac user pick oMLX in setup and finish a tool task without YAML? | [002](002-product-owner-lens.md) |
| AI Engineer | Does local harness physics apply (no dual GEN, tools work)? | [003](003-ai-engineer-lens.md) |
| Rust Expert | Thin adapter, single family source, clippy clean, no crate cycles? | [004](004-rust-expert-lens.md) |

All three must say **yes** before calling citizenship complete.
