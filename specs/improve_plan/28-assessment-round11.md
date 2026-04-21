# Round 11 — Waiting Phase UX: First Principles Assessment

## Context
Round 10 (FP39-FP43) completed: streaming status bar now shows `▶ ~100w  16t/s  8s`.  
This round targets the **waiting phase** — `AwaitingFirstToken` and `Thinking` states —  
the gap between user submitting a message and the first streamed token arriving.

---

## First Principles Analysis

### What currently happens

| Phase | Status bar | Output area |
|-------|-----------|-------------|
| `AwaitingFirstToken` | `⠋ (｡•́︿•̀｡) awaiting  first token` → `... 5s` → `... 12s  ^C=stop` | **frozen** — last message unchanged |
| `Thinking` | Similar with thinking verbs | Reasoning text if `show_reasoning=true`, otherwise frozen |
| Color | Always amber `#ffd278` (invariant) | n/a |

### Three core problems

**P1 — Dead air in the output area**  
The user's attention naturally rests at the **bottom of the conversation** (where new content will appear).
During `AwaitingFirstToken`, this space is completely silent. The only feedback is in the status bar —
spatially disconnected from where the user is looking. The model could be thinking hard, but the
output area says nothing.

**P2 — No urgency gradient**  
A 2-second wait and a 35-second wait show the same amber color. After 10s the text says `^C=stop`,
but the visual signal (color) is identical. Users cannot distinguish "normal slow model" from
"network stall" from "rate-limited". The signal does not escalate with urgency.

**P3 — No TTFB calibration**  
After the response arrives, the Time to First Token (TTFB) is never shown. Users have no way to
calibrate whether their model is fast (0.8s) or slow (12s), or whether today's TTFB is slower than
usual. This information would help with model-selection decisions.

### First Principles Principles

1. **Presence at focal point**: Activity indicators must appear WHERE the user is looking, not
   just in a peripheral bar. The bottom of the output area IS the focal point during waiting.

2. **Urgency gradient**: Visual encoding (color, symbol) must escalate proportionally with the
   magnitude of the deviation from expectation. Same color for 2s and 35s violates this.

3. **Calibration over time**: Users learn their tools. Recording and surfacing TTFB gives
   actionable data for model-choice decisions without requiring external tooling.

4. **Minimum friction for high-signal actions**: `^C=stop` is already shown after 10s. At 20s+,
   the signal should be stronger — a warning symbol, stronger color — because the probability
   that this is an abnormal stall increases.

---

## Brutal Assessment of Current State

**Good**: The elapsed-time tiers in `format_phase_status` already provide time-based text
escalation (bare → label → `+elapsed` → `+^C=stop`). The `^C=stop` hint at >10s is correct.

**Bad**:
- The output area shows **nothing** during wait — a full blank canvas of 20+ lines that
  could be putting the waiting indicator right at the user's visual focal point. This is a
  significant missed opportunity.
- Color is invariant (amber) across all wait durations. There is no visual urgency ramp.
- TTFB is never recorded or displayed. Users cannot learn from it.
- No 20-second "stall" tier with a stronger signal — the >10s tier uses `^C=stop` but no
  visual escalation (same text color, same format).

---

## Implementation Plan

### FP44 — TTFB Tracking and Display

**Goal**: Record Time To First Token per turn; display it after turn completes.

**Changes**:
1. Add `last_ttfb_secs: Option<f32>` to `AppState` (near `last_response_time`)
2. In `AgentResponse::Token` handler: when transitioning `AwaitingFirstToken → Streaming`,
   read `started.elapsed().as_secs_f32()` from the `AwaitingFirstToken` state and store in `last_ttfb_secs`
3. In `AgentResponse::Done` handler: if `last_ttfb_secs` is set and `> 0.0`, emit a subtle
   system line: `  ↳ ttfb: 2.3s` — visible in the output transcript for calibration
4. Add the field to `AppState::default()` / struct initializer

**Tests**: Verify `last_ttfb_secs` is set correctly when Token is received from AwaitingFirstToken.

---

### FP45 — Inline Ghost Waiting Line in Output Area

**Goal**: During `AwaitingFirstToken` and `Thinking` (when no reasoning text is shown),
render a dim animated "ghost" line at the bottom of the output area — putting the waiting
indicator at the user's focal point.

**Changes in `render_output()`**:  
After building the `lines: Vec<Line<'static>>` vector (after the role-bar loop), inject a ghost line:

```
▎  ⠋  awaiting response…          (after 0-3s)
▎  ⠙  awaiting response…  5s      (after 3s)
▎  ⠹  awaiting response…  12s     (after 10s)
```

For `Thinking` when `reasoning_line.is_none()`:
```
▎  ⠋  thinking…
```

**Design**:
- Role bar: dim cyan `▎ ` (matches assistant bar accent, but at DIM modifier)
- Text: italic + DIM in a cool slate blue `#50687a`
- Uses `SPINNER_FRAMES[frame]` from the `DisplayState` for live animation
- Elapsed shown after 3s

**Why this works**: The ghost line is purely cosmetic, injected at render time. No state change.
It disappears naturally when the state transitions to `Streaming` and a real `OutputLine` appears.

**Also inject in `render_output_compact()`** for the compact/BasicCompact profile.

---

### FP46 — Status Bar Color Escalation

**Goal**: Make the status bar color change as wait time increases, providing a visual urgency ramp.

**Tiers**:
- `elapsed < 15s`: amber `Color::Rgb(255, 210, 120)` — current/normal
- `15s ≤ elapsed < 30s`: orange `Color::Rgb(255, 140, 50)` — slow, pay attention
- `elapsed ≥ 30s`: red `Color::Rgb(239, 83, 80)` — stall, likely network issue

**Changes**:
- In `render_status_bar()` `AwaitingFirstToken` arm: compute `elapsed_secs`, map to color
- In `render_status_bar()` `Thinking` arm: same color escalation
- In `compact_status_bar()` `AwaitingFirstToken` arm: same

---

### FP47 — 20-Second Urgency Tier in `format_phase_status`

**Goal**: Add a 4th tier at >20s with a `⚠` prefix warning symbol.

**Change to `format_phase_status()`**:
```
elapsed >20s:  ⚠ {long_label} {elapsed}s  ^C=stop
elapsed >10s:  {long_label} {elapsed}s  ^C=stop   (existing)
elapsed >3s:   {long_label} {elapsed}s             (existing)
elapsed >1s:   {early_label}                        (existing)
else:          bare spinner                          (existing)
```

---

## Checklist

- [ ] FP44: `last_ttfb_secs` field + computation + Done display
- [ ] FP45: Ghost line in `render_output()` and `render_output_compact()`
- [ ] FP46: Color escalation in all three status-bar render sites
- [ ] FP47: 20s tier in `format_phase_status()`
- [ ] Run `cargo fmt --workspace`
- [ ] Run `cargo clippy --workspace -- -D warnings`
- [ ] Run `cargo test --workspace` — 2516+ tests pass
- [ ] Manual verify: Run edgecrab, submit a prompt, observe ghost line in output area

## Expected outcome
- Ghost waiting line visible in output area during wait — spatial coherence
- Status bar turns orange at 15s, red at 30s — urgency ramp
- TTFB printed as `↳ ttfb: X.Xs` after each turn — calibration signal
- `⚠` warning at 20s+ — reinforces the urgency message
