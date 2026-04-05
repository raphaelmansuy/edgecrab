---
title: LLM Provider Overview
description: All 11 LLM providers supported by EdgeCrab — GitHub Copilot, OpenAI, Anthropic, Google Gemini, xAI, DeepSeek, Hugging Face, Z.AI, OpenRouter, Ollama, and LM Studio.
sidebar:
  order: 1
---

EdgeCrab supports 11 LLM providers out of the box. Auto-detection finds the right provider from your environment variables — or switch at any time with `--model` or `/model` inside the TUI.

The provider list and detection priority come directly from `crates/edgecrab-cli/src/setup.rs` (`PROVIDER_ENV_MAP`).

---

## Provider Quick Reference

| Priority | Provider | Env var | Best for | Notes |
|----------|----------|---------|----------|-------|
| 1 | `copilot` | `GITHUB_TOKEN` | Free tier, coding | Requires GitHub Copilot subscription |
| 2 | `openai` | `OPENAI_API_KEY` | General purpose | GPT-4.1, GPT-5, o3/o4 |
| 3 | `anthropic` | `ANTHROPIC_API_KEY` | Long context, coding | Claude 4.5 / 4.6 |
| 4 | `gemini` | `GOOGLE_API_KEY` | Multimodal, long context | Gemini 2.5 / 3.x |
| 5 | `xai` | `XAI_API_KEY` | Fast reasoning | Grok 3 / 4 |
| 6 | `deepseek` | `DEEPSEEK_API_KEY` | Code, cost-effective | DeepSeek V3, R1 |
| 7 | `huggingface` | `HUGGING_FACE_HUB_TOKEN` | Open models | Hugging Face Inference API |
| 8 | `zai` | `ZAI_API_KEY` | GLM models | Z.AI / GLM 4.5–5 |
| 9 | `openrouter` | `OPENROUTER_API_KEY` | 600+ models | Single endpoint for many providers |
| — | `ollama` | *(none)* | Offline / local | Requires `ollama serve` on port 11434 |
| — | `lmstudio` | *(none)* | Offline / local | Requires LM Studio server on port 1234 |

> **Auto-detection order**: EdgeCrab checks env vars in priority order (1–9). The first matching key sets the default provider. Local providers (ollama, lmstudio) are available regardless.

---

## Setting Up a Provider

### 1. Set the API Key

Add to your shell profile or `~/.edgecrab/.env`:

```bash
# ~/.edgecrab/.env  <- edgecrab loads this automatically
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
```

> **Note for Gemini**: The env var is `GOOGLE_API_KEY`, not `GEMINI_API_KEY`.

### 2. Set the Active Provider

Either via setup wizard (recommended for first run):

```bash
edgecrab setup
```

Or directly in `~/.edgecrab/config.yaml`:

```yaml
provider: openai
model: gpt-4o
```

### 3. Verify

```bash
edgecrab doctor
# OK  OpenAI  OPENAI_API_KEY set
# OK  Provider ping  openai/gpt-4o -> OK (421 ms)
```

---

## Provider Details

### GitHub Copilot (`copilot`)

Uses your existing GitHub Copilot subscription — no additional billing. Requires a valid `GITHUB_TOKEN` with Copilot access.

```bash
GITHUB_TOKEN=ghp_...
```

Models:
- `gpt-4.1-mini` (default, fast)
- `gpt-4o` (more capable)
- `claude-sonnet-4-5` (when available in Copilot)

### OpenAI (`openai`)

```bash
OPENAI_API_KEY=sk-...
```

Recommended models:
```
openai/gpt-4o              # Best general-purpose
openai/gpt-4.1-mini        # Fast, cost-effective
openai/o3                  # Advanced reasoning
openai/o4-mini             # Fast reasoning
```

### Anthropic (`anthropic`)

```bash
ANTHROPIC_API_KEY=sk-ant-...
```

Recommended models:
```
anthropic/claude-opus-4-5     # Most capable
anthropic/claude-sonnet-4-5   # Balanced
anthropic/claude-haiku-3-5    # Fast, lightweight
```

### Google Gemini (`gemini`)

```bash
GOOGLE_API_KEY=AIza...
```

> **Important**: The env var is `GOOGLE_API_KEY` — not `GEMINI_API_KEY`.

Models:
```
gemini/gemini-2.5-flash        # Fast, capable
gemini/gemini-2.5-pro          # Long context
```

### xAI (`xai`)

```bash
XAI_API_KEY=...
```

Models:
```
xai/grok-3                    # Most capable
xai/grok-3-mini               # Fast, cost-effective
```

### DeepSeek (`deepseek`)

Excellent for code tasks, highly cost-effective.

```bash
DEEPSEEK_API_KEY=...
```

Models:
```
deepseek/deepseek-chat         # V3 — general purpose
deepseek/deepseek-reasoner     # R1 — reasoning
```

### Hugging Face (`huggingface`)

Access open models via the Hugging Face Inference API.

