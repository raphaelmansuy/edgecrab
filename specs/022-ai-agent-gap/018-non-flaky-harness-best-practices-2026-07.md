# 018 — Non-Flaky AI Agent Engineering · Harness Best Practices (July 2026)

**Status:** Research + brainstorm (actionable)  
**Date:** 2026-07-19  
**Inputs:** Live sessions ([017](017-session-forensics-2026-07-19.md)) · EdgeCrab harness code · Hermes peer · Anthropic harness papers (Nov 2025, Mar 2026) · industry patterns  
**Goal:** Practices that **reduce flaky agent runs** — not “smarter prompts alone.”

---

## 0. Definition: what “non-flaky” means for an agent harness

A harness is **non-flaky** when the same class of task, under the same tools and budgets, tends to:

| Property | Meaning |
|----------|---------|
| **Deterministic gates** | Pass/fail from observables (exit codes, HTTP body, file size), not model mood |
| **Monotone progress** | After success latch, no re-exploration of the same failure mode |
| **Bounded thrash** | Same tool + same failure fingerprint cannot burn unbounded tokens |
| **Honest completion** | “Done” iff evidence state machine says done — never self-praise alone |
| **Recoverable** | One structured recovery recipe per failure class, not N conflicting advisories |
| **Replayable** | Session can be explained from SQLite + harness metrics without the model |

**Flaky harness anti-pattern (observed 2026-07-19):**  
Create succeeds → verify demanded → transport `ok:true` + content fail → advisory spam → port thrash → wrong oracle → interrupt / budget_exhausted. Same intent retried 5× same day.

---

## 1. Evidence base

### 1.1 Live EdgeCrab sessions (homelab, 2026-07-19)

| Session | End | Burn | Diagnosis |
|---------|-----|------|-----------|
| Pinguin `e256f862` | interrupted | 1.6M in / 5 min | 2 writes OK; 50+ verify thrash; `node --check` ESM false fail |
| Hyperframe `cb635449` | budget_exhausted | **4.3M** in / 14 min | vision-heavy spin; no media_render latch |
| Day pattern | mostly interrupt/budget | multi-M tokens | **systemic verify spinlock** |

Full forensics: [017](017-session-forensics-2026-07-19.md).

### 1.2 EdgeCrab harness law (what already exists)

| Module | LOC-ish | Strength | Gap vs non-flaky |
|--------|---------|----------|------------------|
| `completion_assessor.rs` | ~1.1k | Typed `RunOutcome`; chrome-error not evidence | Navigate `ok:true` + **Error response** title still weak |
| `harness_gates.rs` | ~420 | Deterministic oracles | **`.js` → `node --check` always** — flaky for browser ESM |
| `harness_loop_policy.rs` | ~100+ | Hard-stop ON; visual_storm exempts preview serve | Does not **latch** successful serve |
| `harness_advisory.rs` | ~600 | Mutation-without-verify | Rate not hard-capped; injects as user msgs |
| `task_class.rs` | ~850 | visual_ux, document latch | Missing **media_render** subclass |
| `recovery_catalog.rs` | large | Bind-wait, port heal recipes | Alternate port 8010 **reopens shopping** |
| `dev_server` + process_table | solid | Session HTTP port recording | Latch not first-class completion state |
| `document_done_latch` | good | Ends turn on artifact evidence | No twin for **preview_ok** |
| Goals SQLite | present | AE2 surface | **Not auto-seeded** on visual_ux |
| Prompt cache split | strong | 95% cache hit in thrash sessions | Thrash still burns **dynamic** + tool tokens |

### 1.3 Hermes (peer harness)

| Pattern | Hermes | Non-flaky lesson |
|---------|--------|------------------|
| `verification_stop` | Soft string nudge; capped attempts | Cap nudges; don’t infinite-continue |
| Guardrails hard_stop | **Default OFF** | EC better default ON — keep |
| Credential pool | Deep 429 recovery | Separate from task thrash |
| Tool result storage | Spill | Same as EC artifact_spill |

