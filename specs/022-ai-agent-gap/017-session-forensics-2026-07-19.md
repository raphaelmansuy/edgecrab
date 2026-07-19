# 017 — Session Forensics (2026-07-19) · First Principles

**Status:** Evidence note (code is law + SQLite is law)  
**Date:** 2026-07-19  
**Profile:** `homelab` · DB `~/.edgecrab/profiles/homelab/state.db`  
**Authority:** [001-first-principles.md](001-first-principles.md) · AE1–AE10 · J1–J10  
**Related:** [003 harness lens](003-ai-engineer-harness-lens.md) · [014 plan](014-improvement-plan.md) · `edgecrab-tools/src/harness_gates.rs` · `completion_assessor.rs` · `task_class.rs`

---

## 0. Intent of this note

Turn **one day’s live CLI sessions** into:

1. A first-principles diagnosis (not vibes)  
2. A day-level thrash pattern  
3. **Acceptance criteria** for harness / oracle / preview recovery  

---

## 1. Day rollup (msg_count > 0)

| Session (prefix) | End reason | Msgs | Tools | Input tokens | Min | Title (abbrev) |
|------------------|------------|------|-------|--------------|-----|----------------|
| `e256f862` | **interrupted** | 122 | 59 | 1.58M | 5 | 3D pinguin adventure `demos/pinguin` |
| `2a451053` | interrupted | 94 | 48 | 1.05M | 11 | Hyperframe video (retry) |
| `c439d268` | interrupted | 14 | 9 | 63k | 0 | Hyperframe video (abort) |
| `cb635449` | **budget_exhausted** | 193 | 97 | **4.27M** | 14 | Hyperframe video (main burn) |
| `c2a68dfa` | interrupted | 70 | 32 | 796k | 2 | Hyperframe video |
| `edf7455b` | interrupted | 116 | 57 | 1.52M | 4 | Hyperframe video |
| `41372f8f` | interrupted | 146 | 64 | 2.61M | 11 | Hyperframe video |
| `47f1e324` | model_returned_final_text | 96 | 54 | 1.61M | 9 | Profile presentation |
| `da8b08e4` | model_returned_final_text | 11 | 4 | 44k | 1 | PowerPoint |

**Pattern law:** visual / demo tasks dominate. **Healthy ends** (`model_returned_final_text`) are short or non-browser. **Visual_ux class** sessions end in interrupt or budget, with multi-million token burns and high browser_* tool share.

---

## 2. Session A — Pinguin 3D (latest)

### Coordinates

| Field | Value |
|-------|--------|
| **ID** | `e256f862-a457-4692-b058-3ab0ae245b7b` |
| **Model** | `copilot/kimi-k2.7-code` |
| **Source** | `cli` |
| **Wall clock** | 15:03:12 → 15:08:47 (~5.6 min) |
| **End** | `interrupted` |
| **Tokens** | in 1,582,053 · out 12,948 · cache_read 1,505,792 (**95.2%**) |
| **Goals** | none |
| **System prompt** | ~44 KB (stable ~5.8 KB · semi-stable ~3.8 KB) |

### Human intent (exactly one)

```text
Can you create a 3D pinguing adventure game in ./demos/pinguin html, javascript, threejs
```

### Role mix

| Role | N | Notes |
|------|---|--------|
| user | 5 | **1 human** + **4 harness/system** injections |
| assistant | 58 | **55/58 empty content** (pure tool ReAct) |
| tool | 59 | structured `⟦EDGECRAB:TOOL_RESULT…⟧` envelopes |

### Tool mix (assistant calls)

| Tool | N | Role in story |
|------|---|----------------|
| `terminal` | 23 | almost all `python3 -m http.server` restarts |
| `browser_navigate` | 18 | fail-ish cluster (Error response / blocked / unavailable) |
| `browser_snapshot` | 9 | chrome-error / empty / Error response |
| `browser_vision` | 4 | correctly saw **error page, not game** |
| `write_file` | **2** | **only product work** |
| `tool_search` | 3 | blocked as verification workaround once |

### Phase split (first principles)

