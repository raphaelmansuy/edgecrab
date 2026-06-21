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

### P0 extension (session `2e720f47`)

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-16** | `browser_navigate` SSRF/scheme error includes preview config hint + `http://127.0.0.1:PORT` recipe | recovery_catalog / integration |
| **HA-17** | `memory_write` over cap returns `used_chars`, `max_chars`, prune guidance | `memory.rs` unit test |
| **HA-18** | Heredoc terminal rejection recommends `write_file` in tool result JSON | terminal + recovery test |

### P0 extension (session `0aeef965` / assessment 012)

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-41** | Active profile with `security.preview` unset inherits global preview OR doctor warns | profile load test |
| **HA-42** | Browser block recovery never suggests `read_file` on `~/.edgecrab/config.yaml`; offers `/config` path | recovery_catalog test |
| **HA-43** | VisualUx session with ≥3 markdown report files and 0 perception → completion not `Completed` (strict) | completion_assessor test |

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

### P1 extension (Hermes-inspired)

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-19** | After compress, pending todos re-injected as synthetic user message | compression unit test |
| **HA-20b** | Stream interrupted → continuation message matches failure class | conversation test |
| **HA-20c** | `run_process` http.server result mentions bound port | process/terminal test |
| **HA-20d** | `VisualUx` advisory mentions preview URL or vision | `task_class` test |
| **HA-20e** | ≥5 terminal tools in 60s without perception → harness WARN | log assertion |

---

## P2 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-20** | `cargo test -p edgecrab-core` green after loop extraction | CI |
| **HA-21** | `ProgressSink` trait; `StreamEventSink` emits same events as before | unit test |
| **HA-22** | `search_files` 0-hit on existing dirname suggests find | tool unit test |
| **HA-23** | Large read returns inline preview lines + spill metadata | file_read e2e |
| **HA-24** | Turn char budget enforced; `read_file` exempt from per-call spill threshold pin | artifact_spill test |
| **HA-25** | `edgecrab doctor harness` reports spill-without-read count from JSONL | doctor test |
| **HA-26** | Background http.server emits ready notice on shelf | process e2e |
| **HA-27** | `harness_games003_replay` passes with preview enabled | replay test |

---

## P3 gates

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-30** | Strict mode: visual task without preview evidence → `VerificationMissing` | completion_assessor test |

### P0 extension (spec 017 — exceed Hermes)

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-44** | Doctor `tool_starts` equals JSONL harness events | `harness_analyzer::ha44_parses_tracing_jsonl` |
| **HA-45** | `LoopAction::Done` never assesses with empty snapshot | `turn_epilogue::ha45_assess_uses_built_snapshot_not_default` |
| **HA-46** | Guardrail halt → `ExitReason::GuardrailHalt`, loop stops | `turn_dispatch::ha46_halt_sets_guardrail_halt_flag` |
| **HA-47** | `guardrails_hard_stop` default true | `harness_loop_policy` unit test |
| **HA-48** | VisualUx + act storm → `blocks_completion()` | `turn_epilogue::ha48_visual_storm_blocks_in_snapshot` |
| **HA-49** | E16 fixture: 0 perception → not `Completed` | `harness_games003_replay::e16_no_false_completed_on_visual_without_perception` |

### P1 extension (spec 017 — structure + error brain)

| ID | Criterion | Test / check |
|----|-----------|--------------|
| **HA-50** | Failover classifier ≥40 Hermes scenarios | `failover` unit + `harness_failover_matrix` |
| **HA-51** | Unanswered tool_calls → `Incomplete` | `completion_assessor::ha51_unanswered_tool_calls_incomplete` |
| **HA-52** | `conversation.rs` LOC ≤ 6000 after extract | CI `wc -l` gate (post Phase 1) |
| **HA-55** | Turn prologue initializes trackers | `turn_prologue::ha55_prologue_initializes_trackers` |
| **HA-11** | Budget exhausted fallback mentions limit | `turn_epilogue::ha11_budget_fallback_message_mentions_limit` |

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
