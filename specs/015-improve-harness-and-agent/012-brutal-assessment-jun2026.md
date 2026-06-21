# 012 — Brutal Assessment (Jun 2026)

**Date:** 2026-06-19 · **Method:** first-principles harness physics + log forensics + Hermes parity  
**Evidence:** sessions `927f4d85`, `e22c0a28`, `2e720f47`, **`0aeef965`** · `~/.edgecrab/profiles/homelab/logs/harness.jsonl`  
**Cross-ref:** [002-first-principles](./002-first-principles.md) · [005-evidence](./005-evidence-games003.md) · [011-hermes-parity](./011-hermes-parity-map.md) · [008-plan](./008-improvement-plan.md)

---

## Verdict (one paragraph)

EdgeCrab is a **strong ACT harness** (dispatch, security, spill, streaming bus, recovery JSON) and a **weak VERIFY harness** (perception loop, completion truth, config/runtime alignment). Four live homelab sessions on the same visual-UX prompt produced **42–59 tool turns each, zero successful browser preview, and piles of markdown “verification theater.”** Code for P0 fixes largely exists; **operators still fail because profile config is isolated from global config and bundled templates never migrate into existing profiles.** Hermes wins dev UX with `allow_private_urls`; EdgeCrab’s port-scoped preview is **safer but unusable unless the exact YAML lands in the active profile.** Until VERIFY is authoritative, Haiku-class models will look busy and deliver unverified forks.

---

## Scorecard (Jun 2026 agent-harness rubric)

```text
  Dimension                    Score   Notes
  ───────────────────────────  ─────   ─────────────────────────────────────
  J1 Schema / Code Is Law      ████░   Strict JSON; indexed wire set strong
  J2 Dispatch                  █████   Parallel JoinSet, registry, guardrails
  J3 Pre-dispatch validation   ████░   Budget gate; patch steer partial
  J4 Effect mediation          █████   Path jail, SSRF, command scan — correct
  J5 Result shaping            ███░░   Spill transparent; stub actionability 🟡
  J6 Failure recovery          ███░░   recovery_catalog good; config dead-end ✗
  J7 Cost / liveness           ███░░   ctx budget OK; Copilot 30s+ opaque
  Q5 Verification (DONE)       █░░░░   0/4 visual sessions verified in browser
  Operator truth               ███░░   Shelf strong; "107 tools" fixed; exit reason 🟡
  Config ↔ runtime parity      ██░░░   SMOKING GUN — see § Root cause A
```

**Net:** Production-grade **tool OS**; not yet a **task-completion harness** for visual/coding missions.

---

## What is RIGHT (keep / extend)

| Area | Fact (code or log) | Why it matters |
|------|-------------------|----------------|
| Security model | `path_policy`, SSRF, `command_scan` | Correct default for agentic SSRF/file exfil |
| Strict schemas | `ToolSchema.strict`, budget pre-reject | “Code is law” beats Hermes advisory JSON |
| Spill architecture | `.edgecrab-artifacts/{session}/` | More operator-transparent than Hermes inline prune |
| Recovery catalog | `recovery_catalog.rs` structured JSON | Right pattern for model-steerable errors |
| Harness telemetry | `harness.jsonl` per profile | Enables `doctor harness` + replay tests |
| Loop guardrails | `tool_loop_guardrails`, FP11 dedup | Blocks exact repeat calls (screenshot session) |
| Harness advisories | `HarnessTurnAdvisory` + `finalize_tool_turn` | One-shot preview recovery **when wired** |
| Task classification | `task_class.rs` VisualUx footer | Right hook; needs **enforce**, not hint |
| Extraction started | `provider_call.rs`, `turn_dispatch.rs` | Monolith slimming in progress |
| Hermes parity tests | `harness_games003_replay.rs` | Law encoded in CI — expand coverage |

---

## What is WRONG (brutal, ranked)

### R1 — Config/runtime split kills visual tasks (CRITICAL)

