# 009 — E2E & Test Plan

**Laws:**  
1. Default CI is **offline** (no oMLX required).  
2. Live tests are **opt-in** (`OMLX_E2E=1` or `#[ignore]`).  
3. Prefer **fixtures + wiremock** over brittle live assertions when possible.  
4. Mirror patterns from `e2e_lmstudio_*.rs` / `e2e_ollama_*.rs` / `local_harness_geometry_e2e.rs`.

---

## 1. Test pyramid

```text
                    ┌─────────────┐
                    │ Live e2e    │  rare, Mac + oMLX
                    │ chat/tools  │
                ┌───┴─────────────┴───┐
                │ Integration         │  wiremock / mock provider
                │ factory+discovery   │
            ┌───┴─────────────────────┴───┐
            │ Unit (bulk)                   │
            │ identity · policy · catalog   │
            └───────────────────────────────┘
```

---

## 2. Offline unit tests (required)

### 2.1 edgequake-llm

| ID | Test name (suggested) | Asserts |
|----|----------------------|---------|
| U-EQ-01 | `provider_type_from_str_omlx` | aliases → Omlx |
| U-EQ-02 | `omlx_canonical_id` | `"omlx"` |
| U-EQ-03 | `create_llm_provider_omlx_name` | `.name() == "omlx"` |
| U-EQ-04 | `omlx_host_normalize` | strip `/v1`, trailing slash |
| U-EQ-05 | `omlx_discovery_parse_fixture` | ids + profiles |
| U-EQ-06 | `provider_catalog_has_omlx` | resolve_id + features |

### 2.2 edgecrab-core

| ID | Test | Asserts |
|----|------|---------|
| U-EC-01 | `is_local_inference_provider_omlx` | true |
| U-EC-02 | `omlx_blocks_timeout_retry` | blocks_transport_retry |
| U-EC-03 | `omlx_prefers_nonstreaming_tools` | true when tools |
| U-EC-04 | `omlx_timeout_env` | OMLX_TIMEOUT_SECONDS |
| U-EC-05 | `catalog_resolve_omlx_nested` | multi-segment model |
| U-EC-06 | `catalog_resolve_omlx_profile` | `:` preserved |
| U-EC-07 | `discovery_providers_include_omlx` | list contains |
| U-EC-08 | `normalize_discovery_provider_omlx_alias` | o-mlx → omlx |
| U-EC-09 | `zero_cost_omlx` | pricing path $0 |
| U-EC-10 | `lenient_resolve_unknown_live_id` | like lmstudio test |

### 2.3 edgecrab-tools

| ID | Test | Asserts |
|----|------|---------|
| U-ET-01 | `local_annotate_gate_includes_omlx` | annotate path runs |
| U-ET-02 | `progress_tail_omlx_timeout_env` | OMLX_TIMEOUT_SECONDS |
| U-ET-03 | `stall_notice_contains_omlx` | copy |
| U-ET-04 | `vision_normalize_omlx` | aliases (P1) |

### 2.4 edgecrab-cli (if pure functions exist)

| ID | Test | Asserts |
|----|------|---------|
| U-CLI-01 | `setup_providers_include_omlx` | list contains |
| U-CLI-02 | `default_model_omlx` | `omlx/…` |

---

## 3. Integration tests (offline, HTTP mock)

Use `wiremock` or existing mock HTTP patterns in edgequake-llm:

| ID | Scenario | Mock |
|----|----------|------|
| I-01 | list_models | GET /v1/models → 200 list |
| I-02 | chat | POST /v1/chat/completions → assistant text |
| I-03 | chat tools | response with tool_calls → second round |
| I-04 | 401 | assert error mapping |
| I-05 | connection refused | no mock server → NetworkError |
| I-06 | EdgeCrab discovery adapter | mock returns ids → cache write |

---

## 4. Live e2e (opt-in)

### 4.1 Prerequisites

