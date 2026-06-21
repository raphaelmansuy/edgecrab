# 008 — Improvement Plan (DRY · SOLID)

Phased delivery with **one owner module per change**. Each phase ends with acceptance gates in [009-acceptance-criteria.md](./009-acceptance-criteria.md).

**Battle-test anchor:** [005-evidence-games003.md](./005-evidence-games003.md) (sessions `927f4d85`, `e22c0a28`, `2e720f47`, **`0aeef965`**) · [012-brutal-assessment-jun2026.md](./012-brutal-assessment-jun2026.md) · [013-impact-ranked-backlog.md](./013-impact-ranked-backlog.md).

**Implementation legend:** ✅ shipped in `feat/minimum-context-007` branch · 🟡 partial · ⬜ not started

---

## First-principles diagnosis (session `2e720f47`, Jun 19)

```text
  Q1 What is happening?     STRONG — shelf, live tail, non-streaming label, ~29k ctx
  Q2 Is progress real?      WEAK — 42 tools, 0 perception success, terminal storm
  Q3 What work remains?     PARTIAL — manage_todo_list (12 tasks) but no verify tasks
  Q4 Why did it stop?       UNKNOWN mid-run — DB not flushed; operator sees "connecting"
  Q5 Was task done?         NO — no screenshot/preview; forked ultra files + markdown

  ROOT LOOP (broken):
    ACT (write, terminal) ──► blocked PERCEIVE ──► more ACT (docs, todos, execute_code)
```

**Harness must bias:** `INTENT → PLAN(with verify) → ACT → PERCEIVE → DONE` — not `ACT → ACT → ACT`.

---

## Priority overview

```text
  P0  Operator truth + perception loop     (games003 class failures)
  P1  Completion contract + observability   (ADR-001 + OTEL + Copilot latency)
  P2  Loop modularization + discovery       (maintainability + Hermes spill stack)
  P3  Strict verification mode              (optional product tier)
```

---

## P0 — Perception & operator truth (2–3 weeks)

### P0.1 Actionable spill stubs ✅

| | |
|---|---|
| **Problem** | Model gets useless stub; TUI shows `spilled` / `read ?` |
| **Change** | `SpillContext`, `source_path`, `next_read` in `build_conversation_stub()` |
| **Owner** | `crates/edgecrab-tools/src/artifact_spill.rs` |
| **Hermes** | `tool_result_storage._build_persisted_message` |
| **Gate** | HA-01, HA-02 |

### P0.2 Tool-call args cache for summaries 🟡

| | |
|---|---|
| **Problem** | Pruned history / non-streaming Copilot → `write ?`, `read ?` |
| **Change** | `turn_activity` + `response_dispatch` resolve args at `ToolDone` |
| **Owner** | `turn_activity.rs`, `response_dispatch.rs`, `tool_display.rs` |
| **Remaining** | All tool names + parallel dispatch edge cases |
| **Gate** | HA-03 |

### P0.3 Budget rejection → patch steering ✅

| | |
|---|---|
| **Problem** | Silent reject then retry loops |
| **Change** | `recommended_tools: ["patch"]` in budget recovery JSON |
| **Owner** | `recovery_catalog.rs` |
| **Gate** | HA-04 |

### P0.4 Dev preview profile ✅

| | |
|---|---|
| **Problem** | Visual tasks cannot verify (SSRF blocks localhost) |
| **Change** | `security.preview` + `PreviewPolicy` in `url_safety.rs`; `apply_security_runtime()` |
| **Owner** | `edgecrab-security`, `config.rs`, `browser.rs` |
| **Hermes** | Global allow-private (we use **port allowlist** — stricter) |
| **Ops** | homelab bundled profile seeds `security.preview` (existing installs: merge manually) |
| **Gate** | HA-05, HA-06 |

**homelab snippet (required for games003 dogfood):**

```yaml
security:
  preview:
    enabled: true
    allow_localhost_ports: [8000, 8888, 5173, 3000]
```

### P0.5 Wire/deferred operator display ✅

| | |
|---|---|
| **Problem** | "107 tools" vs 55 on wire confuses operators |
| **Change** | `/doctor` + `/context budget`; status bar `wire:N/def:M` chip (full + compact) |
| **Owner** | `doctor.rs`, `status_bar.rs`, `context_budget.rs` |
| **Gate** | HA-07 |

### P0.6 Perception failure → structured recovery (NEW)

| | |
|---|---|
| **Problem** | `browser_navigate` SSRF/scheme errors are opaque; model retries file:// |
| **Change** | On blocked localhost: inject **one-shot user message** with fix recipe (`security.preview` YAML snippet, correct URL shape, `browser_vision` after navigate) |
| **Owner** | `recovery_catalog.rs` + `conversation.rs` (post-tool-result steer) |
| **Hermes** | `preview.restart` prompt semantics |
| **Gate** | HA-16 |

### P0.7 Memory write limit recovery (NEW)

