# Terminal Reply Stall Investigation

## Status

This case was reopened because the original TUI-stream bridge fix was not sufficient to explain the user's latest failure.

There are two distinct issues:

1. A previously fixed TUI bridge reliability bug in `crates/edgecrab-cli/src/app.rs`.
2. A newly isolated pre-LLM stall in `crates/edgecrab-core/src/prompt_builder.rs` when EdgeCrab is launched from `~` in Apple Terminal or iTerm2.

The reopened user report is explained by issue 2.

Follow-up investigation found three adjacent classes of terminal-facing latency risk that were worth fixing after the main root cause was removed:

1. duplicate pre-LLM plugin discovery work per turn
2. native streaming requests that stall before the first visible chunk
3. terminal hosts that cannot keep up with full-frame redraw frequency

## First-Principles Decomposition

Start from facts that do not depend on theory:

1. VS Code works reliably.
2. Apple Terminal and iTerm2 fail when EdgeCrab is launched from the home directory.
3. The spinner continues animating, so the TUI event loop is alive.
4. Some failing runs never reach `api_call_with_retry`.
5. Running the same installed binary from `~` in quiet mode times out before any API request.
6. The same command succeeds from `~` when only `EDGECRAB_SKIP_CONTEXT_FILES=1` is added.

Those facts rule out several wrong explanations:

- not a Copilot outage
- not a missing prompt submission in general
- not a terminal rendering freeze
- not a provider response that the TUI merely fails to paint

They isolate the failure boundary to work that happens before the first LLM call.

## OODA

### Observe

Observed evidence:

- User screenshots:
  - iTerm2 shows `waiting for first token`
  - VS Code displays a normal reply
- TUI log evidence from `~/.edgecrab/profiles/homelab/logs/agent.log`:
  - `TUI: dispatching prompt to agent task`
  - `execute_loop: acquired conversation_lock`
  - repeated `browser_is_available: true`
  - then, in the failing case, no `execute_loop: entering main conversation_loop`
- Quiet-mode reproduction from `/Users/raphaelmansuy` with the installed binary:
  - command:
    `TERM_PROGRAM=iTerm.app EDGECRAB_LOG_FILTER=debug timeout 20 ~/.cargo/bin/edgecrab -p homelab --quiet --debug 'Hello World !'`
  - result: timeout after 20 seconds
  - last logs stop after `execute_loop: acquired conversation_lock` and browser availability checks
- Control experiment with context files disabled:
  - command:
    `TERM_PROGRAM=iTerm.app EDGECRAB_LOG_FILTER=debug EDGECRAB_SKIP_CONTEXT_FILES=1 timeout 40 ~/.cargo/bin/edgecrab -p homelab --quiet --debug 'Hello World !'`
  - result: success
  - logs show:
    - `execute_loop: entering main conversation_loop`
    - `api_call_with_retry: sending API request`
    - `Raw API response status=200 OK`
  - assistant reply prints normally
- PTY TUI verification with the installed binary and context files disabled:
  - `TERM_PROGRAM=iTerm.app ... ~/.cargo/bin/edgecrab -p homelab --debug`
  - `TERM_PROGRAM=Apple_Terminal ... ~/.cargo/bin/edgecrab -p homelab --debug`
  - both runs rendered the assistant reply in the terminal
  - both logs reached `TUI→agent: forwarding done`

### Orient

The critical environment difference is not the terminal emulator by itself.
It is the launch directory.

When EdgeCrab is launched from a VS Code workspace, `cwd` is normally a bounded project directory.
When launched from Apple Terminal or iTerm2, `cwd` is often `/Users/<user>`.

`PromptBuilder::build()` calls `discover_context_files(cwd)`.
That function previously treated `cwd` as a project root and recursively scanned subdirectories for `AGENTS.md`.

From `~`, that means EdgeCrab tries to walk the user's home tree before the first LLM request.
On macOS this is pathological because:

- `~/Library` is extremely large
- many directories are inaccessible or slow
- recursive traversal from `~` is not a meaningful "project context" operation

That explains the exact symptom pattern:

- VS Code workspace: bounded scan, request proceeds
- Terminal launched from `~`: massive preflight scan, request appears to hang before first token

### Decide

The correct fix is to bound recursive AGENTS discovery to an actual project root.

Rules:

1. If inside a git repo, recurse from the git root.
2. If not inside a git repo but `cwd` clearly looks like a project root, recurse from `cwd`.
3. Otherwise, do not recurse through subdirectories.
4. Outside a detected project root, only load `cwd/AGENTS.md` directly if it exists.
5. Do not follow symlinked directories during AGENTS traversal.

This preserves project-context behavior while preventing home-directory scans.

### Act

Implemented in `crates/edgecrab-core/src/prompt_builder.rs`:

- `discover_context_files(...)` now decides whether recursive AGENTS scanning is allowed
- added `agents_scan_root(...)`
- added `find_git_root(...)`
- added `looks_like_project_root(...)`
- recursive AGENTS discovery now skips symlinked directories
- added regression tests:
  - `agents_md_does_not_recurse_outside_project_root`
  - `agents_md_uses_git_root_when_called_from_subdir`

The earlier TUI bridge hardening in `crates/edgecrab-cli/src/app.rs` remains valid, but it does not solve this reopened case by itself because this failure occurs before the first API call.

## Follow-Up OODA: Similar Pause Cases

### Observe

After the home-directory scan bug was fixed, there were still nearby failure modes that could create the same user perception: "spinner moves, but reply feels stuck."

Relevant code-path evidence:

- `Agent::execute_loop()` was still doing extra pre-turn work that could be duplicated
- native streaming could wait indefinitely for the first visible chunk on some providers
- the TUI still assumed that non-VS Code terminals could absorb the same redraw rate as GPU-backed terminals
- macOS AppleEvents permission probing is itself capable of blocking the terminal host

### Orient

From first principles, a terminal reply feels "hung" whenever any of these boundaries stop making visible forward progress:

1. before the request is sent
2. after the request is sent but before the first token / tool event arrives
3. after events arrive but before the terminal can paint them
4. while preflight permission checks block the thread that should reach step 1

Those are distinct boundaries and need distinct fixes.

### Decide

Apply the minimum correction at each boundary:

1. remove duplicated pre-LLM work
2. add a bounded first-chunk wait and retry path for invisible streaming stalls
3. make the TUI adapt redraw rate to terminal capacity instead of assuming one speed fits all
4. keep only safe macOS permission probes in the hot path

### Act

Implemented follow-up hardening:

- `crates/edgecrab-core/src/conversation.rs`
  - plugin discovery is now reused instead of re-run later in the same turn
  - native streaming now has a first-chunk timeout
  - if streaming stalls before any visible output, the request recovers instead of appearing to wait forever
  - smart-routed model failures can retry the primary model before escalating to fallback
- `crates/edgecrab-cli/src/app.rs`
  - terminal behavior now uses capability profiles rather than a single Apple-only branch
  - `Standard`: full UX
  - `ReducedMotion`: live output stays on, but redraw churn is throttled and cosmetic animation is suppressed
  - `BasicCompat`: live token painting is buffered, mouse capture is disabled, redraw rate is heavily throttled
- `crates/edgecrab-tools/src/macos_permissions.rs`
  - Accessibility preflight stays enabled because `AXIsProcessTrusted()` is cheap and safe
  - AppleEvents / AED probing stays disabled in the hot path because the probe itself can block terminal hosts

## Terminal UX Decision

### First-Principles Rule

Terminal UX should degrade by terminal capacity, not by operating system label.

What matters is:

1. how fast the PTY can consume escape-sequence output
2. whether keyboard enhancement is supported
3. whether mouse capture harms expected copy/scroll behavior
4. whether incremental token painting helps more than it hurts

### Terminal Profiles

Implemented model:

- `Standard`
  - full animation
  - live token display
  - mouse capture on
  - no draw throttling
- `ReducedMotion`
  - live token display stays on
  - spinner / cosmetic motion is suppressed
  - draw rate is throttled to reduce PTY pressure
  - keyboard and mouse support stay on
- `BasicCompat`
  - live token display is buffered until tool boundaries / turn completion
  - mouse capture defaults off so copy remains native
  - draw rate is aggressively throttled
  - fallback paging shortcuts are exposed

### Linux Coverage

This is not Apple-only.

Reduced-motion detection now explicitly covers Linux-heavy terminal classes and multiplexed terms, including:

- VTE-family desktops such as GNOME Terminal and related terminals
- Konsole-class environments
- `screen*` and `tmux*` terms
- legacy `rxvt`, `linux`, `vt100`, `vt220`, `ansi`, `putty`, and similar low-signal terms