```bash
# Apple Silicon Mac
omlx start   # or menu bar server
curl -s http://127.0.0.1:8000/v1/models | head
export OMLX_E2E=1
export OMLX_HOST=http://127.0.0.1:8000
# optional: OMLX_MODEL=<id from /v1/models>
```

### 4.2 edgequake-llm live suite

**File:** `tests/e2e_omlx_openai_compatible.rs`

| ID | Case | Pass criteria |
|----|------|---------------|
| L-EQ-01 | health / list_models | ≥1 model or skip if none |
| L-EQ-02 | chat | non-empty assistant content |
| L-EQ-03 | stream | ≥1 chunk |
| L-EQ-04 | tools | if model supports; else skip with message |
| L-EQ-05 | embeddings | if embedding model present; else skip |

Skip pattern:

```rust
if std::env::var("OMLX_E2E").ok().as_deref() != Some("1") {
    eprintln!("skip: set OMLX_E2E=1");
    return;
}
```

Or `#[ignore = "requires OMLX_E2E=1 and running oMLX"]`.

### 4.3 EdgeCrab live / dogfood script

Not necessarily in `cargo test` — a **documented dogfood checklist**:

| ID | Step | Pass |
|----|------|------|
| L-EC-01 | `edgecrab doctor` shows oMLX up | ✓ |
| L-EC-02 | `edgecrab setup` select omlx | config written |
| L-EC-03 | chat “reply pong” | pong |
| L-EC-04 | “list files in cwd with tools” | tool call + result |
| L-EC-05 | `/models omlx` | live ids |
| L-EC-06 | `/cost` | $0 |
| L-EC-07 | interrupt mid-generation | clean stop, no dual hang |
| L-EC-08 | `/model` switch ollama→omlx→back | works |
| L-EC-09 | multi-tool: read + search + write temp | completes |
| L-EC-10 | long context paste (20k+) | progresses or prunes; no silent death |

Optional automated:

```rust
// crates/edgecrab-core/tests/e2e_omlx_react.rs
// #[ignore] full AgentBuilder + mock tools or real tool registry with temp dir
```

---

## 5. Regression guarantees (must not break)

| Existing suite | Note |
|----------------|------|
| lmstudio / ollama local policy tests | still pass after DRY refactor |
| discovery tests for other providers | registration order stable |
| pricing zero-cost for copilot | unchanged |
| model_catalog resolve lmstudio nested | unchanged |

Run:

```bash
# edgequake-llm
cargo test --lib
cargo test --test e2e_provider_factory

# edgecrab
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

---

## 6. Non-flaky rules

| Rule | Application |
|------|-------------|
| No wall-clock sleeps for correctness | use mock instant responses |
| No dependency on specific model weights in CI | live tests skip |
| Deterministic fixtures for JSON parse | commit sample payloads |
| No write to `~/.edgecrab` in unit tests | TempDir + EDGECRAB_HOST |
| No network in default tests | enforced by not starting server |

---

## 7. Coverage targets

| Area | Target |
|------|--------|
| Provider identity / factory | 100% of new match arms |
| Local policy membership | 100% |
| Discovery parse | happy + empty + malformed |
| Live tools | best-effort on dogfood model |

---

## 8. CI matrix suggestion

| Job | oMLX |
|-----|------|
| Linux PR CI | offline only |
| macOS optional nightly | if self-hosted runner has oMLX — run ignored e2e |
| Release verify | dogfood checklist signed off |

---

## 9. Traceability

| Requirement | Tests |
|-------------|-------|
| PO-M1 selectable | U-EC-05, U-CLI-01, L-EC-02 |
| PO-M2 live list | I-01, L-EQ-01, L-EC-05 |
| PO-M3 tools | L-EQ-04, L-EC-04, L-EC-09 |
| PO-M4 zero cost | U-EC-09, L-EC-06 |
| PO-M5 doctor | L-EC-01 |
| AE-T1 non-stream tools | U-EC-03 |
| AE-T2 no dual request | U-EC-02 |
| AE-T3 profile ids | U-EC-06, U-EQ-05 |