```text
CREATE  (~82s)   mkdir → write index.html → write game.js
                 value delivered: demos/pinguin/* exists

VERIFY  (~200s)  harness forces preview + browser evidence
                 53+ tools of server restart + navigate thrash
                 no successful latched preview

END              harness-gates footer + human interrupt
                 oracle: node --check on game.js → exit 1 (ESM)
```

### Artifact law (disk)

| Path | Size | Quality signal |
|------|------|----------------|
| `demos/pinguin/index.html` | ~7 KB | HUD UI + **importmap** → unpkg Three 0.160 |
| `demos/pinguin/game.js` | ~20 KB | ESM `import * as THREE from 'three'` |

**Verdict on product:** create path **succeeded**.  
**Verdict on completion:** evidence loop **failed**.

### Harness injections (synthetic users)

1. **Mutation without verification** — stop shell debug; use browser for visual_ux  
2. **No session HTTP server** — force `python3 -m http.server 8000` + navigate + snapshot  
3. **Do not stop** — evidence still missing (completion assessor)  
4. **harness-gates** — `node --check` fail (ES module warning → exit 1)

### J1–J10 (session A)

| Job | Score | Note |
|-----|-------|------|
| J1 Intent | ✅ | Correct stack/path |
| J2 Safe act | ⚠️ | Preview on; port thrash until blocked |
| J3 Honest observe | ❌ | Vision said error page; loop did not pivot |
| J4 Budgets | ❌ | 1.6M tokens after value shipped |
| J5 Recover | ❌ | Blocks without latched recovery |
| J6 Compress | ✅ cache | 95% cache hit |
| J7 Persist mission | ❌ | No goals/subgoals |
| J8 Human steer | n/a | No mid-course human steer |
| J9 Surface | ✅ | CLI appropriate |
| J10 Extend | n/a | |

### AE1–AE10 (session A)

| AE | Result |
|----|--------|
| AE1 Bounded autonomy | Partial — interrupt, not clean hard-stop completion |
| AE2 Cross-window progress | **Absent** (no goals) |
| AE3 Completion = evidence | **On, path broken** |
| AE4 Tool truth | Envelopes good; model ignored repeats |
| AE5 Cache-stable prompts | **Strong** |
| AE6 Classify → recover | Over-trigger / dead-end |
| AE7 Mediated I/O | Preview mediation yes |
| AE8 Human sovereignty | Interrupt recorded |
| AE9 Observability | Full SQLite forensics possible |
| AE10 Extend | n/a |

### Causal chain (5 whys)

1. Not done → visual_ux + oracles demand evidence / syntax gate.  
2. No evidence → browser always saw Error response / chrome-error.  
3. Error page → serve/path race + thrash; `ok:true` navigate with **title Error response** treated as progress.  
4. Thrash continues → empty assistant prose, no strategy change; harness blocks ports then re-suggests same :8000 recipe.  
5. Still not done → wrong oracle (`node --check` on browser ESM) + interrupt.

---

## 3. Session B — Hyperframe video (`budget_exhausted`)

### Coordinates

| Field | Value |
|-------|--------|
| **ID** | `cb635449-7934-48cb-a532-e5559a08c4eb` |
| **Model** | `copilot/kimi-k2.7-code` |
| **Wall clock** | 11:03:48 → 11:18:09 (**~14.3 min**) |
| **End** | **`budget_exhausted`** |
| **Tokens** | in **4,272,266** · out 33,757 · cache_read 4,106,816 (**96.1%**) |
| **Msgs / tools** | 193 / 97 |
| **Goals** | none |

### Human intent

```text
Can you create a video (use Hyperframe), 15 seconds about Raphaël MANSUY
in demos/raphael_video
```

### Tool mix (assistant)

| Tool | N |
|------|---|
| `browser_vision` | **21** |
| `terminal` | 19 |
| `browser_navigate` | 19 |
| `write_file` | 17 |
| `read_file` | 6 |
| `patch` | 5 |
| other | skill_view, tool_search, snapshot, … |

### What differs from Pinguin

