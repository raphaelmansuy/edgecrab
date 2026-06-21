# 007 — Priority Backlog

**Date:** 2026-06-19  
**Supersedes ranks in:** [015/013](../015-improve-harness-and-agent/013-impact-ranked-backlog.md) where noted  
**Cross-ref:** [006-verdict](./006-brutal-verdict.md) · [015/009](../015-improve-harness-and-agent/009-acceptance-criteria.md)

---

## Priority stack

```text
  IMPACT ▲
         │  P0-1  JSONL harness analyzer (doctor truth)
         │  P0-2  Prove preview merge in ops + persist migrate
         │  P0-3  Wire guardrail halt + mid-loop harness snapshot
         │  P1-1  Terminal block until perception (VisualUx)
         │  P1-2  False-completed regression CI (E16)
         │  P1-3  Hermes error_classifier port
         │  P2-1  conversation.rs extraction complete
         │  P2-2  Compression session lineage
         └────────────────────────────────────────────► EFFORT
```

---

## Ranked items

| Rank | Item | Problem | Change | Owner | Gate | Evidence |
|------|------|---------|--------|-------|------|----------|
| **1** | **Fix harness JSONL analyzer** | Doctor reports 0 tools on 973-line log | Parse JSON `fields.message` / `fields.tool_name` in `analyze_harness_log` | `harness_analyzer.rs`, `doctor.rs` | **HA-44** | C3, E18 |
| **2** | **Ops-proof preview inheritance** | Profile YAML lacks preview; doctor warns | Debug why `load_from_with_global_inheritance` + doctor disagree; run `migrate_profile_preview_from_global` on **every** `load_runtime`; assert in doctor | `config.rs`, `runtime.rs`, `doctor.rs` | HA-41+ | R2, E15 |
| **3** | **Mid-loop harness snapshot** | `HarnessSnapshot::default()` at provisional complete | Build lightweight snapshot or defer text-exit until gates pass | `conversation.rs` | **HA-45** | C1, R1 |
| **4** | **Wire guardrail halt** | `halt_decision` never consumed | On `Halt`, break ReAct loop with `RunOutcome::Incomplete` | `turn_dispatch.rs`, `conversation.rs` | HA-46 | C2, R4 |
| **5** | **Arm hard_stop for VisualUx** | `hard_stop_enabled: false` | Default on when `TaskClass::VisualUx` or config `harness.guardrails_hard_stop: true` | `tool_loop_guardrails.rs`, `config.rs` | HA-47 | R4, E9 |
| **6** | **Terminal block until perception** | 158 terminal vs 0 visual verify | After SSRF block: block `terminal` until `browser_*` success or operator steer | `harness_advisory.rs` | HA-48 | E9, `0aeef965` |
| **7** | **E16 replay CI** | `completed` without perception | Extend `harness_games003_replay.rs` with `e22c0a28` message history → expect `NeedsVerification` | `tests/harness_games003_replay.rs` | HA-49 | E16 |
| **8** | **Theater write cap** | 5+ verify markdowns per task | `completion_assessor` + write_file policy: block `*VERIFY*`, `*REPORT*` after 1 | `completion_assessor.rs`, `task_class.rs` | HA-43+ | R5, E17 |
| **9** | **Spill forced read** | 0% artifact follow-up | Within 2 turns after spill: advisory → **block write** until `read_file` on artifact | `artifact_spill.rs`, `turn_dispatch.rs` | HA-23 | E2 |
| **10** | **Hermes error_classifier** | Scattered retry logic | `FailoverReason` enum + single classifier module | new `error_classifier.rs` | HA-50 | 004 § borrow #2 |
| **11** | **Turn epilogue module** | Finalize logic scattered | Extract Hermes-style `turn_epilogue.rs`: unanswered tools, mutation footer | `turn_completion.rs` | HA-51 | 004 § borrow #1 |
| **12** | **Monolith extraction** | 7.6k line `conversation.rs` | Move `process_response`, mid-loop assess to owners | P2.1 | HA-52 | R6 |
| **13** | **Compression lineage** | No `parent_session_id` on compress | Port Hermes session rotation | `compression.rs`, `edgecrab-state` | HA-53 | 004 § borrow #3 |
| **14** | **Continuation prompts** | Generic stream recovery | P1.6 Hermes branches in `provider_call.rs` | `conversation.rs` | HA-20b | E6 |
| **15** | **Port discovery** | 8888 vs 8000 | P1.7 `dev_server.rs` + `lsof` hint in tool result | `dev_server.rs` | HA-20c | E11 |

---

## 30-day success metrics

| Metric | 016 baseline | 30-day target |
|--------|--------------|---------------|
| Doctor `tool_starts` vs JSONL | 0 / 322 | **equal** |
| Homelab doctor preview | disabled | **enabled** (ports listed) |
| CLI visual preview success | 0/6 | **≥2/3** next sessions |
| False `completed` (visual, no perception) | 2 | **0** (CI + runtime) |
| Terminal share (visual tasks) | 49% | **<30%** |
| Markdown verify files / task | 3–6 | **≤1** |

---

## Dependency sketch

```text
  Rank 1 (JSONL doctor) ──► enables honest regression triage
  Rank 2 (preview ops) ──► unlocks browser success
  Rank 3–6 (loop physics) ──► parallel once preview fixed
  Rank 7 (E16 CI) ──► locks false-completed class
  Rank 10–13 (Hermes ports) ──► P2 after P0 stable
```

---

## Explicitly NOT on backlog

| Idea | Reason |
|------|--------|
| Disable SSRF / global private URLs | Security regression |
| Auto-inject full spill | Turn budget explosion |
| "Just use Opus" | Harness must work on configured default (Haiku) |
| New `done` tool | `CompletionPolicy` sufficient when armed |

---

## Immediate operator unblock

Until ranks 1–2 ship, add to **`~/.edgecrab/profiles/homelab/config.yaml`**:

```yaml
security:
  preview:
    enabled: true
    allow_localhost_ports: [8000, 8888, 5173, 3000, 8080]
```

Restart EdgeCrab. Verify: `edgecrab doctor harness` → preview enabled + non-zero tool_starts (after HA-44).

Cross-ref: [015/012 § operator unblock](../015-improve-harness-and-agent/012-brutal-assessment-jun2026.md)