### 1.4 Industry SOTA (July 2026 reading)

**Anthropic — Effective harnesses for long-running agents (Nov 2025):**

- Initializer vs coding agent; **feature list JSON** with `passes: false`  
- Incremental one-feature-at-a-time  
- **Clean state** handoff: git + progress file  
- End-to-end browser verification as human would  
- Failure modes: one-shot, premature victory, untested “done”

**Anthropic — Harness design for long-running apps (Mar 2026):**

- **Generator ≠ evaluator** (self-praise is flaky by construction)  
- Sprint/contract “done” negotiated **before** code  
- Evaluator uses browser automation; hard thresholds  
- **Strip harness as models improve** — only load-bearing scaffold  
- Cost/quality trade: multi-agent expensive; use when task is past solo reliability  

**Claude Code product (Jul 2026):** `/verify` is **explicit**, not auto-run forever — human-gated verification reduces thrash.

**Headroom / proxy practice:** Prefix-cache stability under concurrent agents — thrash that rotates system-ish messages **destroys cache** (cost flakiness).

**Agent-deck / industry:** Persistence + verification as **hard products**, not afterthoughts.

---

## 2. First principles (non-negotiable)

```text
1. The model is a proposer. The harness is the judge of "done."
2. Transport success ≠ content success ≠ task success.
3. Every advisory must either change state or be rate-limited.
4. Oracles must match the artifact class or they inject flakiness.
5. After value is latched, verify budget is separate and small.
6. Self-evaluation without external tools is marketing, not QA.
7. Complexity that no longer pays for itself must be deleted.
```

Map to EdgeCrab AE (001):

| Principle | AE | Session proof |
|-----------|-----|---------------|
| Bounded autonomy | AE1 | Hard-stop ON helps; thrash still burns budget |
| Cross-window progress | AE2 | No goals → amnesia of plan |
| Completion = evidence | AE3 | Demanded; path broken |
| Tool truth | AE4 | Envelopes good; content classifiers incomplete |
| Cache-stable prompts | AE5 | 95% hit; still thrash dynamic tools |
| Classify → recover | AE6 | Recovery catalog exists; not latch-bound |
| Mediated I/O | AE7 | Preview OK; port shopping residual |
| Human sovereignty | AE8 | Interrupt works; no mid-loop progress UX |
| Observability | AE9 | SQLite strong; thrash metrics weak |
| Extend without bloat | AE10 | Don’t add more advisories without latch |

---

## 3. Non-flaky practice catalog (ranked for EdgeCrab)

### P0 — Fix flakiness observed in production sessions

#### BP-01 · Evidence state machine (latches)

**Practice:** Explicit latches, not vibes.

```text
ArtifactLatch   → files exist matching intent paths
PreviewLatch    → {dir, port, url, process_id, bind_ready, http_200_at}
PerceiveLatch   → snapshot/vision not chrome-error AND not Error response
OracleLatch     → class-aware gates green
Done            → all required latches for task_class
```

**EC today:** Document done latch exists; **preview/perceive not first-class**.  
**Borrow:** Anthropic feature `passes` + EC document latch.  
**Non-flaky rule:** Once `PreviewLatch` set, **forbid** re-serve / port change unless artifact hash under `dir` changes.

#### BP-02 · Content truth over transport `ok`

**Practice:** Classify tool results into:

| Layer | Example |
|-------|---------|
| Transport | CDP connected, HTTP status |
| Content | Title “Error response”, empty body, node_count=0 |
| Task | Canvas present / game HUD / render file size |

**EC today:** `is_chrome_error` handled; **Python Error response pages can still be `ok:true` navigate**.  
**Fix:** `structured_browser` + assessor treat known error titles/bodies as **content_fail**.

#### BP-03 · Class-aware deterministic oracles

