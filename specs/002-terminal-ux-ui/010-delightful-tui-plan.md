# 010 — Delightful TUI Plan (First Principles + Hermes Best Ideas)

**Goal:** Close the polish gap with Hermes while **keeping EdgeCrab’s liveness advantage** — an elegant, calm, delightful ratatui experience where users never feel stuck and rarely feel overwhelmed.

**Inputs:** [005-honest-assessment.md](005-honest-assessment.md) · [009-hermes-comparison.md](009-hermes-comparison.md) · [007-implementation-roadmap.md](007-implementation-roadmap.md) (P0–P3b done)

**Non-goals:** Full terminal emulator mirror; Node/Ink rewrite; duplicating Hermes desktop app / web PTY.

---

## 1. First principles

An agent TUI is not a chat app with extras. It is a **real-time observability surface** for an asynchronous worker the user cannot see directly. Delight emerges when these invariants hold:

| Principle | User question answered | Failure mode |
|-----------|------------------------|--------------|
| **Certainty** | “What is happening *right now*?” | Blank scroll area, frozen spinner |
| **Continuity** | “Did the same task update, or did something new start?” | Layout jump, duplicate lines, lost context |
| **Proportionality** | “Do I get the right amount of detail?” | Wall of logs OR opaque black box |
| **Agency** | “Can I steer, stop, or approve?” | Trapped modal, silent block |
| **Calm** | “Is the UI shouting at me?” | 60fps log flood, flickering status |
| **Personality** | “Does this feel alive, not broken?” | Clinical spinner with no rhythm |

### Derived design law

> **Separate “what is live” from “what is history.”**  
> Transcript = durable narrative. Activity shelf = ephemeral live state. Status bar = at-a-glance compass.

Hermes implements this with Ink’s **thinking shelf** + turn controller. EdgeCrab today merges live state into scrollback placeholders — good for continuity, weak for **parallel work** and **reasoning + tools** in one glance.

### EdgeCrab invariant (do not violate)

> **Liveness beats lifecycle-only.**  
> Foreground tool stdout tail (`ToolProgress`, last 3 lines @ ≤5/sec) stays. Hermes’s lifecycle-only model is explicitly *not* the target.

---

## 2. Target experience (north star)

```text
┌─ Transcript (history) ─────────────────────────────────────────────┐
│ User: refactor auth module                                        │
│ Assistant: I'll run tests first…                                  │
│ ✓ terminal  cargo test  (12.4s)  42 passed                        │
│ …                                                                 │
├─ Activity Shelf (live, 0–4 lines) ────────────────────────────────│  ← NEW
│ ⠹ thinking · planning next step (4s)                              │
│   ├─ 💻 terminal  $ cargo build  · Compiling edgecrab… (18s)      │
│   └─ 📖 read_file  src/auth.rs  (done)                            │
│ 📟 p-7 · npm run dev  · ready on :3000                            │
├─ Status bar ──────────────────────────────────────────────────────│
│ EC · claude-opus · ▰▰▱▱ 62% · $0.04 · ^C stop · ⛵ 1 pending    │
├─ Prompt ──────────────────────────────────────────────────────────│
└───────────────────────────────────────────────────────────────────┘
```

**Behaviors:**
- Shelf updates **in place** (same ratatui discipline as tool placeholders).
- Long tools get **gentle nudge** after 8s (Hermes charms), not alarm.
- **`tool.generating`** shows “preparing `{tool}`…” before first `ToolExec`.
- **`/tail p-7`** opens overlay with rolling buffer (Hermes `process.list` parity).
- Verbose-off: shelf + status bar stay live; transcript stays quiet.

---

## 3. Architecture (SOLID)

### 3.1 New modules (extract from `app.rs`)