| Dimension | Pinguin A | Hyperframe B |
|-----------|-----------|--------------|
| End | interrupted | **budget_exhausted** |
| Create tools | 2 writes | **17 writes + 5 patches** (iterate composition) |
| Verify share | ~90% after create | high throughout (vision-heavy) |
| Terminal | only http.server thrash | `npm run check`, `npm run dev`, render, curl, server |
| Human turns | 1 | 1 (+ harness spam) |
| Cost shape | 1.6M / 5 min | **4.3M / 14 min** |

### Shared failure mode (law)

```text
visual_ux classification
  → demand browser evidence
  → preview/serve fragile
  → model re-mutates or re-navigates
  → harness “mutation without verification” fires again
  → still no latched success
  → budget_exhausted OR human interrupt
```

**AE3 without a latched success state becomes an expensive spinlock.**

Same day: **five** additional Hyperframe attempts ended `interrupted` (another ~6M+ input tokens cumulative). This is a **system** issue, not a single bad turn.

---

## 4. Harness / oracle code anchors

| Mechanism | Location | Observed behavior |
|-----------|----------|-------------------|
| visual_ux class + preview enable | `task_class.rs` · `conversation.rs` | Session-scoped loopback preview |
| Mutation-without-verify advisory | harness advisory path | Injected as **user** messages |
| Completion “do not stop” | `completion_assessor.rs` | Injected when evidence missing |
| Post-mutation oracles | `harness_gates.rs` · `display.harness_post_mutation_oracles` default **true** | `js` → `node --check` |
| Gate footer | `conversation.rs` harness-gates push | Session A end user msg |
| Config override | `EDGECRAB_HARNESS_ORACLES` · `config.rs` | Can disable oracles |

**Oracle bug (class mismatch):**  
`node --check game.js` on browser ESM with importmap:

```text
Warning: To load an ES module, set "type": "module" …
exit Some(1)
```

That gate is valid for **Node CJS/script** checks, **invalid** as sole completion oracle for **browser ESM demos**.

---

## 5. Acceptance criteria (implement these)

IDs are stable for tests / plan tickets.

### AC-V1 — Latched preview success (P0)

**Given** visual_ux task and a demo directory `D`  
**When** agent starts static preview successfully  
**Then** harness records `preview_latch = { dir: D, port: P, url, ok_at }`  
**And** further `http.server` restarts / alternate ports are **blocked** with message pointing at latch  
**And** browser tools are allowed only against latched URL until latch invalidated by write to `D`

### AC-V2 — HTTP body truth (P0)

**Given** `browser_navigate` returns `ok:true`  
**When** title or body matches error page (`Error response`, `chrome-error://`, 404)  
**Then** result is classified **content_fail** (not success)  
**And** assessor does **not** count it as visual verification  
**And** next advisory is diagnose (cwd, curl -I, single restart) not re-navigate

### AC-V3 — Class-aware oracles (P0)

**Given** mutated file is browser ESM under `demos/**` or has importmap peer HTML  
**When** post-mutation oracles run  
**Then** `node --check` is **skipped** or replaced by:

- `curl -sf` latched URL → 200 + HTML contains expected markers, **or**
- browser_snapshot node_count > 0 and not chrome-error, **or**
- optional: `node --check` only for `.cjs` / package `"type"` compatible Node entrypoints

**Test:** `game.js` with only `import` must **not** fail harness-gates solely via `node --check`.

### AC-V4 — Verify thrash circuit breaker (P0)

**Given** ≥ N failed browser_navigate/snapshot in M minutes for same latch (default N=3, M=2)  
**Then** harness injects **one** recovery recipe and sets `verify_blocked_until_human` or forces escalate  
**And** further browser_* calls return structured `thrash_blocked` without CDP spam  
**And** assistant is required to emit ≥1 non-empty progress sentence before more tools

### AC-V5 — Auto-goal for visual_ux (P1)

**Given** first user message classifies as visual_ux  
**Then** session goals auto-seed (unless goals disabled):

1. Scaffold / write artifacts  
2. Latch preview  
3. Browser evidence (snapshot or vision)  
4. Stop  

**And** goal block injects each turn (AE2) without mutating stable system prompt.

### AC-V6 — Mutation advisory rate limit (P1)