**Practice:** Oracle = function of (path, task_class, package context).

| Class | Oracle |
|-------|--------|
| Node library `.cjs` / package type commonjs | `node --check` |
| Browser ESM + importmap peer HTML | HTTP 200 + markers / browser snapshot |
| Hyperframe / video | `output.mp4` exists, size > 0, optional ffprobe |
| Rust | `cargo check -p …` (already natural) |

**EC today:** `oracle_command_for_path` → always `node --check` for `.js`.  
**This alone makes visual demos flaky.**

#### BP-04 · Thrash circuit breaker (fingerprint × N)

**Practice:** Guardrails on **failure fingerprints**, not only tool name counts.

```text
fingerprint = hash(tool, normalized_args_shape, error_code, content_class)
if count(fingerprint) >= N in window → thrash_blocked + ONE recovery recipe
```

**EC today:** exact_failure_block_after=4, same_tool_halt=6; visual_storm; port shopping block.  
**Gap:** Successful transport + content_fail **restarts counter**; advisories re-inject.  
**Non-flaky:** content_fail fingerprints count; after block, only recovery_catalog step allowed.

#### BP-05 · Single recovery recipe (no advisory stacking)

**Practice:** At most **one** harness user-injection per failure class per 90s; supersede, don’t stack.

**Observed flaky:** 4 synthetic users in 4 minutes (mutation, no server, don’t stop, gates) → model context filled with **contradictory urgency**.  
**Borrow:** Claude Code “verify when asked”; Anthropic one contract at a time.

#### BP-06 · Split create budget vs verify budget

**Practice:**

```text
create_budget:  high tool allowance for write/patch
verify_budget:  small (e.g. 8 tools) after ArtifactLatch
if verify exhausts → exit NeedsEvidence | escalate human
  NOT budget_exhausted after 50 thrash tools
```

**Session A:** ~4 create tools, ~55 verify → wrong budget accounting.  
**Non-flaky:** RunOutcome reflects **which** budget died.

---

### P1 — Structural practices (SOTA harness)

#### BP-07 · Auto-goal / feature contract outside history

**Practice:** Standing goals in SQLite (EC has tables); auto-seed visual_ux:

1. Write artifacts  
2. Latch preview  
3. Perceive (browser)  
4. Stop  

**Anthropic:** feature_list.json `passes` field.  
**EC:** `/goal` exists but **unused** in thrash sessions.  
**Non-flaky:** Goals survive compress; inject as **user-role goal block** (cache-safe), never rewrite stable system prompt.

#### BP-08 · Generator / evaluator separation for subjective + UI

**Practice:** When task is visual or “is it good?”, **do not trust generator self-score**.

Options (light → heavy):

| Mode | When |
|------|------|
| Deterministic gates only | HTML demo, file render |
| Same model, second pass with QA system prompt + browser only | Medium |
| Separate subagent evaluator | Long / expensive builds (Anthropic Mar 2026) |

**Non-flaky:** Evaluator **cannot** write product files; only write critique + fail/pass.

#### BP-09 · Clean-state handoff (multi-window)

**Practice:** For multi-hour work: progress file + git commit + “what works” smoke.  
**EC:** goals + parent_session_id + memory — partial.  
**Borrow:** Anthropic initializer + progress.txt pattern for **goal mode / Ralph loop**.

#### BP-10 · Explicit verify skill, not infinite auto-verify

**Practice:** Harness **requires** evidence before Complete, but **does not** free-run verify forever.  
After thrash breaker: surface operator: “Preview failed; press continue or `/verify`.”  
Aligns with Claude Code 2.1.x: verify not auto-spam.

#### BP-11 · Task subclass: `media_render` ≠ `visual_ux`

**Practice:** Hyperframe/video done = **artifact + optional probe**, browser optional.  
**Session B flakiness:** treated like web UX → 21× browser_vision.

#### BP-12 · Progress speech every K tools

