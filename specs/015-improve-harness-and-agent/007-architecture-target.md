# 007 — Target Architecture

Target harness shape satisfying [002-first-principles.md](./002-first-principles.md) and [agent_harness/001](../agent_harness/001_adr_unified_agent_harness.md).

---

## 1. Layer diagram

```text
  ┌─────────────────────────────────────────────────────────────────┐
  │                     SURFACES (consumers)                        │
  │  CLI TUI │ Gateway │ ACP │ SDK │ Cron │ Kanban workers          │
  └────────────┬────────────────────────────────────────────────────┘
               │ ProgressSink · RunOutcome (same contract)
  ┌────────────▼────────────────────────────────────────────────────┐
  │                  UNIFIED HARNESS CORE                           │
  │  ┌─────────────┐ ┌──────────────┐ ┌─────────────────────────┐ │
  │  │ LoopDriver  │ │ Completion   │ │ TaskClassifier          │ │
  │  │ (thin)      │ │ Policy       │ │ → VerificationPolicy    │ │
  │  └──────┬──────┘ └──────────────┘ └───────────┬─────────────┘ │
  │         │                                      │               │
  │  ┌──────▼──────┐ ┌──────────────┐ ┌───────────▼─────────────┐ │
  │  │ Provider    │ │ ToolPipeline │ │ PerceptionCoordinator   │ │
  │  │ Policy      │ │ dispatch     │ │ preview · vision · test │ │
  │  └─────────────┘ └──────────────┘ └─────────────────────────┘ │
  └────────────┬────────────────────────────────────────────────────┘
               │
  ┌────────────▼────────────────────────────────────────────────────┐
  │ edgecrab-tools │ edgecrab-security │ edgecrab-state │ edgequake-llm │
  └─────────────────────────────────────────────────────────────────┘
```

---

## 2. ProgressSink (Interface Segregation)

**Trait** (new module `edgecrab-core/src/progress.rs`):

```rust
// Conceptual — implementation in plan
trait ProgressSink {
    fn on_token(&self, text: &str);
    fn on_tool_start(&self, id: &str, name: &str, args_json: &str);
    fn on_tool_progress(&self, id: &str, message: &str);
    fn on_tool_done(&self, id: &str, preview: &str, is_error: bool);
    fn on_activity(&self, text: &str, tone: ActivityTone);
    fn on_llm_wait(&self, detail: &str, elapsed_secs: u64);
}
```

**Adapters:**

| Adapter | Maps to |
|---------|---------|
| `StreamEventSink` | existing `UnboundedSender<StreamEvent>` |
| `TracingSink` | `edgecrab::harness` JSONL |
| `NoopSink` | tests |

**DRY:** `conversation.rs` emits through sink only — no duplicate `tracing::info!` for tool lifecycle (consolidate with `stream_observability.rs`).

---

## 3. RunOutcome propagation

```text
  execute_loop ends
       │
       ▼
  assess_completion(CompletionContext)
       │
       ▼
  RunOutcome { decision, exit_reason, user_message, verification }
       │
       ├──► TUI final banner + status bar clear
       ├──► Gateway agent:done hook (with exit_reason)
       ├──► ACP terminal notification
       ├──► SQLite sessions.end_reason
       └──► OTEL span attribute exit_reason
```

**Law:** `completed: bool` deprecated → `outcome.decision == Completed`.

---

## 4. Spill stub v2 (actionable)

Current stub (lossy):

```text
[tool_result_spill] artifact: .edgecrab-artifacts/…/read_file_002.md
```

Target stub (actionable):

```text
[tool_result_spill]
source_path: demo/games003/index.html
artifact: .edgecrab-artifacts/{session}/read_file_002.md
bytes: 29380 lines: 653
next: read_file(path="<artifact>", offset=1, limit=120)
hint: use patch for edits; write_file whole-file exceeds budget for this provider
```

**Single builder:** `artifact_spill::build_stub()` — used by TUI preview + model stub + history summary.

---

## 5. TaskClassifier + VerificationPolicy

```text
  user_message + optional /goal
         │
         ▼
  TaskClassifier::classify() → TaskClass
         │
         ▼
  VerificationPolicy::requirements(class)
         │
         ├── code_edit    → require TestOrLspEvidence
         ├── visual_ux    → require PreviewOrVisionEvidence
         ├── research     → require CitationEvidence
         └── conversation → none

  CompletionPolicy checks:
    harness.blocks_completion() OR missing required evidence
         → CompletionDecision::Incomplete + ExitReason::VerificationMissing
```

**Config:** `harness.verification.strict: false` default — advisory warnings first; strict mode for CI dogfood.

---

## 6. Dev preview profile (security-preserving)

```text
  config.security.preview:
    enabled: false          # default
    allow_hosts: ["127.0.0.1", "localhost"]
    allow_ports: [8000, 8080, 5173]
    max_session_minutes: 30

  browser_navigate(http://127.0.0.1:8000/...)  → allowed when enabled
  still blocks: file://, metadata IPs, non-allowlisted ports
```

**Alternative:** `capture_preview` tool runs headless screenshot to temp file → `vision_analyze` (no network SSRF).

---

## 7. Operator honesty panel

TUI status bar extension:

```text
  EC │ haiku-4.5 │ wire:55/def:52 │ ctx 25k/128k │ ↳ composing 12s (copilot non-streaming)
```

Data from `wire_partition_counts` + `local_provider_policy` — **one formatter** in `tool_progress_tail.rs`.

---

## 8. Module extraction from conversation.rs

| Extract to | Responsibility |
|------------|----------------|
| `loop/driver.rs` | iteration while, cancel, budget |
| `loop/tool_turn.rs` | parallel dispatch, spill turn budget |
| `loop/provider_call.rs` | streaming downgrade, heartbeats |
| `conversation.rs` | thin `execute_loop` delegating |

**Rule:** No behavior change in extraction PR — move only, tests green.
