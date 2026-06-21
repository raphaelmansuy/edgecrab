# 006 — Brutal Verdict (Jun 2026)

**Date:** 2026-06-19  
**Cross-ref:** [003-forensics](./003-session-forensics.md) · [004-hermes](./004-hermes-comparator.md) · [005-code-audit](./005-code-audit.md) · [015/012](../015-improve-harness-and-agent/012-brutal-assessment-jun2026.md)

---

## Verdict (read this first)

EdgeCrab built a **serious tool OS** and stapled a **completion assessor** on the end. That is not the same as a **task-completion harness**. Homelab evidence is unambiguous: **six visual-UX sessions, zero CLI browser verification, two marked completed**, while agents produced **markdown theaters and fake headless scripts** instead of screenshots. Spec 015 P0 code largely exists; **operators still lose** because config inheritance does not show up in the active profile file, doctor harness **cannot read its own JSONL**, and loop physics **allow terminal storms** while completion gates **sleep until the last API call**. Hermes is uglier architecturally but **more honest when a turn ends** and **easier to preview localhost**. EdgeCrab should keep its security model and Rust types — and **arm the guardrails it already wrote**.

---

## Scorecard

```text
  Dimension                    Score   Evidence
  ───────────────────────────  ─────   ─────────────────────────────────────────
  J1 Schema / code-is-law      ████░   strict tools, indexed wire
  J2 Dispatch                  █████   JoinSet, dedup, parallel safe
  J3 Pre-dispatch validation   ████░   budget gate; patch steer partial
  J4 Effect mediation          █████   path jail, SSRF, command scan
  J5 Result shaping            ███░░   spill good; follow-up read ~0%
  J6 Failure recovery          ███░░   recovery_catalog; config path still confusing
  J7 Cost / liveness           ███░░   compression OK; Copilot 30s opaque
  Q1 Progress / liveness       ████░   activity shelf excellent
  Q5 Verification (DONE)       ██░░░   policy exists; physics weak; 2 false done
  Config ↔ runtime parity      ██░░░   HA-41 in code; YAML + doctor disagree
  Observability                ██░░░   rich logs; analyzer broken on JSONL
  Hermes loop modularity       ██░░░   behind on prologue/epilogue split
  Maintainability              ██░░░   conversation.rs monolith
```

**Net:** **A** tool harness, **C-** task harness for visual/coding missions on cheap models.

---

## What is RIGHT (protect)

| # | Fact | Why it matters |
|---|------|----------------|
| 1 | Port-scoped preview vs Hermes `allow_private_urls` | Correct security posture for agentic browser |
| 2 | `RunOutcome` / `CompletionDecision` / `ExitReason` | Right operator contract long-term |
| 3 | `recovery_catalog` + `/config` steer (HA-42) | Fixes config jail spiral **when model obeys** |
| 4 | `effective_verification_strict` for VisualUx | Right epistemology — must run **every** assess path |
| 5 | Activity shelf + `TurnActivityState` | Q1 answered better than most agents |
| 6 | `harness.jsonl` structured telemetry | Foundation for replay + doctor (when parser fixed) |
| 7 | Replay test `harness_games003_replay.rs` | Law in CI beats hope |

---

## What is WRONG (ranked, brutal)

### R1 — VERIFY is not loop physics (**CRITICAL**)

```text
  OBSERVED:  terminal×158 → write markdown → "completed?"
  REQUIRED:  perceive → evidence artifact → Completed | NeedsVerification
```

- `harness_advisory` **whispers**; it does not **block**.
- Mid-loop assess skips gates (`HarnessSnapshot::default()`).
- `e22c0a28`, `b84c7ba4`: DB `model_returned_final_text` with **zero perception**.

### R2 — Config ops still broken for homelab (**CRITICAL**)

```text
  global preview: ON     profile preview key: ABSENT     doctor: DISABLED
```

HA-41 merge + migrate coded; **profile YAML on disk unchanged**; `doctor harness` warns on every run. Session `d4d6b6b4` **post-fix** still shows terminal-heavy pattern.

