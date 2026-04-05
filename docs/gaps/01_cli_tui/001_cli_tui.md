# CLI / TUI Gap Analysis

## Bottom line

EdgeCrab exceeds Hermes on terminal control, keyboard normalization, and full-screen overlay composition.

That claim is grounded in `edgecrab/crates/edgecrab-cli/src/app.rs`, which now owns:

- full-screen `ratatui` rendering
- overlay precedence inside one state machine
- progressive keyboard negotiation with all CSI-u flags enabled
- explicit keyboard warmup before the first key is consumed
- ghost hints, fuzzy selectors, compose modes, and mouse-capture toggling in one input loop

From first principles, this matters because terminal UX quality is determined by who owns the event loop, who owns redraw order, and who normalizes key events before application logic sees them.

## Code-backed EdgeCrab advantages

### 1. Terminal state is unified instead of federated

EdgeCrab keeps transcript rendering, input editing, completion, model selection, skill selection, session browsing, approval, and secret capture inside one Rust TUI state machine.

That reduces three classes of failure:

- inconsistent redraw order
- modal conflicts between subsystems
- duplicated keyboard dispatch logic

### 2. Keyboard handling is stronger at the protocol boundary

The keyboard bug behind the original report was real: the terminal could still be switching protocols while the first printable key was already being decoded.

EdgeCrab now fixes that at the lowest correct layer:

- enter raw mode first
- request `DISAMBIGUATE_ESCAPE_CODES`
- request `REPORT_EVENT_TYPES`
- request `REPORT_ALL_KEYS_AS_ESCAPE_CODES`
- request `REPORT_ALTERNATE_KEYS`
- flush stdout
- wait through a short warmup window before reading keys

That is the correct first-principles fix because layout correctness must be established before the app interprets text.

### 3. Overlay composition is materially stronger

EdgeCrab already ships first-class states for:

- model selector
- skill selector
- session browser
- approval capture
- secret and sudo capture

These are not ad hoc prompts. They are part of the same full-screen renderer, which gives EdgeCrab tighter control over focus, precedence, and interruption behavior.

## Where Hermes still leads

Hermes still leads on one real input primitive: live microphone capture.

`hermes-agent/tools/voice_mode.py` provides push-to-talk recording and playback. EdgeCrab does not yet have an equivalent microphone input loop in the CLI. TTS output is not a substitute for audio input.

Hermes also retains some presentation-layer polish in a few Rich-driven surfaces, but that is cosmetic, not architectural.

## Gap verdict

For text-first terminal use, EdgeCrab is now ahead. The remaining substantive CLI gap is microphone input, not keyboard handling or TUI structure.

## Sources audited

- `edgecrab/crates/edgecrab-cli/src/app.rs`
- `edgecrab/crates/edgecrab-cli/src/commands.rs`
- `edgecrab/crates/edgecrab-cli/src/fuzzy_selector.rs`
- `hermes-agent/tools/voice_mode.py`
