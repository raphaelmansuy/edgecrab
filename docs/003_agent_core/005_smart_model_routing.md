# 003.005 — Smart Model Routing

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 003.001 Agent Struct](001_agent_struct.md) | [→ 013 Library Selection](../013_library_selection/001_library_selection.md)
> **Source**: `edgecrab-core/src/model_router.rs` — verified against real implementation

## 1. Routing Purpose

Smart routing selects the optimal model for each turn, saving cost on simple messages while preserving quality for complex ones:

```
user_message
     │
     ▼
 ┌────────────────────────────┐
 │  classify_message(text)    │
 │  None = simple             │
 │  Some(reason) = complex    │
 └──────────┬─────────────────┘
      simple│       │complex
            ▼       ▼
    cheap_model  primary_model
            │       │
            ▼       ▼
     ┌──────────────────┐
     │   try provider   │
     │   on error →     │
     │   try fallback   │
     └──────────────────┘
```

## 2. Complexity Classification

The router uses a **conservative** approach: if in doubt, keep the primary (strong) model. Only short, keyword-free, single-line plain-text messages qualify as "simple".

```rust
// edgecrab-core/src/model_router.rs

/// Classify a user message as simple (None) or complex (Some(reason)).
pub fn classify_message(
    text: &str,
    thresholds: &RoutingThresholds,
) -> Option<ComplexityReason>
```

### ComplexityReason Enum

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComplexityReason {
    TooLong,
    TooManyWords,
    MultiLine,
    ContainsCodeFence,
    ContainsInlineCode,
    ContainsUrl,
    ContainsComplexKeyword(String),
    Empty,
}
```

### Routing Thresholds

```rust
#[derive(Debug, Clone)]
pub struct RoutingThresholds {
    pub max_chars: usize,     // default: 160
    pub max_words: usize,     // default: 28
    pub max_newlines: usize,  // default: 1
}
```

### Complex Keywords (word-boundary match)

```rust
const COMPLEX_KEYWORDS: &[&str] = &[
    "debug", "debugging", "implement", "implementation", "refactor", "patch",
    "traceback", "stacktrace", "exception", "error", "analyze", "analysis",
    "investigate", "architecture", "design", "compare", "benchmark",
    "optimize", "optimise", "review", "terminal", "shell", "tool", "tools",
    "pytest", "test", "tests", "plan", "planning", "delegate", "subagent",
    "cron", "docker", "kubernetes", "code", "function", "class", "struct",
    "enum", "compile", "build", "deploy", "fix", "bug",
];
```

Keywords are matched by word boundary (not substring) — `"error"` in `"no error"` matches, but `"error"` in `"terrorism"` does not.

## 3. SmartRoutingConfig

```rust
pub struct SmartRoutingConfig {
    pub enabled: bool,
    pub cheap_model: Option<String>,
    pub cheap_base_url: Option<String>,
    pub cheap_api_key_env: Option<String>,
    pub thresholds: RoutingThresholds,
}
```

## 4. Turn Route Resolution

```rust
/// Result of routing a user message to a specific model + provider.
pub struct TurnRoute {
    pub model: String,
    pub api_mode: ApiMode,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub label: String,        // human-readable ("primary" / "cheap" / "fallback")
    pub is_primary: bool,
}

/// Resolve which model/provider to use for this turn.
pub fn resolve_turn_route(
    user_message: &str,
    model_config: &ModelConfig,
    smart_routing: &SmartRoutingConfig,
) -> TurnRoute
```

> **Note**: The routing system uses standalone functions, not a `ModelRouter` struct. There is no struct-level state — routing is pure and stateless per invocation.

## 5. Fallback Chain

```yaml
# config.yaml
model:
  default: "anthropic/claude-opus-4.6"
  fallback:
    model: "openai/gpt-4o"
    api_key_env: "OPENAI_API_KEY"
```

**Eager fallback policy** (matching hermes-agent v0.4.0):

| Trigger | Action |
|---------|--------|
| 429 Too Many Requests | Activate fallback immediately |
| 503 Service Unavailable | Activate fallback immediately |
| 3× consecutive errors | Activate fallback |
| Fallback activated | Stays active for rest of session |

## 6. ModelCatalog Integration

Model capabilities are sourced from the compiled-in `model_catalog_default.yaml` (13 providers × N models) and optionally supplemented by `custom_models.yaml`:

```
~/.edgecrab/
└── custom_models.yaml    ← user-added models, merged at startup

ModelCatalog queries:
├── context_length(model) → u32
├── supports_tools(model) → bool
├── supports_streaming(model) → bool
├── supports_reasoning(model) → bool
└── cost_per_token(model) → (f32, f32)
```

Used by `CompressionParams` to set the context window for threshold calculation, and by pricing estimates in the CLI status bar.

## 7. Provider Resolution

```rust
// Model strings use "provider/model" format:
"anthropic/claude-opus-4.6"     → AnthropicProvider
"openai/gpt-4.1-mini"           → OpenAIProvider
"copilot/gpt-4.1-mini"          → VsCodeCopilotProvider
"openrouter/..."                → OpenRouterProvider
"ollama/llama3"                 → OllamaProvider (local)

// Provider factory (edgequake-llm) handles instantiation:
if let Some((provider_name, model_name)) = model.split_once('/') {
    ProviderFactory::create(canonical_name, model_name, ...)
} else {
    ProviderFactory::from_env()  // env-based auto-detect
}
```

## 8. API Mode Detection

EdgeCrab unifies API modes via edgequake-llm's `LLMProvider` trait — all providers expose the same interface:

| API Mode | When Used | Provider |
|----------|----------|----------|
| `ChatCompletions` | Default — OpenRouter, OpenAI, most providers | OpenAI SDK |
| `AnthropicMessages` | Direct Anthropic API (detected from base_url) | Anthropic SDK |
| `CodexResponses` | OpenAI Codex API | OpenAI Responses SDK |

API mode detection becomes provider selection — no runtime mode switching needed.