| Module | Responsibility | Hermes analogue |
|--------|----------------|-----------------|
| `activity_shelf.rs` | Render live shelf; merge active tools, bg procs, thinking | `thinking.tsx` |
| `turn_activity.rs` | Turn-scoped state: active tools map, bg lines, phase, batch coalescing | `turnController.ts` |
| `live_progress.rs` | 16ms coalesce window for shelf text (not StreamEvent rate) | `STREAM_BATCH_MS` |
| `process_tail_panel.rs` | `/tail` overlay; reads `ProcessTable` | `process.list` RPC |

**Keep:** `tool_progress_tail.rs` as the **tool→agent** progress layer. Shelf consumes **StreamEvents**, not tool internals (DIP).

### 3.2 Progressive disclosure ladder

| Level | Surface | Content | Trigger |
|-------|---------|---------|---------|
| L0 | Status bar | One-line summary + elapsed | Always |
| L1 | Activity shelf | Active tools (≤3 visible), phase, bg headline | `is_processing` |
| L2 | Transcript placeholder | Per-tool in-place line (current) | `ToolProgressMode` policy |
| L3 | Expand (Ctrl+Shift+T) | Full tool result body | User action |
| L4 | `/tail <id>` panel | Up to 4KB rolling output | User action |

**Rule:** Events update L0+L1 always; L2 follows `/verbose`; L3/L4 on demand.

### 3.3 Event contract additions

| Event | New? | Purpose |
|-------|------|---------|
| `ToolGenerating { name, partial_args }` | **Yes** | Hermes `tool.generating` — model drafting tool call |
| `ToolProgress` | Existing | Mid-run tail/milestones — **keep** |
| `ActivityNotice` | Existing | Compression, approval, watch match |
| `BackgroundProcessTail` | Existing | Shelf + transcript monitor line |
| `ShelfHint { kind, text }` | Optional | Long-run charm, first-30s onboarding |

Implement `ToolGenerating` in `conversation.rs` when aggregating `ToolCallDelta` (before dispatch).

### 3.4 Data flow (unchanged core, new consumer)

```text
Tools → tool_progress_tail → ToolProgress → StreamEvent
                                              ↓
                                    turn_activity (coalesce)
                                              ↓
                         ┌────────────────────┴────────────────────┐
                         ▼                                         ▼
                  activity_shelf                              status bar
                  transcript placeholders (policy)
```

---

## 4. Hermes ideas → EdgeCrab adoption map

| Hermes idea | Adopt? | EdgeCrab interpretation |
|-------------|--------|---------------------------|
| Thinking shelf | **Yes** | `activity_shelf.rs` — 2–4 lines between output and status |
| Active tools list | **Yes** | Shelf shows all in-flight tools, not only “latest” |
| `tool.generating` | **Yes** | `StreamEvent::ToolGenerating` |
| Long-run charms (8s) | **Yes** | One-line shelf hint; skin-configurable copy |
| Braille spinner | **Partial** | Already have spinner frames; unify shelf + status animation |
| `process.list` 4KB tail | **Yes** | `/tail` panel + ProcessTable read API |
| Gateway 500-char bg push | **Yes** | Raise bg tail budget in `event_processor` (configurable) |
| 16ms UI batch | **Yes** | Coalesce shelf redraws, not agent events |
| Ink sub-agent tree | **Phase 2** | Indent tree in shelf (`active_subagents` already tracked) |
| Reasoning in shelf | **Yes** | Last reasoning line when `/reasoning hide` |
| Per-platform tool_progress defaults | **Gateway only** | Port tier table from `display_config.py` |
| Lifecycle-only terminal | **No** | Keep live tail |

---

## 5. Phased implementation

### Phase D1 — Activity Shelf foundation (2–3 weeks)

**Outcome:** Live work visible in one zone; status bar decluttered.