**Given** mutation-without-verification fires  
**Then** at most **one** injection per 90s  
**And** if agent is mid-verify (browser tools in last 30s), suppress “stop terminal” when terminal is only `http.server` start that matches latch recipe

### AC-V7 — Budget accounting for thrash (P1)

**Given** session ends `budget_exhausted` or `interrupted` after visual thrash  
**Then** harness_analyzer / session summary includes:

- `create_tool_count` vs `verify_tool_count`  
- `preview_latch_present`  
- `oracle_failures[]` with class  
- `cache_hit_ratio`

(So day rollups like §1 are one query away.)

### AC-V8 — Hyperframe / render tasks (P1)

**Given** intent mentions Hyperframe / render / video  
**Then** task class is **not** pure visual_ux-browser-only  
**Or** subclass `media_render` with evidence = **output file exists + size > 0 + optional ffprobe**  
**And** browser preview of composition HTML is optional, not the sole gate.

---

## 6. Suggested implementation order

```text
Week 1  AC-V2 body truth + AC-V3 class-aware oracles     (stop false fails / false success)
Week 1  AC-V1 preview latch + AC-V4 thrash breaker       (stop spinlock)
Week 2  AC-V5 auto-goal + AC-V6 advisory rate limit
Week 2  AC-V8 media_render class
Week 3  AC-V7 observability rollup
```

Primary code touchpoints (law):

| AC | Owner crates |
|----|----------------|
| V1–V4 | `edgecrab-core` (assessor, advisory) · `edgecrab-tools` (browser, harness_gates, terminal preview) |
| V3 | `edgecrab-tools/src/harness_gates.rs` |
| V5 | `edgecrab-core` goals + task_class |
| V7 | harness_analyzer / session_db metrics |
| V8 | `task_class.rs` + completion_assessor |

---

## 7. Regression tests to add

| ID | Test | Assert |
|----|------|--------|
| **T-V1** | latch blocks second port server | second start returns thrash_blocked |
| **T-V2** | navigate Error response ≠ visual evidence | assessor incomplete |
| **T-V3** | ESM demo skips node --check fail | gate green or alt oracle |
| **T-V4** | 3 failed navigates → breaker | 4th blocked |
| **T-V5** | visual_ux seeds 4 subgoals | session_subgoals count ≥ 3 |
| **T-V8** | “hyperframe render mp4” class | media_render or render evidence path |

```bash
# forensics repro (read-only)
sqlite3 ~/.edgecrab/profiles/homelab/state.db \
  "SELECT substr(id,1,8), end_reason, tool_call_count, input_tokens
   FROM sessions WHERE message_count>0
   ORDER BY started_at DESC LIMIT 10;"
```

---

## 8. One-screen verdict

| Session | Created value? | Closed loop? | Burn |
|---------|----------------|--------------|------|
| **Pinguin** `e256f862` | **Yes** (HTML+JS+Three) | **No** (preview thrash + bad oracle) | 1.6M in / 5 min |
| **Hyperframe** `cb635449` | Partial (many writes) | **No** (budget_exhausted) | **4.3M** in / 14 min |
| **Day pattern** | Repeated demo intent | Mostly interrupt/budget | **Systemic verify spinlock** |

**First principles fix:** completion-as-evidence is correct (AE3). Missing piece is a **latched, class-aware, thrash-bounded evidence state machine** — not more advisory text.

---

## 9. Cross-refs

| Doc | Role |
|-----|------|
| [001](001-first-principles.md) | J/AE rubric |
| [003](003-ai-engineer-harness-lens.md) | Loop physics |
| [014](014-improvement-plan.md) | Harness / MCP plan |
| `harness_gates.rs` | `node --check` oracle |
| `completion_assessor.rs` | visual_ux evidence |
| `task_class.rs` | visual_ux classification |

---

## 10. One-line summary

**2026-07-19 homelab sessions show EdgeCrab creates demos fast and caches prompts well, but visual_ux verification without preview latch + class-aware oracles burns millions of tokens and ends in interrupt or budget_exhausted — fix the evidence state machine (AC-V1…V8).**