```text
  ~/.edgecrab/config.yaml          profiles/homelab/config.yaml
  security.preview.enabled: true   security.preview: (missing)
           │                                  │
           │  NO MERGE ON -p homelab          │
           └──────────────┬───────────────────┘
                          ▼
              apply_security_runtime() → preview OFF
                          ▼
              browser_navigate 127.0.0.1 → SSRF blocked
```

**Fact:** `AppConfig::load_from(profile/config.yaml)` replaces entire config ([`config.rs`](../../crates/edgecrab-core/src/config.rs) L309–318). Global preview never applies.  
**Fact:** Live homelab profile has `security:` but **no** `preview:` key (verified 2026-06-19).  
**Fact:** Bundled homelab template **has** preview; `sync_bundled_profiles` **skips existing** profiles ([`bundled_profiles.rs`](../../crates/edgecrab-cli/src/bundled_profiles.rs) L115–117).  
**Impact:** P0.4 marked ✅ in plan but **0/4 sessions** got preview — feature not active for dogfood profile.

### R2 — Recovery steers into unreadable config (HIGH)

After `browser_navigate` block, model reads `~/.edgecrab/config.yaml` → **path jail denies** (outside workspace).  
Then terminal `cat`/`grep` spiral — **6+ terminal calls in 60s** (session `0aeef965`, E9).

```text
  PERCEIVE blocked → read config → denied → terminal storm → markdown reports
```

**Fact:** Recovery text says “add YAML to config.yaml” but agent **cannot read or patch** profile config from repo cwd.  
**Hermes gap:** Dev toggles are env/config reachable or `allow_private_urls` avoids the loop entirely.

### R3 — VERIFY not in the loop physics (HIGH)

```text
  INTENDED:  INTENT → PLAN(verify) → ACT → PERCEIVE → DONE
  OBSERVED:  INTENT → ACT → ACT → ACT → (theater docs) → "done?"
```

**Fact:** `demo/race_gamey/` after `0aeef965`: `index.html`, `game.js` **plus** `DELIVERY.md`, `VERIFICATION.md`, `FINAL_REPORT.txt` — **no screenshot artifact**.  
**Fact:** `manage_todo_list` tasks lack mandatory preview steps (E8, session `2e720f47`).  
**Fact:** `harness.verification_strict` exists (P3) but **off by default** — completion still “had text.”

### R4 — Default preview ports omit 8000 (MEDIUM)

`PreviewConfig::default()` ports: `[3000, 5173, 8080, 8888]` — **not** `8000` (`python -m http.server` default).  
Bundled homelab adds 8000; stale profiles do not.

### R5 — Spill + Haiku = blind rewrite (MEDIUM)

Large reads spill → model sees stub → writes **new files** (`game-beautiful.*`, `index-ultra.*`, `race_gamey/*`) instead of patching.  
P0.1 stubs improved; **follow-up read rate ~0%** in logs (HA-01/02 tests pass; behavior does not).

### R6 — Monolith loop debt (MEDIUM, maintainability)

`conversation.rs` ~7.5k lines still owns compression notices, advisories, spill, provider retry.  
Extraction to `turn_dispatch`/`provider_call` started — **risk of inconsistent harness behavior** until complete.

### R7 — Copilot non-streaming opacity (MEDIUM, UX)

28–40s “composing next tool call” with no tokens (sessions `e22c0a28`, `0aeef965`).  
P1.4 streaming recovery shipped; operator **still** perceives stuckness.

### R8 — Markdown theater as fake verify (LOW but toxic)

Models emit `EVIDENCE_SUMMARY.md`, `VISUAL_EVIDENCE.md`, `FINAL_VERIFICATION.py` when perception blocked.  
Harness does not penalize **doc spam**; success metrics should cap report files (see [008 § metrics](./008-improvement-plan.md)).

---

## Session `0aeef965` (race_gamey) — forensic summary