**Practice:** Empty ReAct (55/58 empty assistant) is unsteerable and hard to debug.  
Force short status every K tools or on latch change — for **operator + next self**.

---

### P2 — Hardening / anti-flaky engineering

#### BP-13 · Golden session replay tests

**Practice:** Fixture: messages + tool results from pinguin thrash → assessor + gates must:

- not complete early  
- content_fail Error response  
- not fail solely on ESM `node --check`  
- trip thrash breaker by tool 4  

**Non-flaky harness is CI-locked**, not vibe-tuned.

#### BP-14 · Cache-safe harness injections

**Practice:** Advisories/goals/gates go to **messages** (dynamic), never stable system.  
EC mostly does this; keep law. Avoid rotating “system-reminder” style that breaks prefix cache (Headroom lesson).

#### BP-15 · Delete non-load-bearing scaffold as models improve

**Practice:** Anthropic Mar 2026: strip sprints when Opus handles coherence.  
**EC:** Re-audit visual_storm thresholds quarterly; don’t add third advisory channel.

#### BP-16 · Cost / thrash observability

**Practice:** Per session metrics:

```text
create_tools, verify_tools, latch_states, thrash_blocks,
oracle_failures_by_class, cache_hit_ratio, end_reason
```

Make day rollups (017 §1) a first-class `harness_analyzer` report.

#### BP-17 · Parallel tool safety already in EC

Keep: parallel_safe tools, spill, SSRF, path jail.  
Non-flaky includes **security defaults that don’t force guesswork** (preview enable on visual_ux is correct).

---

## 4. Architecture brainstorm (target state)

```text
                    ┌─────────────────────────────┐
  User intent ───▶  │ task_class + auto-goals     │
                    └─────────────┬───────────────┘
                                  ▼
                    ┌─────────────────────────────┐
                    │ CREATE phase                │
                    │ write/patch · ArtifactLatch │
                    └─────────────┬───────────────┘
                                  ▼
                    ┌─────────────────────────────┐
                    │ PREVIEW phase (once)        │
                    │ serve recipe → PreviewLatch │
                    │ (no re-serve unless dirty)  │
                    └─────────────┬───────────────┘
                                  ▼
                    ┌─────────────────────────────┐
                    │ PERCEIVE phase (bounded)    │
                    │ navigate+snapshot/vision    │
                    │ content_fail → 1 heal only  │
                    └─────────────┬───────────────┘
                                  ▼
                    ┌─────────────────────────────┐
                    │ ORACLES (class-aware)       │
                    │ then Done / Escalate        │
                    └─────────────────────────────┘

  Optional heavy path: Generator ⇄ Evaluator subagent (UI quality)
```

**SOLID ownership:**

| Concern | Owner |
|---------|--------|
| Latch state | `edgecrab-core` harness state (session-scoped) |
| Content classifiers | `structured_browser` + assessor |
| Oracles | `harness_gates` (class inputs) |
| Thrash | `tool_loop_guardrails` + fingerprint |
| Recovery one-shot | `recovery_catalog` (no alternate ports without latch clear) |
| Goals | existing goals tables + auto-seed |
| Evaluator role | `delegate_task` or future `evaluate_visual` toolset |

---

## 5. What to **not** do (anti-patterns)

| Anti-pattern | Why flaky |
|--------------|-----------|
| More `[harness]` user messages | Context noise; contradictory urgency |
| LLM-as-judge for binary syntax | Use `node --check` only when Node is the runtime |
| Port “heal” that suggests 8010 freely | Reopens shopping (recovery_catalog alternate) |
| Complete on “I tested with curl” for UI | Anthropic: need browser-as-user |
| Infinite verify_on_stop | Soft loops burn tokens (Hermes risk if uncapped) |
| Generator grades own design | Systematic positive bias |
| Counting transport ok as evidence | Session A Error response |
| One global oracle for all `.js` | ESM demos |

