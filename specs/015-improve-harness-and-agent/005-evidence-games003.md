# 005 — Evidence: games003 Sessions (Battle Test)

Quantitative record used to validate spec claims. Sessions stored in `~/.edgecrab/profiles/homelab/state.db`.

---

## Session metadata

| Field | `927f4d85` (Jun 18) | `e22c0a28` (Jun 19) |
|-------|---------------------|---------------------|
| Title | Improve game demo/games003… | (empty — not flushed) |
| End reason | `interrupted` | (in flight at capture) |
| Message count | 31 | 0 in DB at capture |
| Tool calls | 16 | — |
| Model | `copilot/claude-haiku-4.5` | same |
| Harness `tool_count` | 55 on wire | 55 |

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

## Spill artifacts (workspace)

Path: `edgecrab/.edgecrab-artifacts/{session_id}/`

| Session | Files | Sizes |
|---------|-------|-------|
| `927f4d85` | `read_file_001.md`, `read_file_002.md` | 24k, 29k |
| `e22c0a28` | `read_file_001.md`, `read_file_002.md` | 20k, 23k |

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

**Not improved:** primary `index.html` in session 2 path (agent forked new files).

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

---

## Lessons encoded in plan

| Evidence | Plan item |
|----------|-----------|
| E1 | P1 discovery index / path hints in errors |
| E2 | P0 spill stub includes **actionable** read recipe |
| E3 | P0 pre-flight **patch guidance** in budget error |
| E4 | P0 dev preview profile |
| E5 | P1 OTEL fail-soft |
| E6 | P0 summary retains path from tool_call args cache |

See [009-acceptance-criteria.md](./009-acceptance-criteria.md) HA-01..HA-06.
