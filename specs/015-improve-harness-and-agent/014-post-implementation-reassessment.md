# 014 — Post-Implementation Reassessment (Jun 2026)

**Date:** 2026-06-19 · **Baseline:** [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md)  
**Backlog closed:** [013-impact-ranked-backlog.md](./013-impact-ranked-backlog.md) ranks 1–5, 9, 12 (partial)

---

## Verdict after implementation

EdgeCrab moved from **“P0 code shipped, ops broken”** to **“harness closes the loop when config exists anywhere in the install tree.”** The homelab failure class (preview OFF under `-p homelab` while global ON) is fixed at load time, not by hand-editing YAML.

Remaining gap: **model behavior** (Haiku still prefers terminal over browser) — now steered harder, not magically fixed.

---

## Scorecard delta

```text
  Dimension                    Before   After   Change
  ───────────────────────────  ───────  ─────   ─────────────────────────────
  Config ↔ runtime parity      ██░░░    ████░   HA-41 merge + migrate on startup
  Q5 Verification (DONE)       █░░░░    ███░░   VisualUx strict by default + HA-43
  J6 Failure recovery          ███░░    ████░   /config steer, no config read jail loop
  Default preview ports        ██░░░    █████   8000 in PreviewConfig default
  Session preview fallback     ░░░░░    ████░   VisualUx auto-enables loopback policy
  CI regression law            ██░░░    ████░   ha41/ha43/ha16 replay tests
```

---

## What was implemented (code anchors)

| Rank | Item | Owner | Gate |
|------|------|-------|------|
| 1 | `load_from_with_global_inheritance`, `merge_global_inherited`, `migrate_profile_preview_from_global` | `config.rs`, `runtime.rs`, `profile.rs` | HA-41 |
| 2 | `apply_visual_ux_session_preview` + `AgentConfig.security_preview` | `task_class.rs`, `agent.rs`, `conversation.rs` | HA-20d |
| 3 | Recovery `/config` not `read_file ~/.edgecrab` | `recovery_catalog.rs`, `harness_advisory.rs` | HA-42 |
| 4 | `effective_verification_strict` + markdown theater block | `task_class.rs`, `completion_assessor.rs` | HA-30/43 |
| 5 | Default port 8000 | `PreviewConfig::default()` | HA-05 |
| 9 | Stronger iteration-storm steer | `harness_advisory.rs` | HA-20e |
| 12 | Replay tests extended | `harness_games003_replay.rs` | HA-27 |

**Already present (unchanged):** turn budget (`turn_dispatch`), todo compress snapshot (`compression.rs`), dev port hints (`dev_server.rs`), spill stubs with inline preview (`artifact_spill.rs`).

---

## Brutal honesty — still weak

| Issue | Status |
|-------|--------|
| `conversation.rs` monolith (~7.5k lines) | ⬜ P2.1 extraction incomplete |
| P1.6 continuation prompts by failure class | ⬜ partial via `mutation_turn_policy` only |
| P2.4 forced spill follow-up read | 🟡 stub has preview; no hard gate |
| P1.7 live `lsof` port discovery | 🟡 infer from command string only |
| Haiku markdown theater under pressure | 🟡 blocked at **completion**; may still waste turns |
| Copilot 30s non-streaming opacity | unchanged (P1.4) |

---

## Expected homelab outcome (next visual session)

```text
  START (-p homelab)
       │
       ▼
  load_from_with_global_inheritance ──► preview ON (from ~/.edgecrab/config.yaml)
       │
       ▼
  VisualUx task ──► session preview fallback if still OFF
       │
       ▼
  browser_navigate 127.0.0.1:8000 ──► ALLOW (port allowlist)
       │
       ▼
  Complete only with browser_snapshot/vision OR NeedsVerification
```

**Operator action:** restart EdgeCrab once (migration runs on `load_runtime`). Verify: `edgecrab doctor harness` → `security.preview enabled`.

---

## Success metrics (re-baseline)

| Metric | Pre-fix (4 sessions) | Target post-fix |
|--------|----------------------|-----------------|
| Preview active on homelab load | 0/4 | 4/4 (inheritance) |
| Config read after browser block | ≥1/session | 0 (recovery steers /config) |
| Completed without perception (VisualUx) | allowed | **NeedsVerification** |
| CI gates HA-41..43 | absent | **passing** |

Cross-ref: [008-improvement-plan.md](./008-improvement-plan.md) · [009-acceptance-criteria.md](./009-acceptance-criteria.md)
