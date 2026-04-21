# Mission Steering — Implementation Plan

> **Status as of 2026-04-21:** Phases 1 & 2 fully shipped and verified.
> Phase 3 (TUI overlay) and Phase 4 (gateway second-message) are planned next work.

## Phase 1: Core Types (edgecrab-core) ✅ COMPLETE

- [x] Create `crates/edgecrab-core/src/steering.rs`
  - `SteeringKind` enum (Hint, Redirect, Stop)
  - `SteeringEvent` struct with `new()` constructor
  - `SteeringSender` / `SteeringReceiver` type aliases (tokio unbounded mpsc)
  - `steering_channel()` factory
  - `drain_pending_steers()` → non-blocking drain, strongest-kind merge, None when empty
  - `build_steer_message()` with `[⛵ STEER]` prefix, EC-11 truncation, injection scan
  - `scan_and_sanitize_steer()` using `prompt_builder::scan_for_injection` + `ThreatSeverity::High`
  - 6 unit tests — all pass
- [x] Register `pub mod steering;` + re-exports in `lib.rs`

## Phase 2: Agent Integration (edgecrab-core) ✅ COMPLETE

- [x] Extend `Agent` struct with `steer_tx: SteeringSender` + `steer_rx: Mutex<Option<SteeringReceiver>>`
- [x] Initialise channel pair in `build_runtime_clone()` — single `steering_channel()` call
- [x] Expose `Agent::steer_sender() -> SteeringSender` (clones steer_tx)
- [x] `execute_loop`: take `steer_rx` from mutex at loop start; restore at exit
- [x] `execute_loop`: drain steering channel at every `LoopAction::Continue` boundary
- [x] EC-02: STOP steer signals the cancel token after injection
- [x] Emit `StreamEvent::SteerApplied { message }` on each injected steer
- [x] Add `StreamEvent::SteerPending { count }` variant (for future optimistic TUI sync)
- [x] All exhaustive `match event` consumers updated:
  - `crates/edgecrab-gateway/src/event_processor.rs` — log-only arms
  - `crates/edgecrab-cli/src/app.rs` — forwards `SteerApplied` as `AgentResponse::Notice`
  - `crates/edgecrab-core/tests/e2e_copilot.rs` — no-op arms (test hygiene)
- [x] Full workspace builds clean (`cargo build --workspace`)
- [x] All 43 library unit tests pass; 6/6 steering unit tests pass

## Phase 3: TUI Interactive Overlay (edgecrab-cli) 🔜 NEXT

- [ ] Add `steer_tx: Option<SteeringSender>`, `pending_steer_count: usize`,
      `steering_overlay_active: bool`, `steering_input: TextArea`,
      `steering_kind: SteeringKind` fields to `App`
- [ ] Wire `app.steer_tx = Some(agent.steer_sender())` after agent spawn
- [ ] Ctrl+S opens overlay when agent is running; Esc closes without sending
- [ ] Tab cycles `SteeringKind` (Hint → Redirect → Stop → Hint)
- [ ] Enter sends steer: `steer_tx.send(SteeringEvent::new(kind, text))` + increment counter
- [ ] `render_steering_overlay()` — floating panel per `05-ux-tui.md` wireframe
- [ ] Status bar: show `⛵ N pending` while count > 0; `⛵ applied` on `SteerApplied` event
- [ ] Help bar: add `Ctrl+S Steer` hint when agent is running
- [ ] EC-04: send steer while idle → call `chat_streaming()` with steer text as new user message
- [ ] EC-10: overlay focus management — pass all keys to textarea when overlay is open

## Phase 4: Gateway Second-Message Steering 🔜 FUTURE

- [ ] Gateway `SessionManager`: detect second message while session is running
- [ ] Configurable mode: `gateway.second_message_mode = steer | queue | interrupt`
- [ ] `steer` mode: call `steer_tx.send(SteeringEvent::new(Redirect, message))` + ack to user
- [ ] `queue` mode: enqueue for next conversation turn (current behavior)
- [ ] `interrupt` mode: call `agent.interrupt()` + start new turn with combined context

## Phase 5: Tests

- [x] `steering.rs` unit tests: drain, build_steer_message, injection block, truncation (6 tests)
- [ ] `conversation.rs` integration test: steer injected at tool boundary (requires mock provider)
- [ ] `app.rs` TUI test: Ctrl+S opens overlay, Enter sends steer (Phase 3 prerequisite)
- [ ] Edge case tests: EC-02 (stop cancels token), EC-06 (injection blocked), EC-12 (cancel+steer race)
