# 015 — Wave D–F Implementation Assessment

**Date:** 2026-07-24  
**Scope:** First-class `llamacpp`, `vllm-mlx`, `mlx-lm` + documentation  
**Verdict:** **Implemented** against [014](014-apple-silicon-local-landscape.md) acceptance; docs/site updated; fmt/clippy required green before merge.

---

## 1. What shipped

### edgequake-llm

| Item | Status | Location |
|------|--------|----------|
| Shared `LocalOpenAiIdentity` + `LocalOpenAiProvider` | Done | `providers/local_openai_common.rs` |
| `llamacpp` thin citizen | Done | `providers/llamacpp.rs` |
| `vllm-mlx` thin citizen | Done | `providers/vllm_mlx.rs` |
| `mlx-lm` thin citizen | Done | `providers/mlx_lm.rs` |
| `ProviderType::{LlamaCpp,VllmMlx,MlxLm}` + factory | Done | `factory.rs` |
| Provider catalog + attribution | Done | `provider_catalog.rs`, `application_context.rs`, `http/attribution.rs` |
| Public re-exports | Done | `lib.rs` |
| Unit tests (builder names, factory aliases) | Done | module + factory tests |

### EdgeCrab core / tools / CLI

| Item | Status | Location |
|------|--------|----------|
| Catalog YAML seeds | Done | `model_catalog_default.yaml` |
| Alias normalize | Done | `model_catalog.rs`, `vision_models.rs` |
| Live discovery (local TTL, optional key) | Done | `model_discovery.rs` `LocalOpenAiDiscovery` |
| `LOCAL_INFERENCE_PROVIDERS` + harness | Done | `local_provider_policy.rs`, tools mutation/registry/progress |
| Zero-cost pricing | Done | `pricing.rs` |
| `/endpoint` rows | Done | `provider_endpoints.rs` |
| Setup list | Done | `setup.rs` |
| Doctor multi-port probes | Done | `doctor.rs` |
| Help / badges / main hints | Done | `commands.rs`, `model_catalog_ui.rs`, `main.rs`, `app.rs` |
| Citizenship e2e | Done | `tests/local_mac_providers_citizenship.rs` |
| Prefix freeze applies via `is_local_inference_provider` | Done | `local_prefix_cache.rs` (no code fork) |

### Documentation

| Surface | Status |
|---------|--------|
| Root `README.md` provider table | Updated |
| `CHANGELOG.md` Unreleased | Updated |
| `docs/feature-docs/02-model-providers.md` | Updated + local family table |
| `AGENTS.md` catalog notes | Updated |
| Site `providers/local.md` | Rewritten (all local citizens) |
| Site `providers/overview.md` | Updated |
| Site env vars / slash / quick-start / changelog | Updated |
| Specs 010 / 014 / README | Wave D–F marked done |
| This assessment | **015** |

---

## 2. Citizenship bar (per id)

For each of `llamacpp`, `vllm-mlx`, `mlx-lm`:

| Criterion | Met? |
|-----------|------|
| Named in catalog + `/model` seed | Yes |
| Factory `create_llm_provider` | Yes |
| Live discovery adapter | Yes |
| Local harness membership | Yes |
| Prefix freeze eligibility | Yes |
| Zero-cost | Yes |
| setup / doctor / help | Yes |
| `/endpoint` metadata | Yes |
| Offline citizenship tests | Yes |
| Optional live e2e env flag | Yes (`*_E2E=1`) |

---

## 3. DRY / SOLID

| Principle | How |
|-----------|-----|
| **DRY** | One shell (`LocalOpenAiProvider`); three identity constants; shared discovery struct; shared local policy list |
| **S** | Identity = product metadata; shell = HTTP OpenAI; EdgeCrab = catalog/TUI policy |
| **O** | New server = new identity + catalog row + discovery static + endpoint row (no ReAct changes) |
| **D** | Factory depends on modules; TUI depends on `provider_endpoints` registry |

**Not done (intentional):** Refactor oMLX/MTPLX onto `LocalOpenAiProvider` (they keep settings-file loaders). Gate from 010 remains satisfied for D–F.

---

## 4. Gaps / non-goals (still open)

| Gap | Priority |
|-----|----------|
| Doctor HTTP fingerprint (distinguish products on same port) | P2 |
| Live chat e2e against real llama-server/vLLM-MLX/mlx_lm (beyond models list) | Optional dogfood |
| Collapse omlx/mtplx onto shared shell | Clean-up, not required |
| Watchlist: Maic, exo, Jan | Per 014 reject/watch |
| Site build deploy | CI/site pipeline when docs merge |

---

## 5. Verification checklist (operators)

```bash
# Format + lint
cargo fmt --all
cargo clippy --workspace -- -D warnings
# (also edgequake-llm path dep)
cd ../edgequake-llm && cargo fmt --all && cargo clippy -- -D warnings

# Citizenship
cargo test -p edgecrab-core --test local_mac_providers_citizenship
cargo test -p edgecrab-core --test omlx_provider_citizenship
cargo test -p edgecrab-core --test mtplx_provider_citizenship

# Optional live
LLAMACPP_E2E=1 cargo test -p edgecrab-core --test local_mac_providers_citizenship llamacpp_e2e
```

---

## 6. Recommendation

Ship Wave D–F as **complete** for product citizenship. Prefer **oMLX** for Mac agent dogfood; advertise **llamacpp** for Hermes/GGUF users; document port collisions clearly (done on site local page). Next investment: shared doctor fingerprint + optional live chat smoke, not more thin wrappers.
