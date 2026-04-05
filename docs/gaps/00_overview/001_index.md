# EdgeCrab vs Nous Hermes Gap Index

## Scope

This folder compares the current `edgecrab` and `hermes-agent` codebases feature-by-feature, using the source tree as ground truth rather than README claims.

The comparison is grouped by feature area:

- `01_cli_tui` — terminal UX, input system, overlays, interaction model
- `02_core_tools` — agent loop, tool surface, browser/tooling depth
- `03_execution_backends` — local/container/cloud/remote execution substrates
- `04_gateway_channels` — messaging adapters and operator ergonomics
- `05_memory_skills_state` — sessions, memories, skills, migration
- `06_security_distribution` — security model, packaging, deployment ergonomics
- `07_research_training` — batch eval, RL, trajectory tooling

## Audited facts

The current codebase audit supports the following concrete facts:

| Area | EdgeCrab | Hermes |
|---|---|---|
| TUI foundation | Stronger | Weaker |
| Browser tool verbs | 14 | 11 |
| Tool/runtime architecture | Cleaner typed substrate | Broader mature surface |
| Execution backends | 6 active backends | 6 active backends |
| Gateway adapter count | 12 adapters | 12 adapters |
| Skills inventory | 111 `SKILL.md` files | 111 `SKILL.md` files |
| Research / RL infrastructure | Thin | Much stronger |
| Distribution | Stronger | Weaker |
| Test files under audited test trees | about 209 | about 694 |

These numbers matter because first-principles comparison starts with shipped surfaces, not aspiration:

- if a backend module is not present, it is not a runtime capability
- if a tool is not registered in source, it is not part of the core surface
- if a setup path does not exist, the operator experience is incomplete
- if a claim in prose exceeds the code, the prose is wrong

## Executive summary

The current split between the projects is clearer than it was a few months ago.

EdgeCrab is ahead where system shape matters most:

- terminal ownership
- keyboard normalization
- browser/tool integration
- typed boundaries between crates
- binary-oriented distribution
- dedicated security substrate

Hermes is still ahead where historical breadth and research machinery matter most:

- live voice input
- batch evaluation and RL tooling
- long-tail operational maturity
- overall test footprint

## Where Hermes still exceeds EdgeCrab

1. **Research tooling**: Hermes still owns the serious research pipeline with `batch_runner.py`, `mini_swe_runner.py`, `trajectory_compressor.py`, `rl_training_tool.py`, and the `tinker-atropos` integration.
2. **Live voice input**: Hermes has real push-to-talk / microphone capture via `tools/voice_mode.py`; EdgeCrab currently has TTS readback, not live voice capture.
3. **A few mature specialist tools**: Hermes still has a first-class image generation tool and RL-training tool in the runtime surface.
4. **Validation depth**: Hermes carries a much larger test corpus (`hermes-agent/tests` materially exceeds EdgeCrab’s current test tree), which matters for confidence on long-tail behavior.

## Where EdgeCrab exceeds Hermes

1. **TUI architecture**: EdgeCrab’s `ratatui` + `crossterm` stack gives it a stronger full-screen interaction model than Hermes’s `prompt_toolkit` + Rich hybrid.
2. **Browser subsystem**: EdgeCrab’s browser surface is broader in shipped verbs and structurally cleaner than Hermes’s Python browser stack.
3. **Execution backend integration**: EdgeCrab now matches Hermes at six runtime backends, direct plus managed Modal transport variants, direct-Modal filesystem persistence/sync semantics, and remote-process routing, while exceeding it on typed backend integration, unified lifecycle handling, and backend cache coherence.
4. **Security substrate**: EdgeCrab has a dedicated Rust security crate (`edgecrab-security`) with path jail, SSRF checks, injection scanning, and command scanning wired into the runtime surface.
5. **Distribution**: EdgeCrab is easier to ship as a static/bundled artifact across `cargo`, `npm`, `pip`, and Docker.
6. **Migration direction**: EdgeCrab has a dedicated crate for importing Hermes state; Hermes does not provide the reverse path.
7. **Gateway operator surface**: EdgeCrab now matches Hermes on shipped gateway adapters and exceeds it on catalog-driven setup/status coherence across the full channel set.

## High-priority gaps

If the goal is to surpass Hermes rather than merely match it, the priority order is:

1. Add a real voice-input path, not only TTS.
2. Close the research gap with batch / trajectory / evaluation tooling.
3. Decide whether image generation belongs in the core tool surface or as an optional capability.
4. Keep soaking the expanded gateway onboarding and diagnostics path across real operators and long-tail adapters.
5. Keep hardening and soaking the managed Modal, Daytona, and Singularity paths in production-like use.

## Sources audited

- `edgecrab/crates/edgecrab-cli/src/app.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/browser.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/mod.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/mod.rs`
- `edgecrab/crates/edgecrab-gateway/src/lib.rs`
- `edgecrab/crates/edgecrab-state/src/session_db.rs`
- `edgecrab/crates/edgecrab-migrate/src/hermes.rs`
- `edgecrab/crates/edgecrab-types/src/trajectory.rs`
- `edgecrab/crates/edgecrab-core/src/conversation.rs`
- `edgecrab/crates/edgecrab-security/src/lib.rs`
- `hermes-agent/model_tools.py`
- `hermes-agent/tools/browser_tool.py`
- `hermes-agent/tools/environments/`
- `hermes-agent/tools/voice_mode.py`
- `hermes-agent/tools/image_generation_tool.py`
- `hermes-agent/tools/rl_training_tool.py`
- `hermes-agent/tests/`
