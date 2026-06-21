# 004 — Hermes Comparator

**Hermes root:** `/Users/raphaelmansuy/Github/03-working/hermes-agent`  
**Cross-ref:** [015/011](../015-improve-harness-and-agent/011-hermes-parity-map.md) · [005-code-audit](./005-code-audit.md)

---

## Architecture shape

```text
  HERMES (Python)                         EDGECRAB (Rust)
  ─────────────────────────────────────   ─────────────────────────────────────
  run_agent.py (facade)                   Agent + execute_loop
  agent/conversation_loop.py (~4k)        conversation.rs (~7.6k)  ← monolith debt
  agent/turn_context.py (prologue)        (inline in execute_loop)
  agent/turn_finalizer.py (epilogue)      turn_completion.rs (UX only)
  agent/tool_executor.py                  turn_dispatch.rs + conversation dispatch
  agent/error_classifier.py               provider_call.rs (partial)
  agent/context_compressor.py             compression.rs (parity)
  agent/tool_guardrails.py                tool_loop_guardrails.rs (ported)
  tools/tool_search.py + BM25             tool_search.rs + tool_search_bm25.rs
```

**Hermes wins:** modular loop decomposition, battle-tested error taxonomy, turn finalizer honesty.  
**EdgeCrab wins:** typed `CompletionPolicy`, `HarnessSnapshot`, `RunOutcome`, `harness_analyzer` concept, Rust security defaults.

---

## Mechanism comparison matrix

| Capability | Hermes | EdgeCrab | Verdict |
|------------|--------|----------|---------|
| **Localhost preview** | `allow_private_urls` (broad) | `security.preview` port allowlist | EdgeCrab **safer**; Hermes **more usable** in dev |
| **Profile config** | Single `~/.hermes/config.yaml` | Profile-isolated YAML | Hermes **simpler**; caused E15 |
| **Turn budget** | `enforce_turn_budget()` 200k/turn | `finalize_tool_turn` + `result_turn_budget_chars` | **Parity shipped** |
| **Spill stack** | 3-layer (`tool_result_storage`) | `artifact_spill` + turn budget | EdgeCrab **more transparent** ops |
| **Tool loop guardrails** | warn/block/halt configurable | same; **`hard_stop_enabled: false`** | Hermes **more likely armed** in prod |
| **Completion truth** | `_turn_exit_reason` + explainer | `CompletionPolicy` + gates | EdgeCrab **stronger types**; Hermes **simpler path** |
| **File mutation verify** | post-turn footer in finalizer | `harness_gates` mutation debt | **Parity intent** |
| **Compression lineage** | `parent_session_id` on compress | not ported | **Borrow** |
| **Error classifier** | single `FailoverReason` brain | scattered in `provider_call` | **Borrow** |
| **Background review** | forked agent post-turn | none | **Borrow** (optional) |
| **Plugin hooks** | `pre_llm_call`, `post_tool_call` | minimal | Hermes **more extensible** |
| **Harness doctor** | none in core | `doctor harness` + analyzer | EdgeCrab **ahead** (when parser works) |
| **Replay CI** | mock patch points on `run_agent` | `harness_games003_replay.rs` | EdgeCrab **ahead** |

---

## Five Hermes patterns to borrow (high ROI)

### 1. Turn prologue / epilogue split

Hermes `build_turn_context()` resets counters, runs preflight compression, drains steers **before** API.  
`finalize_turn()` emits budget summary, file-mutation verifier, diagnostics when last message is `role=tool`.

**EdgeCrab gap:** epilogue logic split between `conversation.rs` end and thin `turn_completion.rs`. Risk: mid-loop and end-loop **diverge**.

**Target:** extract `turn_prologue.rs` / `turn_epilogue.rs` (extend P2.1).

### 2. Unified error classifier

Hermes `agent/error_classifier.py` maps API failures → `{retry, compress, rotate, abort}`.

**EdgeCrab gap:** `provider_call.rs` has retries but no single `FailoverReason` enum consumed by loop + doctor.

### 3. Compression session lineage

Hermes rotates session on compress with `parent_session_id`; compression children hidden from picker.

**EdgeCrab gap:** compression reshapes messages only; operator loses turn attribution.

### 4. Continuation prompts by failure class

Hermes `_get_continuation_prompt()` branches: stream stall vs length cap vs partial tool JSON.

**EdgeCrab gap:** P1.6 partial; `mutation_turn_policy` covers writes only.

### 5. Turn finalizer honesty

Hermes warns when turn ends with **unanswered tool_calls** or pending tool results.

**EdgeCrab gap:** `count_unanswered_tool_calls` exists in `turn_completion.rs` but not surfaced as hard gate.

---

## Explicit non-borrows

| Hermes pattern | Why reject |
|----------------|------------|
| Global `allow_private_urls` | SSRF regression; keep port allowlist |
| Auto-inject full spill into context | Turn budget explosion |
| Profile fully isolated security | Caused games003; merge instead |
| Python-style dynamic tool registration | `inventory::submit!` is fine |

---

## Hermes file index (for implementers)

| Topic | Path |
|-------|------|
| Main loop | `agent/conversation_loop.py` |
| Turn setup | `agent/turn_context.py` |
| Turn finalize | `agent/turn_finalizer.py` |
| Tool dispatch | `agent/tool_executor.py` |
| Guardrails | `agent/tool_guardrails.py` |
| Compression | `agent/context_compressor.py`, `agent/conversation_compression.py` |
| Error recovery | `agent/error_classifier.py` |
| Tool search | `tools/tool_search.py` |
| Turn budget | `tools/budget_config.py` + `enforce_turn_budget` |
| Session DB | `hermes_state.py` |
| Preview lifecycle | `tui_gateway/server.py` (`preview.restart`) |

---

## Parity score (honest)

```text
  Area                    Hermes   EdgeCrab   Leader
  ──────────────────────  ───────  ────────   ──────
  Loop modularity         ████░    ██░░░      Hermes
  Security defaults       ███░░    █████      EdgeCrab
  Dev preview UX          ████░    ██░░░      Hermes
  Completion types        ███░░    ████░      EdgeCrab
  VERIFY enforcement      ██░░░    ██░░░      Tie (both weak)
  Observability           ███░░    ███░░      Tie
  Provider breadth        █████    ████░      Hermes
```

Net: **complementary**. EdgeCrab should not cosplay Hermes file layout — port **mechanisms** into existing Rust modules per [007-backlog](./007-priority-backlog.md).
