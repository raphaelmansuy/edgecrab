# Dynamic Model Discovery Overview

This spec set defines how EdgeCrab should discover live model inventories from
providers without weakening the existing static catalog guarantees.

## Goals

- Support live model discovery where the provider exposes a stable model-list
  contract.
- Keep the embedded catalog as the single source of truth for defaults,
  pricing, labels, and fallback behavior.
- Avoid flaky heuristics for classifying arbitrary provider responses.
- Keep provider-specific logic isolated behind adapters.
- Surface discovery state clearly in the TUI and slash commands.

## Non-goals

- Replacing the static catalog.
- Inferring pricing from remote APIs.
- Guessing capabilities from model names for providers that do not publish
  those capabilities.
- Treating every OpenAI-compatible `/v1/models` endpoint as equally trustworthy.

## Provider Support Target

### Implement now

- `openrouter`
- `ollama`
- `lmstudio`
- `google` / `gemini`
- `copilot`

### Implement as gated support

- `bedrock`

### Keep static-only for now

- `anthropic`
- `openai`
- `xai`
- `groq`
- `mistral`
- `deepseek`
- `huggingface`
- `vertexai`
- `zai`

## Why this split

The deciding criterion is not popularity. It is whether EdgeCrab can obtain a
provider-scoped model list through a stable API or SDK contract without
guessing which returned models are usable as chat LLMs.

## ADR Index

- `001_adr_architecture.md`
- `002_adr_provider_matrix.md`
- `003_adr_cache_and_fallback.md`
- `004_adr_tui_behavior.md`
- `005_adr_bedrock.md`
- `006_test_plan.md`
