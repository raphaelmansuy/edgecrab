# 006 — Stuck Scenarios Playbook

Scenario → what the user sees → root cause → code anchor → **status (post terminal-ux branch)**.

Use this when users report “the agent is stuck.”

**Legend:** ✅ mitigated · ⚠️ partial · ❌ open

---

## S1 — Long `cargo build` / `npm test` — ✅

| | |
|---|---|
| **User report** | “Spinner for 3 minutes, nothing happening” |
| **Visible now** | Placeholder updates with last 3 stdout lines @ ≤5/sec; status bar elapsed |
| **Code** | [tool_progress_tail.rs](../../crates/edgecrab-tools/src/tool_progress_tail.rs), [terminal.rs](../../crates/edgecrab-tools/src/tools/terminal.rs), [local.rs](../../crates/edgecrab-tools/src/tools/backends/local.rs) |
| **If still stuck** | Remote SSH/Modal (start milestone only until complete); `/verbose off` (see S2) |

---

## S2 — No output area activity — ⚠️

| | |
|---|---|
| **User report** | “Only status bar moves” |
| **Visible now** | `/verbose off`: dim `⏳ tool preview (Ns)` + tail updates in-place |
| **Code** | [app.rs `ensure_tool_progress_placeholder`](../../crates/edgecrab-cli/src/app.rs), `format_minimal_tool_indicator` |
| **Remaining** | User must know `/verbose off` still shows minimal indicator |

---

## S3 — Background dev server — ✅

| | |
|---|---|
| **User report** | “Did it start?” |
| **Visible now** | `BackgroundProcessTail` monitor line; finish line on exit |
| **Code** | [process_table.rs](../../crates/edgecrab-tools/src/process_table.rs), [conversation.rs `forward_process_watch_event`](../../crates/edgecrab-core/src/conversation.rs) |

---

## S4 — `wait_for_process` — ✅

| | |
|---|---|
| **User report** | “Frozen on wait_for_process” |
| **Visible now** | 2s heartbeat with tail snapshot via `format_wait_heartbeat` |
| **Code** | [process.rs](../../crates/edgecrab-tools/src/tools/process.rs) |

---

## S5 — LLM thinking after tool — ⚠️ (normal)

| | |
|---|---|
| **Visible** | `AwaitingFirstToken` ghost + spinner after ToolDone |
| **Mitigation** | Expected; urgency color ramp |
| **Optional polish** | “Processing tool result…” label |

---

## S6 — Extended reasoning — ⚠️ (normal)

| | |
|---|---|
| **Mitigation** | `/reasoning show`; status bar elapsed |

---

## S7 — Context compression — ✅

| | |
|---|---|
| **Visible now** | `ActivityNotice` on start, circuit breaker, and done; `ContextPressure` at 85% |
| **Code** | [conversation.rs compression block](../../crates/edgecrab-core/src/conversation.rs), `format_compression_*` in tool_progress_tail |

---

## S8 — Approval gate — ✅

| | |
|---|---|
| **Visible now** | Approval overlay + system `ActivityNotice` (`format_approval_waiting`); gateway pending-interaction message |
| **Code** | [conversation.rs approval forwarder](../../crates/edgecrab-core/src/conversation.rs), [app.rs WaitingForApproval](../../crates/edgecrab-cli/src/app.rs) |

---

## S9 — Parallel tools — ✅

| | |
|---|---|
| **Visible now** | Per-call placeholders; status follows latest progress; `+N more` in status bar |
| **Code** | Parallel `tool_progress_tx` clone; [app.rs ToolProgress handler](../../crates/edgecrab-cli/src/app.rs) |

---

## S10 — Streaming disabled / BasicCompat — ❌

| | |
|---|---|
| **Mitigation** | `/stream on`; wider terminal |
| **Code** | [app.rs live_token_display](../../crates/edgecrab-cli/src/app.rs) |

---

## S11 — Provider SSE stall — ⚠️

| | |
|---|---|
| **Mitigation** | Retry turn; may surface as `Error` |
| **Code** | [api_call_streaming](../../crates/edgecrab-core/src/conversation.rs) |

---

## S12 — Gateway (Telegram/Discord) — ⚠️

| | |
|---|---|
| **Visible now** | Throttled ToolProgress status (last line); bg process snippets |
| **Remaining** | Not in-place like TUI; platform message limits |
| **Code** | [event_processor.rs](../../crates/edgecrab-gateway/src/event_processor.rs) |

---

## S13 — Remote SSH/Modal command — ⚠️

| | |
|---|---|
| **Visible now** | Start milestone immediately; tail on completion |
| **Remaining** | No live byte-stream during remote run |
| **Code** | [backends/mod.rs `start_execute_progress`](../../crates/edgecrab-tools/src/tools/backends/mod.rs) |

---

## S14 — LM Studio / Ollama “composing tool call” — ⚠️ (often normal)

| | |
|---|---|
| **User report** | “Stuck 30–180s after tools finished”; status `(¬_¬) negotiating` / `awaiting` |
| **Visible now** | Shelf `lmstudio: still composing tool call ~Nk/262k ctx`; preflight `max_tokens=2048 reasoning=none tool_choice=required`; LM Studio **GEN/tok** climbing |
| **Root cause** | **Not tool dispatch** — blocked on non-streaming `chat/completions` while server prefills prompt + generates tool-call JSON ([first principles](../014-improve-local-harness/002-first-principles-why.md)) |
| **Code** | [local_provider_policy.rs](../../crates/edgecrab-core/src/local_provider_policy.rs), [conversation.rs non-streaming heartbeat](../../crates/edgecrab-core/src/conversation.rs), [mutation_turn_policy.rs](../../crates/edgecrab-tools/src/mutation_turn_policy.rs) |
| **If GEN → ~2048 then loops** | `finish_reason=length` + empty `tool_calls` — output budget exhausted; steer incremental mutation or rebuild with [014 prefill/length prune](../014-improve-local-harness/006-solution-plan.md) |
| **If benign** | GEN increasing, tools already fast — wait; watch LM Studio UI |

---

## Diagnostic checklist

1. **`display_state`** — ToolExec vs AwaitingFirstToken vs WaitingForApproval
2. **`/verbose` mode** — Off still shows minimal `⏳` line when progress wired
3. **Last tool** — `terminal`, `wait_for_process`, `web_*`, `browser_*` = long-blocking
4. **Logs** — compression ActivityNotice, tool progress throttling, `edgecrab::local_llm` length/prune events
5. **`in_flight_tool_count`** — parallel tools
6. **Approval/clarify modal** — input rerouted
7. **Local provider** — if tools done but shelf says “composing”, see **S14** (not S1 terminal stall)

## Cross-references

- Assessment → [005-honest-assessment.md](005-honest-assessment.md)
- Stream events → [004-stream-event-contract.md](004-stream-event-contract.md)
- Roadmap → [007-implementation-roadmap.md](007-implementation-roadmap.md)
- Local inference harness → [014-improve-local-harness](../014-improve-local-harness/README.md)
