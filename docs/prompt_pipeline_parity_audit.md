# Prompt Pipeline Parity Audit

This audit compares EdgeCrab's prompt pipeline against Hermes from first principles:

1. What bytes are actually sent to the model.
2. When compression triggers.
3. What the UI claims about prompt pressure.
4. Whether cached prompt mass is preserved across session restore.
5. Whether tool and skill availability are reflected truthfully in the prompt.

## Fixed in this patch set

### 1. Session restore now preserves the cached system prompt

Problem:
Restored EdgeCrab sessions were discarding the persisted `system_prompt`, forcing a rebuild on the next turn. That breaks cache continuity and makes resumed sessions more expensive and less deterministic than Hermes.

Fix:
`Agent::restore_session()` now reuses the persisted prompt snapshot from SQLite.

Why it matters:
Prompt cache hits only work if the prefix remains byte-stable. Rebuilding the prompt on restore is a structural reliability bug, not a cosmetic optimization issue.

### 2. Compression is now model-aware

Problem:
Compression checks used `CompressionParams::default()` with a hardcoded `128_000` window even when the active model had a different context length.

Fix:
`CompressionParams::from_model_config()` resolves the runtime model context window from `ModelCatalog` and applies the configured threshold, target ratio, and protected tail.

Why it matters:
Compression thresholds must be expressed against the real model limit, otherwise warning and compression behavior drift across providers.

### 3. Compression pressure now estimates the full request, not just chat history

Problem:
EdgeCrab only estimated tokens from `session.messages` before deciding whether to compress. That ignored:

- the cached system prompt
- tool schemas

Fix:
The conversation loop now estimates request pressure from the assembled request payload before the API call.

Why it matters:
The model only sees the full request. Any preflight check that ignores fixed prompt mass will undercount pressure and compress too late.

### 4. Skill summary filtering is now wired into the real prompt path

Problem:
The main conversation path called `load_skill_summary(..., None, None)`, so conditional skill activation by tools and toolsets was effectively disabled in production.

Fix:
The live pipeline now passes:

- active tool names
- active toolset names derived from the real registry

Why it matters:
If the prompt advertises skills that cannot actually run in the current tool configuration, the agent becomes less reliable and more likely to hallucinate capabilities.

### 5. Known-empty tool availability is now treated as authoritative

Problem:
`skill_should_show()` treated `Some([])` like "unknown", so a session with no active tools could still surface skills that explicitly required tools.

Fix:
Conditional skill evaluation now distinguishes:

- `None` => availability unknown, stay permissive
- `Some([])` => availability known empty, hide requirements that cannot be satisfied

Why it matters:
This edge case directly affects restricted environments, delegated agents, and no-tool execution modes.

## Still heavier than Hermes

EdgeCrab is still not at full Hermes parity on prompt weight.

### 1. Context-file loading is broader

EdgeCrab currently walks `AGENTS.md` recursively through the working tree. Hermes is much closer to priority-first project context selection.

Impact:

- larger prompt prefix
- more cache invalidation risk
- higher chance of conflicting instructions in monorepos
- more token volatility between directories

Brutal truth:
This is a real architectural reason EdgeCrab remains heavier than Hermes, even after the accounting fixes.

### 2. Prompt assembly is more feature-rich but also more expensive

EdgeCrab injects more guidance blocks than Hermes. Some are now tool-gated, but the baseline prompt is still broader.

Impact:

- stronger built-in behavior shaping
- higher fixed prompt mass
- more risk that a resumed or delegated session hits pressure earlier

Brutal truth:
EdgeCrab currently prefers feature breadth over prefix minimalism. That is a deliberate tradeoff, but it is not free.

## Reliability stance

Reliability beats raw prompt richness.

The correct order of operations is:

1. Build a truthful prompt.
2. Keep the prefix stable.
3. Measure real request pressure.
4. Compress before overflow.
5. Surface the same semantics in the UI that the runtime actually uses.

Anything else creates flaky behavior by design.

## Test matrix

The following cases should remain covered:

### Unit

- session restore reuses persisted `system_prompt`
- model-aware compression resolves runtime context window
- full-request token estimate increases when system prompt or tool schemas are present
- skill conditions hide tool-gated skills when available tools are known empty
- toolset derivation deduplicates registry matches

### Integration

- resumed session keeps prompt prefix stable across the next turn
- disabled toolsets remove matching skills from the assembled prompt
- delegated/sub-agent sessions only expose skills supported by their reduced toolset
- `@context` expansion uses model-aware context limits
- compression warning and compression trigger fire off full-request pressure, not history-only pressure

### Modality and environment

- CLI/TUI session with full core tools
- CLI/TUI session with tools disabled
- gateway session with platform-specific tool restrictions
- delegated agent session with narrowed toolsets
- execute-code heavy session where tool schemas are large
- terminal-heavy session where history stays short but fixed prompt mass is large
- restored session from SQLite after restart

## Bottom line

EdgeCrab now matches Hermes on the core accounting and cache-preservation mechanics addressed here:

- prompt pressure display semantics
- persisted prompt reuse on restore
- model-aware compression thresholds
- full-request pressure estimation
- conditional skill filtering in the live path

EdgeCrab still exceeds Hermes in prompt breadth, and that remains the main structural reason it can feel heavier. If strict parity is the goal, the next high-value change is to narrow recursive context-file loading or place a hard budget on aggregated context-file injection.