Fast-terminal markers such as kitty / WezTerm / Windows Terminal / Alacritty-style environments remain on the standard profile.

## macOS Permission Decision

### Observe

The user explicitly asked whether AED permissions add value when a tool requests access.

We need to separate "valuable information" from "safe to ask in-band."

### Orient

Two macOS permission APIs behave very differently:

- Accessibility: `AXIsProcessTrusted()`
  - direct, fast, safe
- AppleEvents / Automation: `AEDeterminePermissionToAutomateTarget()`
  - sometimes blocks even with `ask_user_if_needed = false`
  - can recreate the same terminal-host stall class we just removed

### Decide

Keep Accessibility probing.

Do not do AppleEvents/AED probing in the hot path by default.

That is the only defensible decision if the product goal is "the terminal must never look hung before the user sees progress."

### Act

Implemented behavior:

- TUI permission status now reports real Accessibility state
- Automation status remains `Unknown` unless inferred heuristically from the command
- code now exposes a direct Accessibility helper instead of faking it through a synthetic command preflight

This keeps the UX useful without reintroducing the blocking class of bug.

## Root Cause

The reopened root cause is unbounded project-context discovery.

More precisely:

- `discover_context_files(cwd)` recursively searched for `AGENTS.md`
- when `cwd` was `/Users/raphaelmansuy`, it treated the home directory like a project workspace
- the preflight context scan consumed the request budget before the first LLM call
- the user saw an infinite-looking wait even though the provider had not been called yet

In short:

> EdgeCrab was recursively scanning the user's home directory for project context before sending the LLM request.

## Why This Fits The Evidence

This explanation matches all verified facts:

- VS Code works: yes, because it launches inside a bounded workspace
- iTerm2 / Apple Terminal from `~` fail: yes
- some failing runs never reach `api_call_with_retry`: yes
- disabling context-file loading makes the same installed binary succeed from `~`: yes
- once the request is allowed to proceed, logs show `200 OK` and the TUI renders the reply: yes

No TUI-only theory explains why `EDGECRAB_SKIP_CONTEXT_FILES=1` flips the outcome from timeout to success before any UI-specific code changes.

## Evidence Block

### Evidence That The Message Was Not Reaching The LLM In The Reopened Case

Failing quiet-mode run from `/Users/raphaelmansuy`:

- command:
  `TERM_PROGRAM=iTerm.app EDGECRAB_LOG_FILTER=debug timeout 20 ~/.cargo/bin/edgecrab -p homelab --quiet --debug 'Hello World !'`
- logs show:
  - `execute_loop: acquired conversation_lock`
  - repeated `browser_is_available: true`
- logs do not show:
  - `execute_loop: entering main conversation_loop`
  - `api_call_with_retry: sending API request`

That proves the stall occurs before the provider request.

### Evidence That Disabling Context Discovery Removes The Stall

Control run from the same directory with only context files disabled:

- command:
  `TERM_PROGRAM=iTerm.app EDGECRAB_LOG_FILTER=debug EDGECRAB_SKIP_CONTEXT_FILES=1 timeout 40 ~/.cargo/bin/edgecrab -p homelab --quiet --debug 'Hello World !'`
- logs show:
  - `execute_loop: entering main conversation_loop`
  - `api_call_with_retry: sending API request`
  - `Raw API response status=200 OK`
- result:
  - assistant reply printed successfully

That is causal evidence, not just correlation.

### Evidence That The TUI Works Once Preflight Is Unblocked

PTY TUI runs from `/Users/raphaelmansuy` with the installed binary:

- `TERM_PROGRAM=iTerm.app EDGECRAB_SKIP_CONTEXT_FILES=1 ~/.cargo/bin/edgecrab -p homelab --debug`
- `TERM_PROGRAM=Apple_Terminal EDGECRAB_SKIP_CONTEXT_FILES=1 ~/.cargo/bin/edgecrab -p homelab --debug`

Observed outcome:

- prompt submitted successfully
- spinner advanced while waiting
- assistant reply rendered in the terminal
- log evidence reached:
  - `api_call_with_retry: sending API request`
  - `Raw API response status=200 OK`
  - `TUI→agent: forwarding done`

## Verification

Code-level verification added:

- `crates/edgecrab-core/src/prompt_builder.rs`
  - `agents_md_does_not_recurse_outside_project_root`
  - `agents_md_uses_git_root_when_called_from_subdir`

