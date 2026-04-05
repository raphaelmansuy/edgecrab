# Core Agent / Tools Gap Analysis

## Bottom line

The core tools gap is narrower than the older note implied, but the audited conclusion is still the same:

- EdgeCrab exceeds Hermes on browser depth and on the structural quality of the core tool substrate
- Hermes still leads on a small number of specialist first-class tools, mainly image generation and RL training

## Audited facts

Both projects expose the mainstream personal-agent surface in code:

- file read, write, search, and patch
- terminal execution
- process management
- browser automation
- web access
- memory and session search
- MCP integration
- todo and clarify flows
- delegation
- TTS, STT, and vision
- skills and skills hub behavior

The clearest audited tool-surface difference is browser breadth:

- EdgeCrab ships 14 browser verbs
- Hermes ships 11 browser verbs

EdgeCrab adds three browser verbs Hermes does not currently expose at the same first-class level:

- `browser_wait_for`
- `browser_select`
- `browser_hover`

Those extra browser verbs are now exposed consistently across EdgeCrab's core and ACP capability surfaces.

## Where EdgeCrab exceeds

### 1. Browser tooling is materially broader

EdgeCrab's `browser.rs` is not only a reimplementation. It is a larger shipped surface.

It includes:

- navigation
- snapshots
- click
- type
- scroll
- press
- back
- close
- console capture
- image extraction
- vision analysis
- wait-for polling
- select-option control
- hover interaction

From first principles, this matters because browser automation quality is constrained by how many real page-state transitions the tool layer can express without forcing the model into hacks.

### 2. Tool boundaries are easier to audit and extend

EdgeCrab splits tool responsibilities into focused Rust modules such as:

- `file_read.rs`
- `file_write.rs`
- `file_search.rs`
- `file_patch.rs`
- `terminal.rs`
- `process.rs`
- `browser.rs`
- `session_search.rs`
- `execute_code.rs`
- `delegate_task.rs`

This lowers coupling and makes failure domains easier to reason about.

### 3. Tool semantics are more strongly typed

EdgeCrab benefits from explicit schemas, typed errors, and compile-time module boundaries across the tool registry.

That does not automatically make it more mature than Hermes, but it does make the substrate easier to evolve without accidental interface drift.

## Where Hermes still leads

### 1. Image generation is a shipped first-class tool

Hermes exposes `tools/image_generation_tool.py` in the runtime surface.

EdgeCrab has image-related skills, but not an equivalent first-class core runtime tool. If image generation is considered part of the default assistant contract, Hermes is ahead.

### 2. RL training remains a core runtime capability in Hermes

Hermes exposes `tools/rl_training_tool.py`.

EdgeCrab does not currently ship an equivalent core tool. This is a real runtime-surface gap, not just a research-docs gap.

### 3. Some long-tail utility surfaces are still broader in Hermes

Hermes still carries older specialist utility surfaces such as:

- image generation
- RL training
- additional website and OAuth helper modules

These are not enough to reverse EdgeCrab's structural advantages, but they do keep Hermes broader in the long tail.

## Re-assessed gap verdict

From first principles, the important question is not whether EdgeCrab has more files or more modules. The real question is whether the default runtime can express the state transitions an agent actually needs, with boundaries that remain auditable under change.

On that bar, EdgeCrab is ahead in the browser-critical path and in typed core-tool architecture.

The remaining meaningful gaps are narrower:

1. decide whether first-class image generation belongs in the default runtime rather than only adjacent skills/stubs
2. decide whether RL training belongs in the default runtime contract
3. keep pushing browser execution quality, especially dynamic-page synchronization and stateful interactions, where EdgeCrab already has real leverage over Hermes

## Sources audited

- `edgecrab/crates/edgecrab-tools/src/tools/mod.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/browser.rs`
- `hermes-agent/model_tools.py`
- `hermes-agent/tools/browser_tool.py`
- `hermes-agent/tools/image_generation_tool.py`
- `hermes-agent/tools/rl_training_tool.py`
