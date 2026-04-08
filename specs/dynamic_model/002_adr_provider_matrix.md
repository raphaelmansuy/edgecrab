# ADR 002: Provider Support Matrix

## Status

Accepted

## Decision rule

A provider gets live discovery only if at least one of these is true:

1. It exposes a provider-native model listing API with stable semantics.
2. It exposes a typed SDK method already used by EdgeCrab or `edgequake-llm`.
3. EdgeCrab can determine that returned models are chat-usable without model-name
   guesswork.

If none of the above are true, the provider remains static-only.

## Matrix

| Provider | Live discovery | Basis |
|----------|----------------|-------|
| `openrouter` | Yes | Native provider API via `OpenRouterProvider::list_models()` |
| `ollama` | Yes | Native provider API via `OllamaProvider::list_models()` |
| `lmstudio` | Yes | Local `/v1/models` endpoint, provider-scoped and operationally safe |
| `google` / `gemini` | Yes | Native provider API via `GeminiProvider::list_models()` |
| `copilot` | Yes | Native provider API via `VsCodeCopilotProvider::list_models()` |
| `bedrock` | Gated | AWS control-plane SDK required; see ADR 005 |
| `anthropic` | No | No comparable public list endpoint with usable chat-scoped semantics |
| `openai` | No | `/v1/models` is too broad for chat selection without filtering heuristics |
| `xai` | No | No adopted typed listing path in current stack |
| `groq` | No | Same issue as generic OpenAI-compatible listing |
| `mistral` | No | Same issue as generic OpenAI-compatible listing |
| `deepseek` | No | Same issue as generic OpenAI-compatible listing |
| `huggingface` | No | Discovery semantics too broad for direct chat selection |
| `vertexai` | No | Discovery is region/project scoped and not exposed through current stack cleanly |
| `zai` | No | No adopted typed listing path in current stack |

## Comparison with EdgeCode

EdgeCode already uses an adapter pattern for dynamic providers. That part is
correct and should be mirrored.

EdgeCode also relies on more inference inside adapter conversions. EdgeCrab
should be stricter:

- prefer provider-native contracts over guessed capability inference
- use static metadata when available
- avoid expanding support to providers that only expose ambiguous generic
  `/models` responses

## Consequences

- EdgeCrab advertises fewer live-discovery providers than a naive
  OpenAI-compatible sweep.
- The result is more honest and less failure-prone in the model selector.