| | |
|---|---|
| **Problem** | `memory_write` fails at 2200 chars mid-task; agent has no prune path |
| **Change** | Error JSON: `used_chars`, `max_chars`, `suggested_actions: [prune_old, session_search]`; optional drift detect + `.bak` (Hermes `memory_tool._drift_error`) |
| **Owner** | `tools/memory.rs`, `recovery_catalog.rs` |
| **Gate** | HA-17 |

### P0.8 Terminal heredoc → write_file steering (NEW)

| | |
|---|---|
| **Problem** | `Shell heredocs are not supported` — model keeps trying `cat <<` |
| **Change** | `recovery_catalog` entry on heredoc rejection; terminal schema already warns — add **tool result** with `recommended_tools: ["write_file"]` |
| **Owner** | `command_interaction.rs`, `recovery_catalog.rs` |
| **Hermes** | `terminal_tool` schema + `file_tools` cross-reference |
| **Gate** | HA-18 |

### P0.9 Profile config inheritance ✅

| | |
|---|---|
| **Problem** | `-p homelab` loads profile `config.yaml` only; global `security.preview` ignored |
| **Change** | `load_from_with_global_inheritance`, `merge_global_inherited`, startup `migrate_profile_preview_from_global` |
| **Owner** | `config.rs`, `runtime.rs`, `profile.rs` |
| **Gate** | HA-41 |

### P0.10 Config recovery without path jail ✅

| | |
|---|---|
| **Problem** | Preview recovery tells model to edit `config.yaml`; path jail blocks read |
| **Change** | Recovery JSON + harness advisory: `/config set security.preview.enabled true`; forbid home config reads |
| **Owner** | `recovery_catalog.rs`, `harness_advisory.rs` |
| **Gate** | HA-42 |

### P0.11 VisualUx session preview + strict verify ✅

| | |
|---|---|
| **Problem** | Preview off → SSRF block; completion without browser evidence |
| **Change** | `apply_visual_ux_session_preview`, `effective_verification_strict`, markdown theater gate (HA-43) |
| **Owner** | `task_class.rs`, `completion_assessor.rs`, `conversation.rs`, `agent.rs` |
| **Gate** | HA-30, HA-43 |

---

## P1 — Completion contract & observability (2 weeks)

### P1.1 RunOutcome everywhere ✅

| | |
|---|---|
| **Change** | `turn_completion.rs` explainer; `completion_assessor` + strict mode hook |
| **Owner** | `completion_assessor.rs`, `turn_completion.rs` |
| **Gate** | HA-10..HA-12 |

### P1.2 TaskClassifier (advisory) ✅

| | |
|---|---|
| **Change** | `task_class.rs` → visual UX footer in `conversation.rs` |
| **Gate** | HA-13 |

### P1.3 OTEL fail-soft ✅

| | |
|---|---|
| **Change** | `collector_reachable_sync` skip OTLP init |
| **Gate** | HA-14 |

### P1.4 Copilot streaming recovery ✅

| | |
|---|---|
| **Change** | `copilot_tool_stream_locked` one-shot streaming; session downgrade flag |
| **Gate** | HA-15 |

### P1.5 Todo snapshot on compress (NEW — Hermes)

| | |
|---|---|
| **Problem** | After `/compress`, model forgets plan or re-does finished work |
| **Change** | Post-compress synthetic user msg: pending/in_progress todos only, capped (`MAX_TODO_ITEMS`, `MAX_TODO_CONTENT_CHARS`) |
| **Owner** | `compression.rs` + todo store |
| **Hermes** | `conversation_compression` todo_snapshot |
| **Gate** | HA-19 |

### P1.6 Partial-stream continuation prompts (NEW — Hermes)

| | |
|---|---|
| **Problem** | Stream interrupted / truncated tool args — generic recovery |
| **Change** | Branch `continuation_user_message` on: network stall vs length cap vs invalid JSON vs oversized args chunking steer |
| **Owner** | `conversation.rs`, `mutation_turn_policy.rs` |
| **Hermes** | `_get_continuation_prompt()` |
| **Gate** | HA-20b |

### P1.7 Dev server port discovery (NEW — Hermes preview.restart)

| | |
|---|---|
| **Problem** | `http.server` on 8000, agent navigates 8888 (E11) |
| **Change** | After `run_process` / terminal `http.server`, tool result or activity notice lists **listening ports** (`lsof`/`ss` parse); browser_navigate error cites detected servers |
| **Owner** | `process_table.rs`, `tools/browser.rs`, `doctor harness` |
| **Gate** | HA-20c |

### P1.8 Verify targets in task-class footer (NEW — Hermes coding_context)

| | |
|---|---|
| **Problem** | Visual/coding tasks lack concrete verify commands |
| **Change** | `TaskClassifier` adds footer lines: for `VisualUx` → preview URL recipe; for `CodeChange` → `npm test`/`node -c` from manifest scan |
| **Owner** | `task_class.rs`, `prompt_builder` (minimal) |
| **Hermes** | `_VERIFY_TARGETS` |
| **Gate** | HA-20d |

### P1.9 Iteration storm advisory (NEW)