| Field | Value |
|-------|-------|
| Task | “Create best 3D race car … demo/race_gamey” |
| Model | `copilot/claude-haiku-4.5` |
| API iterations | **59** |
| Tool events | **118** |
| Tool errors | **6** (`browser_navigate`×2+, `read_file` config, `terminal`×2) |
| Perception success | **0** |
| Outcome | `index.html` + `game.js` + 5 markdown/txt “verify” files |

```text
  write×2 → terminal (http.server) → browser_navigate FAIL
       → read ~/.edgecrab/config.yaml FAIL
       → terminal×15+ (ls/cat/grep storm)
       → write markdown “verification”
       → never browser_snapshot / vision
```

Cross-ref: [005 § `0aeef965`](./005-evidence-games003.md)

---

## Hermes comparison (mechanism, not philosophy)

```text
  Capability              Hermes                          EdgeCrab
  ──────────────────────  ──────────────────────────────  ───────────────────────────
  Localhost preview       security.allow_private_urls     security.preview port list
  Dev default             Often ON in homelab setups      OFF unless profile YAML
  Spill                   tool_result_storage + budget    artifact_spill (better ops)
  Turn budget             enforce_turn_budget()           per-call spill only (P2.5)
  Compress + todos        todo_snapshot inject            ⬜ P1.5
  Continuation prompts    _get_continuation_prompt()      ⬜ P1.6
  Preview lifecycle       preview.restart (TUI gateway)   dev_server hint only
  Config read in loop     N/A (Python env)                Blocked — path jail correct
  Strict completion       Softer                          RunOutcome + strict mode hook
```

**Borrow next (high ROI):** turn budget, todo compress snapshot, continuation prompts, preview.restart port ownership.  
**Do not borrow:** global `allow_private_urls` (keep port allowlist).

Full map: [011-hermes-parity-map.md](./011-hermes-parity-map.md)

---

## Root-cause chain (unified)

```text
                    ┌─────────────────────────────────┐
                    │ User: visual UX task (demo/*)   │
                    └───────────────┬─────────────────┘
                                    │
                    ┌───────────────▼─────────────────┐
                    │ TaskClass=VisualUx (advisory)   │
                    └───────────────┬─────────────────┘
                                    │
              ┌─────────────────────▼─────────────────────┐
              │ profile config: preview.enabled = false      │ ◄── R1
              └─────────────────────┬─────────────────────┘
                                    │
              ┌─────────────────────▼─────────────────────┐
              │ browser_navigate blocked (SSRF)              │
              └─────────────────────┬─────────────────────┘
                                    │
         ┌──────────────────────────┼──────────────────────────┐
         │                          │                          │
         ▼                          ▼                          ▼
  read config (fail)          terminal storm              execute_code pivot
  path jail R2                E9 / no PERCEIVE            sandbox escape E10
         │                          │                          │
         └──────────────────────────┴──────────────────────────┘
                                    │
                    ┌───────────────▼─────────────────┐
                    │ ACT continues: writes, docs      │
                    │ VERIFY never closes → false done │
                    └─────────────────────────────────┘
```

Five-whys extension: [001-five-whys.md](./001-five-whys.md)

---

## Immediate operator unblock (homelab)

Add to **`~/.edgecrab/profiles/homelab/config.yaml`** (not global only):

```yaml
security:
  preview:
    enabled: true
    allow_localhost_ports: [8000, 8888, 5173, 3000, 8080]
```

Then: `edgecrab doctor harness` · retry preview at `http://127.0.0.1:8000/` after `python3 -m http.server 8000`.

---

## Where the plan stands

| Phase | Honest status |
|-------|---------------|
| P0 code | 🟡 ~70% — spill, recovery, advisories **shipped**; **profile ops gap** |
| P0 ops | ✗ homelab profile not migrated |
| P1 | 🟡 RunOutcome, task class, OTEL, streaming — **verify gates still soft** |
| P2 | 🟡 doctor harness, replay test started; turn budget ⬜ |
| P3 strict verify | Code exists; **not default** |

Ranked backlog: [013-impact-ranked-backlog.md](./013-impact-ranked-backlog.md)