| Task | Touch points | Acceptance |
|------|--------------|------------|
| Extract `TurnActivity` state | New `turn_activity.rs`; slim `app.rs` | Unit tests for merge/coalesce |
| Shelf layout region | `app.rs` layout split: output / shelf / status / prompt | Shelf visible when processing; hidden when idle |
| Wire shelf to existing events | `ToolExec`, `ToolProgress`, `ToolDone`, `BackgroundProcessTail`, `ActivityNotice` | Parallel tools list ≥2 entries; tail text on terminal tool |
| Config toggle | `display.activity_shelf: true` in `config.yaml` | `/config` or `/statusbar` documents toggle |
| Compact / BasicCompat | Shelf collapses to 1 line <60 cols | Termux profile tested |

**Grade target:** UI chrome C+ → **B**.

---

### Phase D2 — Pre-tool & long-run delight (1 week)

**Outcome:** No dead air between “model stopped typing” and `ToolExec`.

| Task | Touch points | Acceptance |
|------|--------------|------------|
| `StreamEvent::ToolGenerating` | `conversation.rs` ToolCallDelta aggregation; `agent.rs` enum | Shelf shows “preparing terminal…” during JSON stream |
| Long-run hint | `turn_activity.rs` + `skin.yaml` `long_run_hints[]` | After 8s on same tool, one subtle shelf line (max 2/turn) |
| First-run onboarding | Reuse existing onboarding flags | 30s tool in `all` mode → one-time shelf tip |

**Hermes refs:** `tool.generating`, `useLongRunToolCharms.ts`.

**Grade target:** liveness B+ → **A−** (perceived).

---

### Phase D3 — Background depth (1–2 weeks)

**Outcome:** Background servers feel monitored, not forgotten.

| Task | Touch points | Acceptance |
|------|--------------|------------|
| `/tail [process_id]` command | `commands.rs`, `process_tail_panel.rs` | Overlay shows last 4KB; Esc closes; read-only |
| Shelf bg headline | Extend `bg_process_lines` → shelf entry | One bg proc in shelf without extra transcript line |
| Larger tail budget (TUI) | `tool_progress_tail.rs` `BG_SHELF_TAIL_CHARS = 512` | Configurable; still throttled |
| Gateway bg push | `event_processor.rs` + config `gateway.bg_tail_chars: 500` | Telegram gets running updates like Hermes |

**Hermes refs:** `process.list`, `_run_process_watcher`.

**Grade target:** Background visibility B → **A−**.

---

### Phase D4 — Reasoning & delegation shelf (1–2 weeks)

**Outcome:** Think mode and sub-agents readable without transcript spam.

| Task | Touch points | Acceptance |
|------|--------------|------------|
| Reasoning shelf line | When reasoning hidden, last 80 chars in shelf | `/reasoning hide` still informative |
| Sub-agent tree in shelf | `active_subagents` + indent | `[2/3] migrate handlers` under parent turn |
| Sub-agent tool churn | Optional `verbose_subagent_tools` config | Default: tool name only in shelf |

**Hermes refs:** `thinking.tsx`, sub-agent payload fields.

**Grade target:** Reasoning visibility B → **B+**.

---

### Phase D5 — Polish & calm (ongoing, 1 week hardening)

| Task | Touch points | Acceptance |
|------|--------------|------------|
| 16ms shelf coalesce | `live_progress.rs` | No >5 shelf redraws/sec under flood |
| Reduced motion | Shelf static text; spinner off | Respects `animate_status_indicators` |
| Skin tokens for shelf | `skin_engine.rs`: `shelf_border`, `shelf_dim`, `shelf_accent` | Hermes-compatible YAML keys |
| Parallel status bar | `summarize_active_tools` → “3 tools · terminal +2” | Matches shelf headline |
| SDK export | `ToolGenerating`, shelf-oriented docs | Python/Node types updated |

**Grade target:** Overall visibility B− → **B+**; delight **A−** for terminal-heavy users.

---

## 6. Acceptance matrix (definition of done)

