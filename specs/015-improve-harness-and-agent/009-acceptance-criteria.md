# 009 — Acceptance Criteria (HA-01..HA-40)

Automated where possible; manual dogfood for UX gates.

---

## P0 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-01** | Spill stub includes `source_path` when known | `artifact_spill` unit test |
| **HA-02** | Spill stub includes `next` read_file recipe with artifact path + limit | unit test |
| **HA-03** | TUI tool row never shows `read ?` when `tool_call_id` had valid `path` in args | CLI integration test |
| **HA-04** | Budget exceeded error JSON includes `recommended_tools` containing `patch` | `mutation_turn_policy` test |
| **HA-05** | With `security.preview.enabled: true`, `http://127.0.0.1:PORT` allowed for allowlisted PORT | security e2e |
| **HA-06** | With preview disabled, localhost still blocked (no regression) | security e2e |
| **HA-07** | Status bar or `/doctor` shows `wire:N deferred:M` when indexed mode | CLI test |

---

## P1 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-10** | Interrupt shows `ExitReason::Interrupted` in TUI final banner | manual + snapshot test |
| **HA-11** | Budget exhausted shows `ExitReason::BudgetExhausted`, not "completed" | `completion_assessor` test |
| **HA-12** | Gateway `agent:done` payload includes `exit_reason` field | gateway test |
| **HA-13** | Message containing "beautiful" + path `demo/` → `TaskClass::VisualUx` advisory | `task_class` unit test |
| **HA-14** | No OTEL export ERROR >1 per 60s when collector down | observability e2e |
| **HA-15** | Copilot session logs streaming attempt before downgrade | provider_tracing e2e |

---

## P2 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-20** | `cargo test -p edgecrab-core` green after loop extraction | CI |
| **HA-21** | `ProgressSink` trait; `StreamEventSink` emits same events as before | unit test |
| **HA-22** | `search_files` 0-hit on existing dirname suggests find | tool unit test |
| **HA-23** | Large read returns inline preview lines + spill metadata | file_read e2e |

---

## P3 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-30** | Strict mode: visual task without preview evidence → `VerificationMissing` | completion_assessor test |

---

## Regression pack (games003 replay)

Script: `crates/edgecrab-core/tests/harness_games003_replay.rs` (to add)

| Step | Assert |
|------|--------|
| Mock provider returns tool calls from session log | loop progresses |
| Spill triggered on 30k read | stub passes HA-01/02 |
| Oversized write rejected | error passes HA-04 |
| Preview enabled | localhost navigate succeeds HA-05 |

---

## CI wiring (recommended)

```yaml
# .github/workflows/ci.yml addition (future)
- cargo test -p edgecrab-core --test harness_games003_replay
- cargo test -p edgecrab-tools artifact_spill -- --exact spill_stub_actionable
```

Cross-ref: [014 007-acceptance LH-01..LH-64](../014-improve-local-harness/007-acceptance-criteria.md) — orthogonal (local geometry).
