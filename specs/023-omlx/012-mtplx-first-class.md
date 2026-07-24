# 012 — MTPLX as a First-Class Provider Citizen

**Status:** Implemented (2026-07-24)  
**Date:** 2026-07-24  
**Parity target:** Same citizenship bar as **oMLX** (this pack 000–011 + shipped code)  
**Subject:** [MTPLX](https://mtplx.app) — native MTP speculative decoding on Apple Silicon  
**Evidence host (2026-07-24):** macOS app `com.youssofal.mtplx` v2.3.0, bundle `/Applications/MTPLX.app`

---

## 0. Why MTPLX belongs in this pack

023 started as **oMLX**. Both products are **Mac-local OpenAI-compatible inference servers** optimized for agent workloads on Apple Silicon. Implementing MTPLX by cloning oMLX line-by-line would violate DRY.

**First principle:** oMLX and MTPLX are two **instances** of the same abstract role:

```text
LocalAppleSiliconServer {
  id, aliases, default_host, settings_file, env_prefix,
  list_models: GET /v1/models,
  chat: POST /v1/chat/completions,
  optional_api_key, local_harness: true, zero_cost: true
}
```

Ship MTPLX as the **second registration** of that family — not a third HTTP stack.

| | oMLX (shipped) | MTPLX (this doc) |
|--|----------------|------------------|
| Product | omlx.ai / jundot/omlx | MTPLX app (Youssofal) |
| Differentiator | SSD KV cache, multi-model admin | Native **MTP** speculative decode |
| Wire | OpenAI (+ Anthropic) | OpenAI (+ Anthropic clients via `mtplx connect`) |
| EdgeCrab role | First-class local provider | **Same** first-class bar |

---

## 1. Code-is-law: MTPLX facts (verified)

### 1.1 Product surface

| Fact | Value |
|------|--------|
| App name | **MTPLX** |
| Bundle ID | `com.youssofal.mtplx` |
| Bundle path | `/Applications/MTPLX.app` |
| Version (sample) | **2.3.0** (build 23000) |
| Positioning | “native MTP · Apple Silicon” — fast local AI |
| CLI (app-managed) | `~/Library/Application Support/MTPLX/runtime-venv/bin/mtplx` |
| Model cache | `~/.mtplx/models` |
| Settings file | `~/Library/Application Support/MTPLX/settings.json` |

### 1.2 Settings schema (relevant keys)

From live `settings.json` (user machine):

| Key | Role | Sample |
|-----|------|--------|
| `host` | Bind host | `127.0.0.1` |
| `port` | API port | **`8002`** (this host; CLI docs often show **8000**) |
| `model` | Last/active model path or id | path under `~/.mtplx/models/…` |
| `context_window` | Context tokens | `131072` |
| `generation_mode` | e.g. `mtp` | MTP on |
| `reasoning` / `reasoning_effort` | local reasoning mode | `auto` |
| `api_key` (if present) | optional auth | via CLI `--api-key` / `--api-key-file` |

**Resolution law (same pattern as oMLX):**

```text
MTPLX_HOST / MTPLX_BASE_URL
  → settings.json host:port
  → compiled default http://127.0.0.1:8000
```

Do **not** hardcode only 8000 or only 8002 — **settings win**.

### 1.3 CLI / API contract

| Command / surface | Meaning for EdgeCrab |
|-------------------|----------------------|
| `mtplx quickstart --host 127.0.0.1 --port <P>` | Starts OpenAI-compatible server |
| `mtplx start hermes --port 18085` | Competitor already integrates MTPLX for Hermes |
| `mtplx connect {openwebui,claude-code,opencode,swival}` | Emits client snippets (OpenAI base URL) |
| `mtplx models` | Lists **cached** models (filesystem), not necessarily loaded |
| `mtplx status` | Install / runtime health |
| `mtplx stop` | Stop daemon on a port |

**P0 HTTP (when server is running):**

| Method | Path | Use |
|--------|------|-----|
| `GET` | `/v1/models` | Live discovery (preferred for picker) |
| `POST` | `/v1/chat/completions` | ReAct chat + tools |
| `GET` | health if documented | Doctor (optional) |

**P1:** Anthropic-shaped clients only if EdgeCrab needs `/v1/messages` to MTPLX (same deferral as oMLX).

### 1.4 Model identity shape

Cached models look like:

```text
Youssofal/Qwen3.5-9B-MTPLX-Optimized-Speed
Youssofal/Qwen3.6-27B-MTPLX-Optimized-Speed
Youssofal/Qwen3.6-35B-A3B-MTPLX-Optimized-Speed
```

Filesystem dirs may use `--` instead of `/` (`Youssofal--Qwen3.6-…`).  
**Opaque model id rule:** after first `/` in `mtplx/<id>`, treat the rest as opaque (same as omlx multi-segment + profiles).

### 1.5 EdgeCrab / edgequake-llm today

```text
rg -i 'mtplx|mtp-lx'  →  expect ZERO (as of 2026-07-24)
```

MTPLX is **not** a citizen. Users must abuse generic OpenAI-compatible config or oMLX-shaped env hacks.

---

## 2. First principles (MTPLX-specific + family)

### 2.1 Ontology

```text
MTPLX.app / mtplx daemon  ──OpenAI HTTP──▶  MtplxProvider (edgequake-llm)
                                                 │
                                                 ▼
                              EdgeCrab catalog · discovery · local harness · TUI
```

### 2.2 Laws (extend pack 001 L1–L10)

| Law | MTPLX application |
|-----|-------------------|
| **L1** | Canonical id **`mtplx`** only; aliases normalize at boundary |
| **L2** | Add `"mtplx"` to `LOCAL_INFERENCE_PROVIDERS` — **one line**, no new conversation forks |
| **L3** | `MtplxProvider` = thin wrap of `OpenAICompatibleProvider` (clone **OmlxProvider shape**, not LM Studio CLI) |
| **L4** | Unreachable → empty live list + doctor red; static seed always visible |
| **L5** | Full local harness (no dual-request, non-stream tools, 600s timeout) |
| **L6** | P0 OpenAI only |
| **L7** | Server = Apple Silicon; client OS-agnostic |
| **L8** | Zero-cost provider |
| **L9** | Live `/v1/models` when server up; optional FS cache list as **fallback inventory** (see §4.3) |
| **L10** | Offline unit tests + opt-in live e2e |

### 2.3 DRY abstraction (do this once, use for omlx + mtplx)

Prefer a single internal helper (name illustrative):

```rust
// edgequake-llm (conceptual)
struct LocalOpenAiServerSpec {
    id: &'static str,                 // "omlx" | "mtplx"
    default_host: &'static str,       // with port
    env_host_keys: &'static [&str],   // OMLX_HOST / MTPLX_HOST …
    env_key_keys: &'static [&str],    // OMLX_API_KEY / MTPLX_API_KEY
    settings_paths: &'static [fn() -> Option<PathBuf>],
    settings_host_port: fn(&Value) -> Option<(String, u16)>,
    settings_api_key: fn(&Value) -> Option<String>,
}
```

**OCP:** Adding the next Mac server (future product) = one `LocalOpenAiServerSpec` row + catalog seed + discovery adapter.

**Anti-pattern:** Copy-paste `omlx.rs` → `mtplx.rs` with only string renames and no shared resolve/list helpers.

---

## 3. Multi-lens requirements

### 3.1 Product Owner

| ID | Requirement | Acceptance |
|----|-------------|------------|
| PO-MTP-M1 | Selectable as `mtplx/<model>` | setup, `--model`, `/model` |
| PO-MTP-M2 | Live list when daemon up | `/models mtplx`, selector “live discovery” |
| PO-MTP-M3 | Chat + tools ReAct | local harness e2e / dogfood |
| PO-MTP-M4 | Zero cost | `/cost` |
| PO-MTP-M5 | Doctor sees MTPLX | port from settings/env; key status |
| PO-MTP-M6 | Docs: local Mac providers | oMLX **and** MTPLX side-by-side |
| PO-MTP-M7 | Optional API key | env + settings; no secret logging |
| PO-MTP-S1 | Settings auto-read | `Application Support/MTPLX/settings.json` |
| PO-MTP-S2 | FS model cache fallback | if `/v1/models` empty but `~/.mtplx/models` has dirs |
| PO-MTP-S3 | `/endpoint` row | base URL override for `mtplx` |
| PO-MTP-W1 | Bundle MTPLX.app | **out of scope** |
| PO-MTP-W2 | MTP depth tuning UI | leave to MTPLX app |

**Copy:**

| Surface | Text |
|---------|------|
| Setup | `mtplx` — **MTPLX (local MTP on Apple Silicon, free)** |
| Doctor up | `MTPLX: reachable at {host} ({n} models)` |
| Doctor down | `MTPLX: not reachable — run \`mtplx quickstart\` or start the app server` |
| Help | `mtplx — MTPLX (local, MTPLX_HOST, default :8000 or settings.port)` |

**Journeys:** same as oMLX (cold start / discover / server down) with MTPLX branding and `mtplx` id.

### 3.2 AI Engineer

| Concern | Policy |
|---------|--------|
| Tool turns | Non-streaming preferred; `tool_choice` / max_tokens local policy |
| Timeout | `MTPLX_TIMEOUT_SECONDS` default **600** |
| Retry | Block transport retry on timeout (orphan gen) |
| Prefill | Same structural prune / mid-band compress as other locals |
| Reasoning | Honor local `reasoning_effort` when model supports; force `none` on tool turns if family policy says so |
| Context | Prefer settings `context_window` (e.g. 131072) as catalog fallback |
| Failover | Never silent cloud fallback (privacy) |
| Discovery | Prefer `GET /v1/models`; if server down, optional **offline catalog** from `mtplx models` / FS (label as `cache` or `static`, never fake `live`) |

### 3.3 Rust Expert

| Layer | Work |
|-------|------|
| **edgequake-llm** | `providers/mtplx.rs` thin provider; `discovery/providers/mtplx.rs`; factory + catalog descriptor; `resolve_mtplx_runtime_config()` |
| **edgecrab-core** | YAML seed; `MtplxDiscovery` adapter (or shared local OpenAI adapter with MTPLX resolve); `LOCAL_INFERENCE_PROVIDERS` += `mtplx`; pricing zero-cost; `provider_endpoints` row |
| **edgecrab-tools** | local match lists + progress tail copy for `mtplx` |
| **edgecrab-cli** | setup, doctor, `/model` help, selector badge `local MTP` |
| **Tests** | offline factory/catalog/policy; live `MTPLX_E2E=1` |

**Env contract:**

| Variable | Default / notes |
|----------|-----------------|
| `MTPLX_HOST` / `MTPLX_BASE_URL` | settings or `http://127.0.0.1:8000` |
| `MTPLX_MODEL` | settings `model` basename / last model |
| `MTPLX_API_KEY` | optional |
| `MTPLX_TIMEOUT_SECONDS` | `600` |
| `MTPLX_E2E` | live tests |

**Settings paths (in order):**

1. `$MTPLX_SETTINGS` if set  
2. `~/Library/Application Support/MTPLX/settings.json` (macOS)  
3. `$XDG_CONFIG_HOME/mtplx/settings.json` (future / non-Mac docs only)

---

## 4. Implementation plan (DRY with shipped oMLX)

### 4.1 Reuse checklist (do not re-litigate)

| Capability | Reuse from oMLX | MTPLX-only delta |
|------------|-----------------|------------------|
| Thin OpenAICompatible wrap | Yes | Default port/settings path |
| Local harness membership | Yes | id string `mtplx` |
| `/endpoint` TUI | Yes | new row in `PROVIDER_ENDPOINT_SPECS` |
| Zero cost | Yes | id string |
| Live discovery GET /v1/models | Yes | resolve host/key from MTPLX settings |
| Fuzzy rank provider prefix | Yes | badge `local MTP` |
| Drop static `default` when live | Yes | same merge rule |
| Anthropic wire | P1 same | — |

### 4.2 edgequake-llm tasks (EQL-MTP)

| Step | File / area | Work |
|------|-------------|------|
| EQL-MTP-1 | `providers/mtplx.rs` | Provider + builder + `resolve_mtplx_runtime_config` |
| EQL-MTP-2 | Shared local helper (optional refactor) | Extract common “local OpenAI server” resolve/list if omlx+mtplx duplicate > ~80 LOC |
| EQL-MTP-3 | `factory.rs` / `provider_catalog.rs` | `ProviderType::Mtplx`, from_str, create_* |
| EQL-MTP-4 | `discovery/providers/mtplx.rs` | Dynamic discovery |
| EQL-MTP-5 | docs/providers.md | Feature table row |
| EQL-MTP-6 | `tests/e2e_mtplx_openai_compatible.rs` | `#[ignore]` live |
| EQL-MTP-7 | Version bump | consume from EdgeCrab |

### 4.3 EdgeCrab tasks (EC-MTP)

| Step | Work |
|------|------|
| EC-MTP-1 | Bump edgequake-llm |
| EC-MTP-2 | `LOCAL_INFERENCE_PROVIDERS` += `mtplx` |
| EC-MTP-3 | Catalog YAML seed `mtplx` / `default` |
| EC-MTP-4 | Discovery adapter (local TTL); **optional** FS fallback from `~/.mtplx/models` labeled non-live |
| EC-MTP-5 | pricing / vision normalize aliases |
| EC-MTP-6 | setup + doctor + help |
| EC-MTP-7 | `provider_endpoints` default `http://127.0.0.1:8000` + description “settings.port overrides” |
| EC-MTP-8 | Selector badge `local MTP` |
| EC-MTP-9 | site/README/feature-docs local providers |
| EC-MTP-10 | citizenship tests + live dogfood |

### 4.4 FS fallback discovery (optional but high UX)

When `GET /v1/models` fails (daemon stopped) but models exist on disk:

```text
source = Cache or Static
models = basenames of ~/.mtplx/models/*
detail = "model cache (server offline)"
```

Never claim `live discovery` for FS-only lists.  
User can still select an id; chat will fail with actionable “start mtplx quickstart” until server is up.

### 4.5 PR order

```text
PR-EQ-MTP   edgequake-llm MtplxProvider + discovery + tests
    │
    ▼
PR-EC-MTP1  EdgeCrab family + catalog + discovery + pricing
    │
    ▼
PR-EC-MTP2  CLI setup/doctor/selector/endpoint + docs
    │
    ▼
PR-EC-MTP3  dogfood + CHANGELOG
```

Estimate: **~1.5–2 days** if oMLX path is reused; **+0.5 day** if extracting shared local-server helper.

---

## 5. Edge cases

| ID | Case | Expected |
|----|------|----------|
| EC-MTP-01 | Settings port 8002, default 8000 | Resolve **8002** from settings |
| EC-MTP-02 | Server not started | Empty live; static seed; doctor red |
| EC-MTP-03 | API key required | 401 → message set `MTPLX_API_KEY` |
| EC-MTP-04 | Model path with `--` vs `/` | Normalize or pass opaque id server accepts |
| EC-MTP-05 | Multi-segment `mtplx/Youssofal/Qwen…` | lenient resolve |
| EC-MTP-06 | Hermes port 18085 | User may set `MTPLX_HOST=http://127.0.0.1:18085` via `/endpoint` |
| EC-MTP-07 | oMLX and MTPLX both up | Distinct ids; no port collision in defaults (9050 vs 8000/8002) |
| EC-MTP-08 | Type `mtp` in picker | Rank `mtplx` above unrelated substrings |
| EC-MTP-09 | Long MTP prefill | 600s timeout; no dual retry |
| EC-MTP-10 | Non-Mac CI | Compiles; discovery empty |

---

## 6. E2E / test plan

### Offline (required)

| ID | Assert |
|----|--------|
| U-MTP-01 | `from_str("mtplx"|"mtp-lx")` → Mtplx |
| U-MTP-02 | `create_llm_provider("mtplx", …).name() == "mtplx"` |
| U-MTP-03 | `is_local_inference_provider("mtplx")` |
| U-MTP-04 | catalog has `mtplx`; resolve multi-segment |
| U-MTP-05 | discovery registry contains `mtplx` |
| U-MTP-06 | settings parse: host+port → base URL |
| U-MTP-07 | zero cost |
| U-MTP-08 | endpoint default + override |

### Live (opt-in)

```bash
# terminal A
mtplx quickstart --profile sustained --host 127.0.0.1 --port 8000

# terminal B
MTPLX_E2E=1 cargo test -p edgequake-llm --test e2e_mtplx_openai_compatible -- --ignored
# EdgeCrab dogfood
./target/debug/edgecrab
# /model → type mtplx → pick live id → multi-tool turn
```

| ID | Case |
|----|------|
| L-MTP-01 | list_models non-empty |
| L-MTP-02 | chat pong |
| L-MTP-03 | tools multi-round |
| L-MTP-04 | doctor green |
| L-MTP-05 | `/cost` $0 |

---

## 7. Definition of first-class (same bar as oMLX)

MTPLX is first-class iff **all** hold:

1. **Named** `mtplx` in factory + catalog + help  
2. **Selectable** in setup + `/model`  
3. **Discoverable** live when server up  
4. **Runnable** chat + tools through ReAct  
5. **Policed** local harness  
6. **Diagnosable** doctor  
7. **Priced** zero-cost  
8. **Overridable** `/endpoint`  
9. **Documented** local Mac providers  
10. **Tested** offline + optional live  

---

## 8. Comparison: oMLX vs MTPLX (operator guide)

| Dimension | oMLX | MTPLX |
|-----------|------|-------|
| Strength | SSD KV for agent context thrash | MTP speculative decode speed |
| Typical port | **9050** (settings-aware) | **8000** docs / **settings.port** (e.g. 8002) |
| Settings | `~/.omlx/settings.json` | `~/Library/Application Support/MTPLX/settings.json` |
| Models dir | `~/.omlx/models` | `~/.mtplx/models` |
| Multi-model admin | Strong | Single primary model + cache |
| Hermes | Guide-level | First-class `mtplx start hermes` |
| EdgeCrab id | `omlx` (shipped) | `mtplx` (this spec) |

Both must appear in `/model` without fighting each other.

---

## 9. Cross-refs

| Doc | Relationship |
|-----|----------------|
| [README](README.md) | Pack index — family status |
| [000](000-code-is-law.md) | oMLX evidence (pattern to mirror) |
| [001](001-first-principles.md) | Shared laws L1–L10 |
| [005](005-touchpoint-matrix.md) | Surfaces — add `mtplx` column when implementing |
| [006](006-edgequake-llm-plan.md) | Library template |
| [010](010-implementation-plan.md) | oMLX PR DAG — MTPLX is Wave-next |
| Shipped code | `edgequake-llm/src/providers/omlx.rs`, `edgecrab-core/src/model_discovery.rs` `OmlxDiscovery` |

---

## 10. Locked decisions (MTPLX)

| ID | Decision |
|----|----------|
| D-MTP-1 | Canonical id `mtplx` |
| D-MTP-2 | Thin OpenAI-compatible provider |
| D-MTP-3 | Settings-aware host/port (never assume single port) |
| D-MTP-4 | Local family + zero cost + harness |
| D-MTP-5 | Optional FS cache inventory when API down |
| D-MTP-6 | No bundling of MTPLX.app |
| D-MTP-7 | Implement **after** oMLX path is stable (reuse resolve/discovery/UI patterns) |
| D-MTP-8 | Prefer shared `LocalOpenAiServer` helper if second copy exceeds ~80 LOC |

---

## 11. Acceptance checklist (post-implementation)

- [ ] `ProviderFactory::create_llm_provider("mtplx", …)?.name() == "mtplx"`  
- [ ] Catalog + `/model` shows `mtplx`  
- [ ] Live models with `mtplx quickstart` running  
- [ ] Settings port (e.g. 8002) honored without env  
- [ ] `is_local_inference_provider("mtplx")`  
- [ ] Doctor green/red correct  
- [ ] `/endpoint` can set MTPLX base URL  
- [ ] Citizenship tests green offline  
- [ ] Dogfood multi-tool coding turn  
- [ ] README / feature-docs local table includes MTPLX  
