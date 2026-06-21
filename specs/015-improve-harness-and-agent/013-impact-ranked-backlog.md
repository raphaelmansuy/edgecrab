# 013 — Impact-Ranked Backlog (Jun 2026)

Single priority stack for harness work. Each row links plan item, acceptance gate, and evidence ID.

**Cross-ref:** [008-improvement-plan.md](./008-improvement-plan.md) · [009-acceptance-criteria.md](./009-acceptance-criteria.md) · [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md)

---

## Priority stack

```text
  IMPACT ▲
         │  P0-A  Profile preview parity + migrate
         │  P0-B  VisualUx auto-enable preview (runtime)
         │  P0-C  Config dead-end recovery (no read ~/.edgecrab)
         │  P1-A  Strict verify default for VisualUx
         │  P1-B  Dev port discovery (8000 in defaults)
         │  P1-C  Turn budget (Hermes enforce_turn_budget)
         │  P2-A  conversation.rs extraction complete
         │  P2-B  games003 + race_gamey replay CI
         └────────────────────────────────────────────► EFFORT
```

---

## Ranked items

| Rank | Item | Problem | Change | Owner | Gate | Evidence |
|------|------|---------|--------|-------|------|----------|
| **1** | **Profile config merge / migrate preview** | Global preview ON; homelab profile OFF | On profile load: merge `security.preview` from global if unset; OR `edgecrab profile migrate --apply-bundled-keys`; doctor warns at TUI start | `profile.rs`, `config.rs`, `doctor.rs` | **HA-41** | R1, E4, `0aeef965` |
| **2** | **VisualUx runtime preview enable** | Operator must hand-edit YAML | When `TaskClass::VisualUx` and preview disabled: inject one-shot **operator** shelf notice + optional env `EDGECRAB_PREVIEW=1` for session | `task_class.rs`, `conversation.rs` | HA-20d+ | R1, all games003 |
| **3** | **Config recovery without file read** | Model tries `read_file ~/.edgecrab/config.yaml` → jail | Recovery JSON: `fix_via: "/config preview on"` slash command or `edgecrab config set security.preview.enabled true`; **never** suggest reading home config from agent | `recovery_catalog.rs`, `commands.rs` | **HA-42** | R2, `0aeef965` |
| **4** | **Mandatory verify before Complete (VisualUx)** | Markdown theater passes as done | Default `harness.verification_strict` for VisualUx; block `Completed` without `browser_snapshot`/`vision_*` success | `completion_assessor.rs` | HA-30 | R3, E8 |
| **5** | **Default ports include 8000** | http.server blocked on default port | Add `8000` to `PreviewConfig::default().allow_localhost_ports` | `config.rs` | HA-05 | R4, E11 |
| **6** | **Dev server port in tool result** | Agent navigates wrong port | Ship P1.7: `dev_server.rs` + `process_table` listening ports in result | `dev_server.rs` | HA-20c | E11 |
| **7** | **Turn budget aggregate** | Turn blow-up / spill loops | Ship P2.5 Hermes `enforce_turn_budget` | `artifact_spill.rs` | HA-24 | spill sessions |
| **8** | **Todo + verify coupling** | 12 todos, zero preview task | P1.5 compress snapshot + todo template adds “browser verify” for VisualUx | `compression.rs`, todo tool | HA-19 | E8 |
| **9** | **Iteration storm → hard steer** | 15× terminal after block | P1.9 advisory exists; escalate to **block terminal** until perception or operator steer | `harness_advisory.rs` | HA-20e | E9, `0aeef965` |
| **10** | **Spill → forced range read** | 0% artifact follow-up | P2.4 inline 80 lines + require read on artifact path within 2 turns (advisory → steer) | `artifact_spill.rs` | HA-23 | E2 |
| **11** | **Continuation prompts** | Generic stream recovery | P1.6 Hermes branches | `conversation.rs` | HA-20b | E6 |
| **12** | **Replay CI from logs** | Regressions rediscover manually | P2.8 expand `harness_games003_replay.rs` with `0aeef965` timeline | `edgecrab-core/tests/` | HA-27 | all |
| **13** | **Markdown theater cap** | 5+ report files per task | TaskClass footer: “max 1 summary doc”; completion assessor warns | `task_class.rs` | HA-43 (new) | R8, race_gamey |

---

## 90-day success metrics (unchanged targets)

From [008 § Success metrics](./008-improvement-plan.md):

| Metric | Baseline (4 sessions) | Target |
|--------|-------------------------|--------|
| Preview success / visual session | **0/4** | >50% |
| Profile preview active on homelab | **false** | true after migrate |
| Config read attempts after browser block | **≥1/session** | 0 |
| Terminal storm (≥5/60s no perception) | **≥2 sessions** | 0 with steer/block |
| Markdown reports / visual task | **5+** | ≤1 |

---

## Dependency sketch

```text
  Rank 1 (profile merge) ──► Rank 2 (runtime enable) ──► Rank 4 (strict verify)
  Rank 3 (config recovery) ── parallel ──► Rank 6 (port hint)
  Rank 7 (turn budget) ──► Rank 10 (spill read)
  Rank 12 (replay CI) ── validates all P0
```

---

## Explicitly NOT on this backlog

| Idea | Reason |
|------|--------|
| Disable SSRF | Security regression |
| Auto-read full spill into context | Turn budget explosion |
| Hermes global private URLs | Port allowlist is correct design |
| New “done” tool | `CompletionPolicy` sufficient |
| Model swap to Opus | Harness must fix cheap-path first |
