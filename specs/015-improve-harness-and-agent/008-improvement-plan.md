# 008 — Improvement Plan (DRY · SOLID)

Phased delivery with **one owner module per change**. Each phase ends with acceptance gates in [009-acceptance-criteria.md](./009-acceptance-criteria.md).

**Battle-test anchor:** [005-evidence-games003.md](./005-evidence-games003.md) · [010-battle-test-matrix.md](./010-battle-test-matrix.md).

---

## Priority overview

```text
  P0  Operator truth + perception loop     (games003 class failures)
  P1  Completion contract + observability   (ADR-001 + OTEL noise)
  P2  Loop modularization + discovery       (maintainability)
  P3  Strict verification mode              (optional product tier)
```

---

## P0 — Perception & operator truth (2–3 weeks)

### P0.1 Actionable spill stubs

| | |
|---|---|
| **Problem** | Model gets useless stub; TUI shows `spilled` / `read ?` |
| **Change** | Extend `artifact_spill::build_stub()` with `source_path`, `next_read`, budget hint |
| **Owner** | `crates/edgecrab-tools/src/artifact_spill.rs` |
| **DRY** | Same struct feeds `tool_result_summary.rs` + TUI |
| **SOLID** | Spill formatting = single function (SRP) |
| **Gate** | HA-01, HA-02 |

### P0.2 Tool-call args cache for summaries

| | |
|---|---|
| **Problem** | Pruned history loses path → `read ?` |
| **Change** | Shelf/transcript stores `(tool_call_id → args_json)` until turn ends |
| **Owner** | `crates/edgecrab-cli/src/turn_activity.rs` + `tool_display.rs` |
| **Gate** | HA-03 |

### P0.3 Budget rejection → patch steering

| | |
|---|---|
| **Problem** | Silent reject then retry loops |
| **Change** | `tool_argument_budget_exceeded` adds `recommended_tools: ["patch"]`, `max_bytes`, `split_strategy` |
| **Owner** | `recovery_catalog.rs` + `mutation_turn_policy.rs` |
| **Gate** | HA-04 |

### P0.4 Dev preview profile

| | |
|---|---|
| **Problem** | Visual tasks cannot verify (SSRF blocks localhost) |
| **Change** | `security.preview` config; SSRF allowlist OR `capture_preview` → vision |
| **Owner** | `edgecrab-security` + `tools/browser.rs` or new `tools/preview.rs` |
| **Gate** | HA-05, HA-06 |

### P0.5 Wire/deferred operator display

| | |
|---|---|
| **Problem** | "107 tools" vs 55 on wire confuses operators |
| **Change** | Status bar: `wire:N def:M` from `wire_partition_counts` |
| **Owner** | `edgecrab-cli` status bar + `doctor` |
| **Gate** | HA-07 |

---

## P1 — Completion contract & observability (2 weeks)

### P1.1 RunOutcome everywhere

| | |
|---|---|
| **Problem** | `completed` bool ambiguous |
| **Change** | TUI final line + gateway hook pass `exit_reason` + user_message |
| **Owner** | `completion_assessor.rs`, `app.rs`, `event_processor.rs` |
| **ADR** | Implements [agent_harness/001](../agent_harness/001_adr_unified_agent_harness.md) |
| **Gate** | HA-10..HA-12 |

### P1.2 TaskClassifier (advisory)

| | |
|---|---|
| **Problem** | Vague prompts get wrong verification |
| **Change** | Heuristic classifier: keywords + file targets → `TaskClass` |
| **Owner** | `edgecrab-core/src/task_class.rs` (new) |
| **Inject** | One user-message footer: "Task class: visual_ux — preview recommended" |
| **Gate** | HA-13 |

### P1.3 OTEL fail-soft

| | |
|---|---|
| **Problem** | tcp connect ERROR spam |
| **Change** | Startup probe OR circuit breaker on export; downgrade log level |
| **Owner** | `otel_export.rs`, `observability.rs` |
| **Gate** | HA-14 |

### P1.4 Copilot streaming recovery

| | |
|---|---|
| **Problem** | 30s+ non-streaming tool turns |
| **Change** | Retry streaming once per session before permanent downgrade; shelf shows provider hint |
| **Owner** | `conversation.rs` provider_call + `local_provider_policy` (extend for copilot) |
| **Gate** | HA-15 |

---

## P2 — Maintainability & discovery (3 weeks)

### P2.1 conversation.rs extraction

| | |
|---|---|
| **Change** | Move tool turn + provider call per [007-architecture-target.md](./007-architecture-target.md) |
| **Rule** | Move-only PR first; no logic changes |
| **Gate** | HA-20 (all existing tests pass) |

### P2.2 ProgressSink trait

| | |
|---|---|
| **Change** | Introduce trait; `StreamEventSink` adapter |
| **Gate** | HA-21 |

### P2.3 Discovery hints for search_files

| | |
|---|---|
| **Problem** | `games003` 0 hits |
| **Change** | On 0 results: suggest `terminal find` or include workspace roots in error |
| **Owner** | `tools/file_search.rs` |
| **Gate** | HA-22 |

### P2.4 Spill-aware read_file default

| | |
|---|---|
| **Change** | When read > threshold, auto-offer first chunk inline (lines 1–80) + artifact path |
| **Owner** | `tools/file_read.rs` + spill policy |
| **Gate** | HA-23 |

---

## P3 — Strict verification (optional)

| | |
|---|---|
| **Change** | `harness.verification.strict: true` blocks `Completed` without evidence |
| **Owner** | `completion_assessor.rs` + `VerificationPolicy` |
| **Gate** | HA-30 |

---

## Dependency graph

```text
  P0.1 spill stub ──┬──► P0.4 preview (model can read artifact paths)
  P0.2 args cache   │
  P0.3 budget hint  │
  P0.5 wire display │
                    │
  P1.1 RunOutcome ──┴──► P1.2 TaskClassifier ──► P3 strict verify
  P1.3 OTEL
  P1.4 streaming

  P2.* parallel after P0 stable
```

---

## What we explicitly defer

| Idea | Defer reason |
|------|--------------|
| New "done" tool | CompletionPolicy sufficient (ADR-001) |
| Disable SSRF globally | Security regression |
| Raise 27k budget for cloud | Breaks local geometry law |
| Auto-run browser for all tasks | Cost + safety; task-class only |

---

## Success metrics (90 days post-P1)

| Metric | Baseline (games003) | Target |
|--------|---------------------|--------|
| Spill stub → artifact read within 2 turns | ~0% | >80% in dogfood |
| write_file budget rejections per visual task | 2+ | 0 (patch used) |
| Operator "stuck" reports on Copilot tool turns | frequent | <10% sessions |
| OTEL ERROR lines/min (no collector) | ~12 | 0 |
| RunOutcome shown on interrupt | no | yes |