```bash
HUGGING_FACE_HUB_TOKEN=hf_...
```

```bash
edgecrab --model huggingface/meta-llama/Llama-3.3-70B-Instruct "..."
```

### Z.AI (`zai`)

Z.AI provides access to GLM model series.

```bash
ZAI_API_KEY=...
```

Models:
```
zai/glm-4.5                   # Latest GLM
zai/glm-5                     # Most capable GLM
```

### Ollama (local, no API key)

Run any model locally. Requires [Ollama](https://ollama.com) installed and running:

```bash
# Start Ollama (keep this running)
ollama serve

# Pull a model
ollama pull llama3.3
ollama pull codestral
```

No API key needed. EdgeCrab connects to `http://localhost:11434` automatically:

```bash
edgecrab --model ollama/llama3.3 "explain this code"
edgecrab --model ollama/codestral "write a Rust async function"
```

→ [Local Models guide](/providers/local/) for full setup and model recommendations.

### LM Studio (local, no API key)

Download a model in [LM Studio](https://lmstudio.ai) and start its local server (default port 1234):

```bash
edgecrab --model lmstudio/your-loaded-model "..."
```

→ [Local Models guide](/providers/local/).

### OpenRouter (`openrouter`)

Access 600+ models from a single API endpoint and API key.

```bash
OPENROUTER_API_KEY=...
```

```bash
edgecrab --model openrouter/anthropic/claude-opus-4-5 "..."
edgecrab --model openrouter/google/gemini-2.5-flash "..."
edgecrab --model openrouter/meta-llama/llama-3.3-70b-instruct "..."
```

---

## Switching Providers at Runtime

### Command line

```bash
edgecrab --model anthropic/claude-opus-4-5 "refactor this module"
```

### Inside the TUI

```
/model groq/llama-3.3-70b-versatile
```

The switch takes effect immediately — the conversation history carries over, with the new model seeing all previous messages.

---

## Fallback Chain

Configure automatic failover in `config.yaml`:

```yaml
provider: openai
model: gpt-4o
fallback_providers:
  - anthropic/claude-sonnet-4-5
  - ollama/llama3.3
```

If the primary provider returns an error (rate limit, outage), EdgeCrab retries with the next in the chain.

---

## Comparing Models for Coding Tasks

| Task | Recommended |
|------|-------------|
| Large refactor (100+ files) | `anthropic/claude-opus-4-5` |
| Quick one-file fix | `openai/gpt-4.1-mini` or `deepseek/deepseek-chat` |
| Reasoning / complex logic | `deepseek/deepseek-reasoner` or `xai/grok-3` |
| Offline / air-gapped | `ollama/llama3.3` or `ollama/codestral` |
| Maximum model variety | `openrouter/...` (600+ models) |
| Budget-conscious | `deepseek/deepseek-coder-v2` |
| Reasoning-heavy | `openai/o3` |
| European data residency | `mistral/mistral-large-latest` |

---

## Pro Tips

- **Use `/model` in the TUI to experiment**: type `/model deepseek/deepseek-reasoner` mid-session to switch models without losing conversation history.
- **OpenRouter for prototyping**: a single `OPENROUTER_API_KEY` unlocks 600+ models. Iterate fast across different providers before committing to one API key.
- **DeepSeek R1 for hard reasoning**: `deepseek/deepseek-reasoner` matches o3-class reasoning at a fraction of the cost. Ideal for algorithm design and complex debugging.
- **`edgecrab doctor`** shows which providers are configured and their latency. Run it after adding a new key to verify the key works.
- **Fallback chain protects long runs**: configure `fallback_providers` in `config.yaml` so that a rate-limit spike doesn't kill a multi-hour refactor.

---

## FAQ

**Why is my `GOOGLE_API_KEY` not working with Gemini?**
Make sure you're using `GOOGLE_API_KEY` (not `GEMINI_API_KEY`). Also verify the key has the **Generative Language API** enabled in Google Cloud Console.

**Can I use two providers in the same session?**
Not simultaneously, but you can switch mid-session with `/model provider/model-name`. Each turn after the switch uses the new model.

**Does EdgeCrab send conversation history to every provider I've configured?**
No. Only the active provider receives messages. Other API keys are only used if you explicitly switch to that provider.

**How does auto-detection priority work?**
EdgeCrab checks env vars in the order listed in the Provider Quick Reference table. The first key found sets the default provider. If you have multiple keys set, add `provider: <name>` to `config.yaml` to pin the preference.

**Can I use a fine-tuned model?**
Yes — any OpenAI-compatible endpoint accepts a custom `model` name. Set `base_url` and `model.default` in `config.yaml`.

---

## See Also

- [Local Models](/providers/local/) — Ollama and LM Studio setup
- [Environment Variables](/reference/environment-variables/) — all API key env var names
- [Configuration Reference](/reference/configuration/) — `provider`, `model`, and `fallback_providers` config keys
