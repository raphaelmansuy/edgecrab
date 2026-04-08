# Smart Model Routing 🦀

> **Verified against:** `crates/edgecrab-core/src/model_router.rs` ·
> `crates/edgecrab-core/src/model_catalog.rs`

---

## Why smart routing exists

Running Claude Opus on "what time is it?" costs 5× as much as Claude Haiku
and takes 2–3× longer. But running Haiku on "refactor this 1,000-line module
with proper error handling" produces significantly worse results.

Smart routing classifies each user turn and selects the most cost-effective
model. The decision is conservative: only obviously simple messages route to
the cheaper model. When in doubt, the primary model is used.

🦀 *`hermes-agent` and OpenClaw used one model for everything — every "what time is it?"
cost the same as a full refactor. EdgeCrab picks the right weapon for the fight.*

---

## Route types

```rust
// model_router.rs
pub enum TurnRoute {
    Primary,   // use the configured primary model
    Cheap,     // use the configured cheap_model
    Fallback,  // use the fallback model (if primary fails)
}
```

---

## Classification algorithm

`classify_message(msg: &str, thresholds: &RoutingThresholds) -> TurnRoute`

```
  Input: user message string

  Step 1 — Length checks
    chars > 160  → Primary
    words > 28   → Primary
    newlines > 1 → Primary (multiline → complex)

  Step 2 — Structural checks
    contains code fences (```)  → Primary
    contains inline code (`)    → Primary
    contains URL (http://)      → Primary

  Step 3 — Keyword scan
    message (lowercased) contains any COMPLEX_KEYWORD → Primary

  Step 4 — Default
    none of the above fired → Cheap
```

---

## Complex keywords (from source)

```rust
const COMPLEX_KEYWORDS: &[&str] = &[
    // debugging and fixing
    "debug", "fix", "bug", "traceback", "exception", "error",
    // coding
    "implement", "refactor", "patch", "code", "function", "class",
    "struct", "enum", "compile", "build",
    // analysis
    "analyze", "analyse", "architecture", "design", "compare",
    "benchmark", "optimize", "optimise", "review",
    // tools and execution
    "terminal", "shell", "tool", "docker", "kubernetes",
    "pytest", "test", "deploy", "ci", "pipeline",
    // planning
    "plan", "delegate", "subagent", "cron",
    // more technical keywords...
];
```

---

## Routing decision flow

```
  User types message
        │
        ▼
  SmartRoutingConfig::enabled?
        │
        ├─ NO  → always use Primary model
        │
        └─ YES
              │
              ▼
        classify_message()
              │
              ├─ TurnRoute::Primary → use config.model
              │
              └─ TurnRoute::Cheap   → use config.smart_routing.cheap_model
                                        fallback to Primary if cheap not configured
```

---

## Configuration

In `~/.edgecrab/config.yaml`:

```yaml
model:
  name: anthropic/claude-opus-4-20250514   # Primary model
  smart_routing:
    enabled: true
    cheap_model: anthropic/claude-haiku-4-5-20251001
    # Optional fallback if primary fails:
    fallback_model: anthropic/claude-sonnet-4-20250514
```

Or via CLI:
```sh
edgecrab --model anthropic/claude-opus-4-20250514 "refactor auth.rs"
```

The `--model` flag overrides smart routing for the entire session.

Inside the TUI:

```sh
/cheap_model                  # open the same selector-style UX as /model
/cheap_model status           # inspect current smart-routing state
/cheap_model off              # disable cheap-model routing and clear its override
/config cheap                 # jump there from the config center
```

The cheap-model selector persists `model.smart_routing.enabled` and
`model.smart_routing.cheap_model` back to `config.yaml`.

## Related multi-model defaults

EdgeCrab also exposes a separate top-level `moa` block for the
`moa` tool (legacy alias: `mixture_of_agents`):

```yaml
moa:
  enabled: true
  aggregator_model: anthropic/claude-opus-4.6
  reference_models:
    - anthropic/claude-opus-4.6
    - google/gemini-2.5-pro
    - openai/gpt-4.1
    - deepseek/deepseek-r1
```

These defaults are used when the MoA tool call omits explicit
`aggregator_model` or `reference_models` arguments. When `moa.enabled` is
`false`, the tool is hidden from the model and direct calls are rejected. MoA
also depends on toolset policy: `tools.enabled_toolsets` / `tools.disabled_toolsets`
must still expose the `moa` toolset. `/moa on` repairs literal whitelist and
blacklist entries when possible and reports when a broader alias still blocks
the tool. The TUI exposes:

```sh
/moa status
/moa on
/moa off
/moa aggregator
/moa experts
/moa add
/moa remove
/config moa
```

Editing the aggregator or reference roster normalizes provider aliases,
deduplicates the roster, and re-enables MoA for future turns. During execution,
the active chat model is also used as a safety net: it is appended as an
implicit last-chance expert when needed, and aggregation falls back to the
current chat model before failing the tool outright. `/moa reset` now writes a
safe baseline for the current chat model instead of restoring a brittle
cross-provider roster blindly.

---

## Model catalog

`ModelCatalog` is the single source of truth for available models and providers.
It loads from two sources:

```
  1. Embedded defaults: model_catalog_default.yaml (compiled into binary)
  2. User overrides:    ~/.edgecrab/models.yaml     (merged on top)

  Stored in: OnceLock<RwLock<CatalogData>>
  Can be refreshed via: ModelCatalog::reload()
```

Key types:

```rust
pub struct ModelEntry {
    pub id: String,           // "anthropic/claude-opus-4-20250514"
    pub name: String,         // "Claude Opus 4"
    pub provider: String,     // "anthropic"
    pub tier: ModelTier,      // Fast | Balanced | Powerful
    pub context_window: usize,
    pub pricing: PricingPair, // input_per_million, output_per_million (USD)
    pub supports_tools: bool,
    pub supports_vision: bool,
    pub supports_reasoning: bool,
}

pub enum ModelTier { Fast, Balanced, Powerful }
```

---

## Routing thresholds

```rust
pub struct RoutingThresholds {
    pub max_chars:    usize, // default 160
    pub max_words:    usize, // default 28
    pub max_newlines: usize, // default 1
}
```

These are conservative by design. A 160-character message is roughly two
medium sentences — anything longer is likely nuanced enough to warrant the
primary model.

---

## Example routing decisions

| Message | Route | Reason |
|---|---|---|
| `"what time is it?"` | Cheap | 20 chars, no keywords |
| `"list files in current dir"` | Cheap | 26 chars, no complex keywords |
| `"fix the bug in auth.rs line 42"` | Primary | contains `fix` and `bug` |
| `"refactor the auth module"` | Primary | contains `refactor` |
| `"hello"` | Cheap | 5 chars, no keywords |
| `"implement a redis cache for sessions"` | Primary | contains `implement` |
| `"explain this code:\n```rust\n...`"` | Primary | has newline + code fence |

---

## Fallback routing

If the primary model fails (e.g., rate limit, quota exceeded):

```
  primary fails with AgentError::RateLimited or AgentError::Llm
        │
        ▼
  fallback_route(config)
        │
        ├─ fallback_model configured? → use fallback
        └─ no fallback?               → propagate error to caller
```

---

## Tips

> **Tip: Set `smart_routing.cheap_model` to a fast model from the same provider.**
> Cross-provider routing (e.g. Anthropic for primary, Together.ai for cheap) works
> if both use OpenRouter as the proxy. Same-provider routing avoids credential setup.

> **Tip: Disable smart routing for reproducible benchmarks.**
> `smart_routing.enabled: false` forces every turn through the primary model, giving
> consistent output quality for A/B testing.

> **Tip: Add domain-specific keywords to trigger primary routing.**
> If your project uses `"query"` as a complex operation keyword, add it to a custom
> `routing_keywords` list in config. The default list covers general software
> engineering but not every domain.

---

## FAQ

**Q: Does smart routing affect the conversation history?**
No. The model name is recorded in `ConversationResult::model` for the turn,
but the message history uses the same format regardless of which model handled
the turn.

**Q: Can I see which model handled each turn?**
Yes. The TUI status bar shows the current model. `ConversationResult::model`
records the name for each turn. Session analytics in the SQLite database
include per-turn model breakdowns.

**Q: What if the cheap model lacks tool support?**
`ModelEntry::supports_tools` is checked before routing. If the cheap model
does not support tools and the turn requires them, routing falls back to Primary
automatically.

---

## Cross-references

- How routing integrates in the loop → [Conversation Loop](./002_conversation_loop.md)
- Model config in `AppConfig` → [Config and State](../009_config_state/001_config_state.md)
- Model pricing and cost tracking → [Data Models](../010_data_models/001_data_models.md)
- MoA tool behavior → [Tool Catalogue](../004_tools_system/002_tool_catalogue.md)
