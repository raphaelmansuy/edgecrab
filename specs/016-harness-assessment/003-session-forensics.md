# 003 — Session Forensics

**Date:** 2026-06-19  
**Sources:** `harness.jsonl`, `state.db`, workspace `demo/*`  
**Cross-ref:** [015/005](../015-improve-harness-and-agent/005-evidence-games003.md) · [006-verdict](./006-brutal-verdict.md)

---

## Aggregate metrics (`harness.jsonl`, 973 lines)

```text
  Sessions instrumented:     7
  Total tool starts:         322
  Tool mix (top 5):
    terminal .............. 158  (49%)
    write_file ............  71  (22%)
    browser_navigate ......  22  ( 7%)
    read_file .............  17  ( 5%)
    tool_search ...........  11  ( 3%)

  browser_navigate outcomes:  18 OK / 18 FAIL  (50% — cron web session drives OK)
  CLI visual sessions:      0 OK / ~16 FAIL   (all homelab visual tasks)

  Turn decisions:
    interrupted ........... 4
    completed ............. 2  ← e22c0a28 visual, cron web
    budget_exhausted ...... 1
```

**Brutal read:** Half of all tool calls are `terminal`. The harness optimizes for **shell forensics**, not **perception**.

---

## Session table (`state.db`)

| ID | Title | msgs | tools | end_reason | Perception OK? |
|----|-------|------|-------|------------|----------------|
| `d4d6b6b4` | (in flight) | 0 | 0 | — | **0** (3 navigate, no snapshot) |
| `0aeef965` | race_gamey 3D | 150 | 72 | interrupted | **0** |
| `07ffeba0` | 3D racing HTML | 189 | 88 | budget_exhausted | **0** (6 navigate, all fail) |
| `2e720f47` | games003 beautify | 110 | 52 | interrupted | **0** |
| `e22c0a28` | games003 beautify | 53 | 25 | **model_returned_final_text** | **0** |
| `927f4d85` | games003 beautify | 31 | 16 | interrupted | **0** |
| `b84c7ba4` | games003 (Jun 17) | 75 | 38 | **model_returned_final_text** | **0** |

Two sessions marked **completed** in SQLite with **zero browser success** on visual tasks.

---

## Forensic timeline: `0aeef965` (race_gamey)

```text
  write×2 (index.html, game.js)
    → terminal×2 (http.server)
    → browser_navigate×2 FAIL (SSRF — preview OFF)
    → read_file(~/.edgecrab/config.yaml) FAIL (path jail)
    → terminal×52 (ls/cat/grep storm)
    → write×12 (README, DELIVERY, VERIFICATION, FINAL_REPORT…)
    → api_call_count 72 → interrupted
```

**Workspace after:** `demo/race_gamey/` — game files + **6 markdown/txt "verify" files**, no screenshot artifact.

---

## Forensic timeline: `d4d6b6b4` (race_game_z, Jun 19 post-fix)

Session started **after** spec 015 P0 implementation landed. Pattern **unchanged**:

```text
  read_file → terminal×16 → write×10
    → browser_navigate×3 (no logged success)
    → report_task_status
    → more terminal / write spiral
    → api_call_count 37+ (in flight at capture)
```

**Workspace after:** `demo/race_game_z/` — `index.html`, `game.js` **plus** `FINAL_REPORT.md`, `VERIFICATION_EVIDENCE.md`, `headless_verify.js`, `screenshot.js` (theater scripts, not browser artifacts).

---

## Forensic timeline: `e22c0a28` (false completed)

```text
  terminal discovery → read×4 (spills) → write×8
    → NO browser_navigate attempts
    → report_task_status
    → terminal×9
    → harness: turn complete → decision=completed
```

DB agrees: `end_reason=model_returned_final_text`. **Strict VisualUx verification did not fire** (session predates HA-43 enforcement or task class not classified).

---

## Cron contrast: `cron-234…`

Same log file, **web** task (not visual UX):

```text
  browser_navigate×5 → 3+ SUCCESS (7s, 2s durations)
  → web_extract, browser_scroll, browser_wait_for
  → decision=completed
```

**Interpretation:** Browser **works** when preview policy is active in that runtime context. Visual CLI sessions fail because **homelab profile loads with preview OFF** at process start (see config forensics below).

---

## Config forensics

```text
  ~/.edgecrab/config.yaml
    security.preview.enabled: true
    ports: [8000, 8765, 8888, 5173, 3000]

  ~/.edgecrab/profiles/homelab/config.yaml
    security: (present)
    security.preview: ABSENT  ← serde default enabled=false

  edgecrab doctor harness (2026-06-19):
    ⚠ security.preview disabled
    Harness log analysis: tool starts: 0  ← JSONL parser bug
```

HA-41 `merge_global_inherited` exists in `config.rs` and passes unit test `ha41_profile_inherits_global_preview_when_omitted`. **Production doctor still reports disabled** — inheritance path or global load is not effective for operator diagnostics. Profile file **never migrated** on disk.

---

## Failure signature registry (for CI replay)

| ID | Signature | Sessions |
|----|-----------|----------|
| E1–E6 | spill, budget, OTEL (015) | multiple |
| E7 | memory_write 2200 cap | `2e720f47` |
| E8 | todo without verify step | `2e720f47` |
| E9 | terminal storm ≥5/60s | `0aeef965`, `07ffeba0`, `d4d6b6b4` |
| E10 | execute_code pivot | `2e720f47` |
| E11 | wrong preview port 8888 vs 8000 | `2e720f47` |
| E12–E15 | config jail, profile drift | `0aeef965` |
| **E16** | **completed without perception** | **`e22c0a28`, `b84c7ba4`** |
| **E17** | **post-fix theater scripts** | **`d4d6b6b4`** (`headless_verify.js`) |
| **E18** | **doctor harness 0 metrics on JSONL** | tooling |

---

## Spill follow-up rate

| Metric | Value |
|--------|-------|
| Spill events in `agent.log` | 6 |
| `read_file` after spill in harness log | ~0 for visual sessions |
| Agent behavior | writes **new** files (`game-beautiful.*`, `index-ultra.*`) instead of patching spilled source |

HA-01/02 unit tests pass; **behavior does not**.

---

## 90-day metrics (re-baseline)

| Metric | 015 baseline (4 sess) | 016 (7 sess) | Target |
|--------|----------------------|--------------|--------|
| CLI visual preview success | 0/4 | **0/6** | >50% |
| False `completed` (visual) | 1? | **2** | 0 |
| Profile preview on disk | false | **false** | true |
| Doctor harness tool_starts | N/A | **0 (bug)** | match JSONL |
| Terminal share of tools | ~45% | **49%** | <25% on visual |