---

## 6. Priority roadmap (non-flaky first)

Aligns with [017 AC-V1…V8](017-session-forensics-2026-07-19.md):

| Week | BP | Deliverable |
|------|-----|-------------|
| **1** | 02, 03 | Content_fail + class-aware oracles |
| **1** | 01, 04, 05 | Preview latch + thrash fingerprint + advisory rate limit |
| **2** | 06, 07, 11 | Create/verify budgets · auto-goal · media_render |
| **2** | 13 | Golden replay tests from session dumps |
| **3** | 08, 10 | Optional evaluator path · operator `/verify` |
| **3** | 16 | harness_analyzer thrash report |
| **later** | 09, 15 | Multi-window progress file · harness strip review |

---

## 7. Success metrics (non-flaky KPIs)

| KPI | Baseline (2026-07-19) | Target 30d |
|-----|----------------------|------------|
| visual_ux end `budget_exhausted` rate | high on Hyperframe day | **&lt; 10%** |
| verify_tools / create_tools ratio | ~10–20× | **≤ 3×** |
| Sessions with PreviewLatch | ~0 | **&gt; 80%** of visual demos that write HTML |
| False oracle fail on browser ESM | 1/1 pinguin | **0** |
| Mean input tokens / successful demo | multi-M retries | **&lt; 400k** first success |
| Same failure fingerprint &gt; N without block | common | **0 in CI** |

---

## 8. Deep brainstorm: “best harness” synthesis

If we distilled **best-in-class July 2026** into one sentence each:

| Source | Sentence |
|--------|----------|
| Anthropic Nov 2025 | **Structure the environment** so a cold agent can resume: features, progress, clean git. |
| Anthropic Mar 2026 | **Separate generation from judgment**; grade with criteria + tools, not self-talk. |
| Claude Code product | **Verify on demand** with hard skills; don’t auto-thrash. |
| EdgeCrab today | **Typed completion + hard-stop + security + SQLite** — keep; finish the **latch graph**. |
| Hermes | Soft verify + rich recovery — borrow **nudge caps**, not soft hard-stop defaults. |
| Sessions 017 | The missing product is **preview_ok as a first-class latch**, not another advisory. |

**EdgeCrab’s unfair advantage:** Rust typed `RunOutcome`, document latch, recovery_catalog, structured browser, goals SQLite, hard-stop ON.  
**EdgeCrab’s unfair failure:** partial implementation of AE3 for visual_ux — **policy without latch**.

---

## 9. Concrete code touch list (when implementing)

| BP | Primary files |
|----|----------------|
| Content_fail | `structured_browser.rs`, `completion_assessor.rs` |
| Class oracles | `harness_gates.rs` (`oracle_command_for_path`) |
| Preview latch | new `preview_latch.rs` or extend `dev_server` + session state |
| Thrash fingerprint | `tool_loop_guardrails.rs`, `harness_advisory.rs` |
| Advisory rate | `harness_advisory.rs`, conversation inject sites |
| media_render | `task_class.rs`, assessor evidence collectors |
| Auto-goal | conversation prologue / task_class advisory path |
| Golden tests | `edgecrab-core/tests/harness_*` + fixtures from 017 IDs |

---

## 10. One-line doctrine

**Non-flaky agent engineering (July 2026) = latched external evidence + class-aware deterministic gates + thrash-bounded recovery + optional separated evaluation — never “try browser again” as a strategy.**

---

## Cross-refs

| Doc | Role |
|-----|------|
| [001](001-first-principles.md) | AE / J rubric |
| [003](003-ai-engineer-harness-lens.md) | Loop physics |
| [014](014-improvement-plan.md) | Prior harness plan |
| [017](017-session-forensics-2026-07-19.md) | Session evidence + AC-V* |
| Anthropic Nov 2025 | Long-running harness |
| Anthropic Mar 2026 | Planner/generator/evaluator |