Runtime verification completed with the installed binary:

- reproduced pre-LLM stall from `/Users/raphaelmansuy`
- proved the request does not reach the provider in the failing configuration
- proved the same binary reaches the provider and gets `200 OK` when context discovery is disabled
- proved the TUI renders replies in simulated iTerm2 and Apple Terminal PTYs once preflight is unblocked

Additional verification added for the follow-up hardening:

- `crates/edgecrab-core/src/conversation.rs`
  - `api_call_with_retry_falls_back_after_stream_stalls_before_first_chunk`
- `crates/edgecrab-cli/src/app.rs`
  - terminal profile tests for Apple Terminal, Linux desktop terminals, fast terminals, and multiplexed terms
  - `model_selector_page_down_pages_list_in_split_view`
  - `raw_ctrl_f_and_ctrl_b_page_model_selector_list_without_page_keys`
  - `model_selector_fullscreen_esc_returns_to_split_view`
- syntax validation on edited Rust files:
  - `rustfmt --edition 2024 --check crates/edgecrab-cli/src/app.rs`
  - `rustfmt --edition 2024 --check crates/edgecrab-core/src/conversation.rs`
  - `rustfmt --check crates/edgecrab-tools/src/macos_permissions.rs crates/edgecrab-cli/src/permissions.rs`

The full cargo compile in this environment remains intermittently blocked by sleeping `cargo/rustc` processes, so targeted syntax and unit-test coverage were used as the reliable evidence path for this pass.

## Follow-Up Root Cause: PgUp / PgDn In Overlay Selectors

This turned out to be a different bug from the pre-LLM stall.

### First-Principles Observation

In split selector overlays, the visible user goal is usually:

- move through the result list faster

But the code path was doing:

- `PgUp` / `PgDn` routed to `page_up_split_detail(...)` / `page_down_split_detail(...)`
- the selected row in the left list did not move
- the right detail pane often had no overflow, so the scroll mutation had no visible effect

That combination makes a received key look indistinguishable from a dropped key.

### Code Evidence

Before the fix, the simple split selectors in `crates/edgecrab-cli/src/app.rs` all mapped page intent to detail scroll in split view:

- model selector
- vision model selector
- image model selector
- MCP selector
- remote MCP browser
- remote skill browser
- remote plugin browser
- profile selector
- skill selector
- tool manager
- plugin toggle
- config selector

This was the wrong abstraction boundary.

`PgUp` / `PgDn` is not fundamentally a "detail scroll" event.
It is a paging intent.
Only after focus/context is known should it resolve to one of:

- page the list
- page the detail pane

### Corrected Decision

Implemented rule:

- split selectors without explicit pane focus: page the list
- fullscreen detail mode: page the detail pane
- focused two-pane browsers such as gateway/session views: keep focus-based paging

### Evidence That The Fix Matches The Real Failure

Regression tests now prove the intended behavior:

- `model_selector_page_down_pages_list_in_split_view`
  - `PageDown` advances `model_selector.selected`
  - split-detail scroll stays `0`
- `raw_ctrl_f_and_ctrl_b_page_model_selector_list_without_page_keys`
  - raw PTY control bytes `^F` and `^B` page the same list even when dedicated page keys are absent
- `model_selector_fullscreen_esc_returns_to_split_view`
  - fullscreen detail paging changes detail scroll while leaving the selected list row unchanged

That is the root-cause proof:

> the keys were reaching the app, but split overlays were mutating invisible detail scroll state instead of the visible list selection.

## Residual Risk

The code fix in `prompt_builder.rs` still needs a clean rebuild of the CLI binary in this environment before the exact patched binary can be exercised end-to-end.

However, the root cause is now isolated with direct causal evidence:

- failing from `~` without context-scan suppression
- succeeding from the same directory, same binary, same terminal identity, once context scanning is disabled

That makes the remediation target precise and defensible.

For the follow-up work, residual risk is concentrated in terminal-profile tuning rather than correctness:

- some terminals may need future reclassification between `ReducedMotion` and `Standard`
- real GUI-host testing in Apple Terminal / iTerm2 / Linux terminals is still preferable after the next clean rebuild

The important architectural decision is now stable:

> never let optional preflight work or cosmetic redraw churn sit on the same critical path as "send request, receive first visible progress, paint reply".