| Scenario | D1 | D2 | D3 | D4 | D5 |
|----------|----|----|----|----|-----|
| S1 long cargo build | Shelf + tail | + long-run hint | | | coalesced |
| S2 verbose off | Shelf live | | | | |
| S3 bg dev server | Shelf headline | | `/tail` | | |
| S9 parallel tools | All tools in shelf | | | | status sync |
| S6 reasoning hidden | | | | shelf snippet | |
| Gateway bg proc | | | 500-char push | | |

Full scenario list: [006-stuck-scenarios-playbook.md](006-stuck-scenarios-playbook.md).

---

## 7. Metrics & grades (target)

| Dimension | Today | After D1–D5 |
|-----------|-------|-------------|
| Tool lifecycle | A− | **A** |
| Tool mid-run detail | A− | **A** (keep tail) |
| Background visibility | B | **A−** |
| Parallel tool clarity | B | **A−** |
| UI chrome / polish | C+ | **B+** |
| Gateway parity | B− | **B+** |
| **Overall visibility** | B− | **B+** |
| **Overall liveness** | B+ | **A−** |

Validate with scripted TUI tests (extend `app::tests::*`) + manual playbook walkthrough.

---

## 8. Risk register

| Risk | Mitigation |
|------|------------|
| Shelf + transcript duplicate info | Shelf = summary; transcript = policy-gated detail; D1 copy guidelines |
| `app.rs` grows further | **Mandatory** extract in D1 before new features |
| Tail flood breaks calm | Agent-side throttle unchanged; shelf coalesce separate |
| BasicCompat clutter | Shelf off or 1-line mode <60 cols |
| Gateway message limits | Cap + line-boundary snap (port Hermes `run.py` truncate logic) |
| Regression: no live tail | CI test: terminal tool emits ToolProgress during mock build |

---

## 9. Testing strategy

| Layer | Tests |
|-------|-------|
| `turn_activity.rs` | Merge parallel tools; coalesce; long-run timer |
| `activity_shelf.rs` | Snapshot render at widths 80/120/50 |
| `app.rs` integration | Shelf updates on ToolProgress; parallel 2-tool shelf |
| `process_tail_panel.rs` | Mock ProcessTable; 4KB truncation |
| Gateway | event_processor bg 500-char fixture |
| E2E manual | [006](006-stuck-scenarios-playbook.md) checklist |

---

## 10. Documentation updates (per phase)

| Phase | Update |
|-------|--------|
| D1 | [003-tui-visibility-layer.md](003-tui-visibility-layer.md) — shelf region, stale ToolProgress note removed |
| D2 | [004-stream-event-contract.md](004-stream-event-contract.md) — `ToolGenerating` |
| D3 | AGENTS.md — `/tail` command |
| D5 | [005-honest-assessment.md](005-honest-assessment.md) — re-grade; [009](009-hermes-comparison.md) — parity closed items |

---

## 11. Suggested sprint order

```text
Sprint 1:  D1 (shelf foundation)     → biggest UX leap
Sprint 2:  D2 + D3 (/tail, generating) → Hermes parity + delight
Sprint 3:  D4 (reasoning/delegation)   → power users
Sprint 4:  D5 + gateway + docs         → ship quality bar
```

**Start D1 immediately** — it unlocks every other item without conflicting with `tool_progress_tail.rs`.

---

## 12. Cross-references

- Hermes comparison → [009-hermes-comparison.md](009-hermes-comparison.md)
- Completed liveness work → [007-implementation-roadmap.md](007-implementation-roadmap.md)
- Current TUI behavior → [003-tui-visibility-layer.md](003-tui-visibility-layer.md)
- Stream events → [004-stream-event-contract.md](004-stream-event-contract.md)
- Prior width-adaptive work → [specs/05-improve-ux-tui.md](../05-improve-ux-tui.md)
- Tool streaming visibility (Focus Tool Pane) → [specs/020-tool-streaming-visibility/000-overview.md](../020-tool-streaming-visibility/000-overview.md)