### R3 — Doctor harness lies (**HIGH**)

973-line JSONL → `tool_starts: 0`. Operators cannot trust `edgecrab doctor harness` for regression triage until **HA-44** JSONL parser lands.

### R4 — Guardrails unarmed (**HIGH**)

`hard_stop_enabled: false` + `halt_decision` unwired = **infinite ACT** on Haiku. Hermes defaults are softer too, but EdgeCrab **built** block/halt and left them unloaded.

### R5 — Markdown / script theater (**HIGH**)

`race_game_z`: `VERIFICATION_EVIDENCE.md`, `headless_verify.js`, `screenshot.js` — **simulacra of verify**. HA-43 blocks at completion; **does not stop turn waste**.

### R6 — Monolith debt (**MEDIUM**)

`conversation.rs` ~7.6k lines — inconsistent harness behavior risk increases every sprint.

### R7 — Spill → blind rewrite (**MEDIUM**)

49% terminal + 22% write; spill artifacts ignored. Models fork `game-beautiful.*` instead of patching.

### R8 — Hermes gaps not ported (**MEDIUM**)

Error classifier, compression lineage, turn finalizer warnings, background review — all documented, none blocking but compound VERIFY gap.

---

## Unified root-cause chain

```text
                    ┌──────────────────────────────────┐
                    │ Visual UX prompt (demo/*)         │
                    └───────────────┬──────────────────┘
                                    │
                    ┌───────────────▼──────────────────┐
                    │ TaskClass::VisualUx (heuristic)   │
                    └───────────────┬──────────────────┘
                                    │
         ┌──────────────────────────▼──────────────────────────┐
         │ Preview OFF at process start (profile YAML + merge)   │ R2
         └──────────────────────────┬──────────────────────────┘
                                    │
         ┌──────────────────────────▼──────────────────────────┐
         │ browser_navigate FAIL (SSRF)                          │
         └──────────────────────────┬──────────────────────────┘
                                    │
      ┌─────────────────────────────┼─────────────────────────────┐
      │                             │                             │
      ▼                             ▼                             ▼
 recovery steer              terminal storm                 write theater
 (/config — ignored)          (no block) R4                (md + .js) R5
      │                             │                             │
      └─────────────────────────────┴─────────────────────────────┘
                                    │
                    ┌───────────────▼──────────────────┐
                    │ Model returns text                │
                    └───────────────┬──────────────────┘
                                    │
              ┌─────────────────────▼─────────────────────┐
              │ Mid-loop assess (empty harness) — may exit  │ R1
              └─────────────────────┬─────────────────────┘
                                    │
              ┌─────────────────────▼─────────────────────┐
              │ End assess: strict OR completed OR interrupt │
              └────────────────────────────────────────────┘
```

---

## Spec 015 / 014 honesty check

| Claim (014) | 016 truth |
|-------------|-----------|
| "homelab failure class fixed at load time" | **Partial** — `apply_visual_ux_session_preview` helps per-session; profile file + doctor still show OFF |
| "VisualUx strict by default" | **Code yes** — `effective_verification_strict`; **logs show** `e22c0a28` completed earlier |
| "CI gates HA-41..43 passing" | **Unit tests pass** — **ops path does not** |
| "model behavior still weak" | **Correct** — but harness must not allow false done |

---

## Bottom line

| Question | Answer |
|----------|--------|
| Is EdgeCrab a good agent runtime? | **Yes** — dispatch, security, streaming |
| Can it close visual UX tasks on Haiku? | **No** — not reliably today |
| Is Hermes strictly better? | **No** — different tradeoffs; **better dev preview + finalize** |
| Biggest single fix? | **Arm VERIFY in the loop** — block terminal storms, fix JSONL doctor, prove preview merge in ops |
| Ship blocker? | **False `completed`** on zero perception — destroys operator trust |

Next: [007-priority-backlog](./007-priority-backlog.md)
