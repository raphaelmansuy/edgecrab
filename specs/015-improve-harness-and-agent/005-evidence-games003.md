# 005 — Evidence: games003 Sessions (Battle Test)

Quantitative record used to validate spec claims. Sessions stored in `~/.edgecrab/profiles/homelab/state.db`.

---

## Session metadata

| Field | `927f4d85` (Jun 18) | `e22c0a28` (Jun 19) | `2e720f47` (Jun 19) | `0aeef965` (Jun 19, race_gamey) |
|-------|---------------------|---------------------|---------------------|--------------------------------|
| Title | Improve game demo/games003… | (empty — not flushed) | (empty — in flight) | 3D race car demo/race_gamey |
| End reason | `interrupted` | (in flight at capture) | (in flight, api≥43) | in flight (api=59) |
| Message count | 31 | 0 in DB at capture | 0 in DB (not flushed) | 0 in DB (not flushed) |
| Tool calls | 16 | — | **42+** tool starts | **59** starts (118 events) |
| Tool errors | 2 (browser) | 2 (write reject) | 3+ | **6** |
| Model | `copilot/claude-haiku-4.5` | same | same | same |
| Harness `tool_count` | 55 on wire | 55 | 15 on wire (indexed) | 16 (indexed) |
| Context | — | — | ~29k/128k at capture | ~20k/128k |
| Preview active | OFF (profile) | OFF | OFF | **OFF** (global ON ignored) |

---

## Tool timeline (`927f4d85`)

```text
  iter  tool(s)                    duration   outcome
  ────  ─────────────────────────  ────────   ──────────────────────────
  0     search_files ×2            ~66ms      0 matches (games003)
  1     terminal find              2045ms     found demo/games003
  2     read_file (directory)      0ms        ERROR cannot read dir
  3     read_file ×3               0–2ms      game.html ok; 2× SPILLED
  4     write_file game.html       4481ms     ok 16371 B
  5     write_file game.js         3519ms     ok 21291 B
  6     browser_navigate file://   1ms        permission denied
  7     run_process http.server    2ms        ok proc-1
  8     browser_navigate localhost 0ms        SSRF blocked
  9–11  terminal ls/node/grep      9–65ms     syntax checks only
  12    write_file ULTIMATE-*.md   1ms        ok 9492 B
  →     interrupted
```

---

## Tool timeline (`e22c0a28`) — harness.jsonl

```text
  iter  tool(s)                    notes
  ────  ─────────────────────────  ─────────────────────────────────────
  0–2   terminal find/ls           discovery
  3–5   read_file ×3               spills in TUI
  6     write_file REJECTED        28077 B > 27852 B max
  7     write_file                 ok (~3s) → game-beautiful.html
  8     read_file                  verify
  9     write_file REJECTED        30546 B > max
  10+   write_file                 game-beautiful.js (~2.3s)
  →     composing tool call 32s+   iteration 9+ non-streaming
```

---

## Tool timeline (`0aeef965`) — harness.jsonl (race_gamey)

**Profile:** `homelab` · **CWD:** `edgecrab` repo · **Task:** 3D race car HTML/JS in `demo/race_gamey`.

```text
  phase          tool(s)                         outcome / note
  ─────────────  ───────────────────────────────  ─────────────────────────────────────
  write          write_file×2                     index.html + game.js (~16k/20k)
  serve          terminal×2                       http.server; harness dev hint present
  perceive FAIL  tool_search, browser_navigate×2  SSRF blocked — profile preview OFF
  config spiral  read_file(config.yaml)           path jail — outside workspace
  storm          terminal×15+ in ~5min            ls/cat/grep; duplicate browser blocked
  theater        write_file×5+                    README, DELIVERY, VERIFICATION, FINAL_REPORT
  →              api_call_count 59                zero browser_snapshot / vision
```

**Failure signatures new in this session:**

| ID | Signature | Detection |
|----|-----------|-----------|
| E12 | Agent reads `~/.edgecrab/config.yaml` after preview block | read_file error outside allowed roots |
| E13 | Recovery suggests config edit agent cannot perform | terminal cat/grep on home config |
| E14 | Markdown verify theater (`VERIFICATION.md`, `FINAL_REPORT.txt`) | ≥3 report files, 0 perception tools success |
| E15 | Global `security.preview` ignored under `-p homelab` | doctor: global ON, profile missing preview key |

See [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md) § R1–R3.

---

## Tool timeline (`2e720f47`) — harness.jsonl (extended battle test)

**Profile:** `homelab` · **CWD:** `edgecrab` repo · **Task class:** visual UX (ultra/beautiful edition fork).

