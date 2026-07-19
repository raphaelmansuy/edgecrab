# 003 — AI Engineer / Harness Lens (Re-assessed)

**Authority:** [000-code-is-law.md](000-code-is-law.md) · AE1–AE6, AE8–AE9  
**Date:** 2026-07-19  
**Owners:** EC `edgecrab-core` · Hermes `agent/*`, `tools/tool_result_storage.py`, `tools/tool_executor` via agent

---

## 1. Harness is the product core

Models are commodities. Harness quality answers:

1. Does the agent stop for the right reason?  
2. Does it recover from 429 / billing / partial streams?  
3. Does it avoid tool thrash and false “done”?  
4. Can an operator explain *why* a turn ended?

---

## 2. Loop physics (law)

### EdgeCrab `execute_loop` (conversation.rs:364)

```text
TurnPrologueState::begin
while iterations && budget:
  compress?  (compression.rs + ContextEngine)
  provider.chat  (provider_call.rs + failover classify)
  if tool_calls:
    pre_dispatch_decision  (turn_dispatch_policy)
    JoinSet parallel | sequential dispatch
    spill / summary
    steers · goals · shadow judge · document latch
  else:
    assess_completion → RunOutcome
    turn_epilogue (verify_on_stop debt)
learning_reflection bg (≥5 tools)
```

### Hermes `run_conversation`

```text
build_turn_context  (turn_context.py — full prologue)
while budget:
  API + error_classifier recovery paths
  execute_tool_calls_{concurrent|sequential|segmented}
  guardrails / continuation prompts
finalize_turn  (string exit reason, hooks, trajectory)
optional background_review thread
```

---

## 3. Dimension scores (code-backed)

### 3.1 Budgets & stop (AE1)

| | EC | H | Score |
|-|----|---|-------|
| Iteration cap | yes | yes | = |
| Hard-stop default | **ON** | **OFF** | **EC** |
| Cancel | interrupt latch | interrupt + drain | = |
| Cost | model_cost_guard | credits/usage modules | H depth |

### 3.2 Pre-dispatch mediation (AE4/AE7)

EC `pre_dispatch_decision` order (law):

1. visual storm  
2. loopback port shopping  
3. repeated browser nav  
4. verification theater  
5. theater write / document GUI thrash  
6. **spill-blind write block**  
7. tool-loop guardrail  

Hermes focuses guardrails on **failure signature loops** (exact failure, same-tool failure, no-progress).

**Score: EC** for anti-theater / spill-blind; **H** for mature failure-loop taxonomy naming.

### 3.3 Parallel dispatch (AE4 latency)

| | EC | H |
|-|----|---|
| Mechanism | `JoinSet` + `can_parallelize_in_batch` | ThreadPool concurrent + segmented |
| Module isolation | still inside conversation.rs process_response | **tool_executor.py** dedicated |

**Score: = capability · H modularity.**

### 3.4 Spill / turn budget (AE4)

| | EC | H |
|-|----|---|
| Module | `artifact_spill.rs` 913 LOC | `tool_result_storage.py` 254 LOC |
| `enforce_turn_budget` | yes | yes |
| Spill unread → block mutation | **yes** (pre-dispatch) | no equivalent hard block |

**Score: EC.**

### 3.5 Error classify → recover (AE6)

| | EC | H |
|-|----|---|
| Variants | 21 | 23 (`upstream_rate_limit`, `ssl_cert_verification` extra) |
| LOC | 854 | 1621 |
| Credential pool | flag only | **2459** LOC pool |
| In-loop billing UX | thinner | dense |

**Score: = enum · H operational depth.**

### 3.6 Compression (AE5/AE2)

Both: LLM + structural; protect last-N; todo snapshot (EC `todo_snapshot_user_message`); defer preflight helpers (EC `should_defer_preflight_to_real_usage`).

Hermes: larger compressor (3486 LOC), compression lock lease, image shrink recovery, Codex app-server path.

**Score: = core · H heuristic volume.**

### 3.7 Completion truth (AE3)

| | EC | H |
|-|----|---|
| Types | `RunOutcome`, `CompletionDecision`, `ExitReason` | string `_turn_exit_reason` |
| verify_on_stop | default true + epilogue debt | auto/nudge |
| Shadow judge | yes | no |
| Document done latch | yes | no specialty |

**Score: EC.**

### 3.8 Cross-window progress (AE2)

| | EC | H |
|-|----|---|
| Goals outside history | SQLite + inject block | goals |
| parent_session_id | **yes** | yes + UI walk |
| Learning reflection | bg after tools | background_review + curator + learning_graph |

**Score: ≠ complementary.**

### 3.9 Human mid-flight (AE8)

Typed steering bus EC; Hermes interrupt + app UX. **= / EC steer types.**

---

## 4. Shared SOTA gaps (both−)

1. Full **browser-as-user** VERIFY for web product tasks (Anthropic 2025).  
2. First-class **initializer agent** session (feature_list.json + init.sh + progress.txt pattern).  
3. Partial stream tool-arg recovery still provider-fragile.  
4. Multi-agent completion contracts across workers still hard.

---

## 5. Borrow / keep (harness only)

| Priority | Item | Hermes anchor | EC owner | Note |
|----------|------|---------------|----------|------|
| P0 | Real prologue extract | `turn_context.py` | `turn_prologue.rs` | structure |
| P0 | +2 FailoverReason + pools wiring | classifier + credential_pool | `failover.rs` + router | not greenfield |
| P0 | Harden external VERIFY evidence | verification_stop + browser tools | epilogue + contract_verify | SOTA AE3 |
| P1 | Compression lock / image shrink | conversation_compression | compression.rs | |
| P1 | Extract process_response → module | tool_executor shape | new `tool_batch.rs` | DRY |
| P2 | Curator depth optional | curator.py | thin curator | |
| KEEP | hard-stop ON | — | harness | |
| KEEP | spill-blind write block | — | turn_dispatch_policy | |
| KEEP | RunOutcome / shadow judge | — | types + core | |
| KEEP | document done latch | — | task_class | |
| REJECT | hard-stop default OFF | tool_guardrails | — | |

---

## 6. Scorecard

| Dimension | Score |
|-----------|-------|
| Loop modularity | **H** |
| Pre-dispatch intelligence | **EC** |
| Parallel tools capability | = |
| Spill stack | **EC** |
| Error taxonomy presence | = |
| Error ops depth | **H** |
| Typed completion | **EC** |
| verify_on_stop default | **EC** |
| Cross-window goals | **EC** slight |
| Personal curation | **H** |
| Offline forensics | **EC** |
| Coding-agent local models | **EC** |
| Multi-provider SaaS chaos | **H** |