| | |
|---|---|
| **Problem** | 6× terminal in 18s with no perception (E9) |
| **Change** | Harness WARN + optional user-message when ≥N same-class tools without perception tool in window |
| **Owner** | `conversation.rs` or `stream_observability.rs` |
| **Gate** | HA-20e (metrics / log-based) |

---

## P2 — Maintainability & discovery (3 weeks)

### P2.1 conversation.rs extraction 🟡

| | |
|---|---|
| **Change** | `provider_call.rs` owns streaming/retry/Hermes API hooks; `turn_dispatch.rs` owns tool-turn finalization; tests migrated |
| **Remaining** | Further slim `execute_loop` — compression notices, continuation prompts |
| **Rule** | Move-only PR first |
| **Gate** | HA-20 |

### P2.2 ProgressSink trait ✅

| | |
|---|---|
| **Change** | `progress_sink.rs` + `StreamEventSink` |
| **Gate** | HA-21 |

### P2.3 Discovery hints for search_files ✅

| | |
|---|---|
| **Change** | Zero-hit `discovery_hint` |
| **Gate** | HA-22 |

### P2.4 Spill-aware read_file default ⬜

| | |
|---|---|
| **Change** | Inline first 80 lines + artifact path on spill |
| **Gate** | HA-23 |

### P2.5 Turn budget layer (NEW — Hermes)

| | |
|---|---|
| **Problem** | Single huge tool output blows turn; spill per-call but no aggregate steer |
| **Change** | `enforce_turn_budget()` — 200k char turn cap; per-tool thresholds; pin `read_file` threshold ∞ |
| **Owner** | `artifact_spill.rs`, `read_tracker.rs`, config `tools.result_turn_budget_chars` |
| **Hermes** | `tool_result_storage.enforce_turn_budget` |
| **Gate** | HA-24 |

### P2.6 `edgecrab doctor harness` (NEW)

| | |
|---|---|
| **Change** | Analyze `harness.jsonl`: spill-without-read, perception gap, OTEL noise, last `exit_reason` |
| **Owner** | `edgecrab-cli/doctor.rs` |
| **Gate** | HA-25 |

### P2.7 Background completion notify ⬜

| | |
|---|---|
| **Change** | `notify_on_complete` for `run_process` — shelf event when http.server ready |
| **Hermes** | `process_registry.notify_on_complete` |
| **Gate** | HA-26 |

### P2.8 games003 replay test ⬜

| | |
|---|---|
| **Change** | `harness_games003_replay.rs` from session logs |
| **Gate** | HA-27 |

---

## P3 — Strict verification (optional)

| | |
|---|---|
| **Change** | `harness.verification_strict` blocks `Completed` without evidence |
| **Owner** | `completion_assessor.rs` |
| **Status** | ✅ code (`HA-30` test) |
| **Gate** | HA-30 |

---

## Dependency graph

```text
  P0.1 spill stub ──┬──► P0.4 preview (model can read artifact paths)
  P0.2 args cache   │
  P0.3 budget hint  │
  P0.6 perceive recovery ──► P1.7 port hint
  P0.5 wire display │
                    │
  P1.1 RunOutcome ──┴──► P1.2 TaskClassifier ──► P1.8 verify targets ──► P3 strict
  P1.3 OTEL
  P1.4 streaming
  P1.5 todo compress
  P1.6 continuation prompts

  P2.5 turn budget ──► P2.4 inline spill read
  P2.6 doctor harness ──► dogfood metrics

  P2.* parallel after P0 stable
```

---

## What we explicitly defer

| Idea | Defer reason |
|------|--------------|
| New "done" tool | CompletionPolicy sufficient (ADR-001) |
| Disable SSRF globally | Security regression |
| Raise 27k budget for cloud | Breaks local geometry law |
| Auto-run browser for all tasks | Cost + safety; task-class only |
| Auto-read spill into context | Turn budget (battle test S22) |
| Hermes global `allow_private_urls` | Port allowlist is safer |

---

## Success metrics (90 days post-P1)

| Metric | Baseline (games003) | Target |
|--------|---------------------|--------|
| Spill stub → artifact read within 2 turns | ~0% | >80% in dogfood |
| write_file budget rejections per visual task | 2+ | 0 (patch used) |
| Operator "stuck" reports on Copilot tool turns | frequent | <10% sessions |
| OTEL ERROR lines/min (no collector) | ~12 | 0 |
| RunOutcome shown on interrupt | no | yes |
| Successful preview/vision per visual session | **0/3 sessions** | >50% |
| memory_write cap failures without recovery hint | 1 (2e720f47) | 0 |
| Markdown report files per visual task | 5+ | ≤1 (operator constraint) |

---

## Immediate operator actions (homelab)

1. Add `security.preview` to `profiles/homelab/config.yaml` (P0.4 ops).
2. Steer: "Preview at http://127.0.0.1:8000/index-ultra.html — no more README files."
3. Run `/doctor harness` once P2.6 ships; until then tail `profiles/homelab/logs/harness.jsonl`.