```text
  phase          tool(s)                         outcome / note
  ─────────────  ───────────────────────────────  ─────────────────────────────────────
  discovery      search_files, terminal×2         find demo/games003
  read           read_file×3                      spills → stub only in loop
  write          write_file×2                     index-ultra.html + game-ultra.js (~26k/22k)
  perceive FAIL  tool_search, browser_navigate×2  file:// scheme + localhost SSRF (no preview config)
  theater        terminal×3                       ls, node -c, grep — not UX
  docs spiral    write_file×5+                    ULTRA-EDITION-README, ENHANCEMENT-*, etc.
  plan           manage_todo_list                 12 tasks set — no verify gate tied to todos
  infra          terminal (pkill python)          cleanup old http.server
  memory FAIL    memory_write                     MEMORY.md 2200-char cap exceeded
  shell spam     terminal×6 in ~18s               rapid cd/ls — iteration churn
  pivot          tool_search, execute_code        escape to sandbox after browser blocked
  env mismatch   http.server on **8000**         agent navigated **8888** (wrong port)
```

**Failure signatures new in this session:**

| ID | Signature | Detection |
|----|-----------|-----------|
| E7 | `memory_write` cap hit mid-task | error contains `would exceed 2200-char limit` |
| E8 | Todo plan without verify steps | `manage_todo_list` tasks lack preview/vision item |
| E9 | Terminal iteration storm | ≥5 terminal calls without perception tool in 60s |
| E10 | `execute_code` after perception block | model pivots to sandbox instead of enabling preview |
| E11 | Wrong localhost port | navigate port ≠ listening `lsof`/`ps` port |

---

## Spill artifacts (workspace)

Path: `edgecrab/.edgecrab-artifacts/{session_id}/`

| Session | Files | Sizes |
|---------|-------|-------|
| `927f4d85` | `read_file_001.md`, `read_file_002.md` | 24k, 29k |
| `e22c0a28` | `read_file_001.md`, `read_file_002.md` | 20k, 23k |
| `2e720f47` | `read_file_001.md` | ~20k |

Content: spilled `index.html` / `game.js` bodies — model saw stubs in loop.

---

## File system outcome (`demo/games003/`)

**Before agent:** `index.html` (31k), `game.js`, multiple prior enhancement docs.

**After agents:**

| File | Agent action |
|------|----------------|
| `game.html` | overwritten (session 1) |
| `game.js` | overwritten (session 1) |
| `ULTIMATE-ENHANCEMENT-GUIDE.md` | created (session 1) |
| `game-beautiful.html` | created (session 2) |
| `game-beautiful.js` | created (session 2) |
| `index-ultra.html` | created (session 3 `2e720f47`) |
| `game-ultra.js` | created (session 3) |
| `ULTRA-EDITION-README.md` etc. | many markdown reports (session 3) |

**Not improved:** primary `index.html` in session 2–3 paths (agent forked new files). **Zero** successful browser preview in any session.

---

## Failure signatures (for regression tests)

| ID | Signature | Detection |
|----|-----------|-----------|
| E1 | `search_files` 0 hits for existing dir name | tool result `total=0` when path exists |
| E2 | spill without follow-up artifact read | stub in messages, no subsequent read on artifact path |
| E3 | `rejecting tool call` write_file budget | WARN log + recovery error in tool_results |
| E4 | browser localhost blocked for visual task | permission + SSRF errors, no vision fallback |
| E5 | OTEL tcp connect spam | ERROR count > 3/min with no collector |
| E6 | TUI `read ?` on completed read | summary missing path |
| E7 | memory cap mid-run | `memory_write` 2200 limit |
| E8 | todo without verify | manage_todo_list only |
| E9 | terminal storm | perception gap |
| E10 | execute_code pivot | sandbox escape |
| E11 | wrong preview port | 8888 vs 8000 |
| E12 | config read after browser block | path outside workspace |
| E13 | unfixable config recovery loop | cat/grep home config fails |
| E14 | markdown verify theater | report files without perception |
| E15 | profile ignores global preview | `-p homelab` loads profile-only config |

---

## Lessons encoded in plan

| Evidence | Plan item |
|----------|-----------|
| E1 | P2.3 discovery hints |
| E2 | P0.1 spill stub includes **actionable** read recipe |
| E3 | P0.3 pre-flight **patch guidance** in budget error |
| E4 | P0.4 dev preview profile |
| E5 | P1.3 OTEL fail-soft |
| E6 | P0.2 summary retains path from tool_call args cache |
| E7 | P0.7 memory limit recovery |
| E8 | P1.5 todo + verify coupling |
| E9 | P1.9 iteration storm guard (advisory) |
| E10 | P1.7 dev server port hint |
| E11 | P1.7 port discovery |
| E12–E15 | P0.10 profile merge · P0.11 config recovery · [013 backlog rank 1–4](./013-impact-ranked-backlog.md) |

See [009-acceptance-criteria.md](./009-acceptance-criteria.md) HA-01..HA-43 · [011-hermes-parity-map.md](./011-hermes-parity-map.md) · [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md).
