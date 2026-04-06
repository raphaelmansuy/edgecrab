---
title: LLM Provider Overview
description: All 14 LLM providers supported by EdgeCrab — GitHub Copilot, OpenAI, Anthropic, Google Gemini, Vertex AI, xAI Grok, DeepSeek, Mistral, Groq, Hugging Face, Z.AI, OpenRouter, Ollama, and LM Studio.
sidebar:
  order: 1
---

EdgeCrab supports **14 LLM providers** out of the box (12 cloud, 2 local). Over 200 models compiled in, with user override via `~/.edgecrab/models.yaml`. Auto-detection finds the right provider from your environment variables — or switch at any time with `--model` or `/model` inside the TUI.

---

## Provider Quick Reference

| Priority | Provider | Env var | Notable Models |
|----------|----------|---------|----------------|
| 1 | `copilot` | `GITHUB_TOKEN` | GPT-4.1-mini, GPT-4.1 — free with GitHub Copilot |
| 2 | `openai` | `OPENAI_API_KEY` | GPT-4.1, GPT-5, o3, o4-mini |
| 3 | `anthropic` | `ANTHROPIC_API_KEY` | Claude Opus 4.6, Sonnet 4.6, Haiku 4.5 |
| 4 | `google` | `GOOGLE_API_KEY` | Gemini 2.5 Pro, Gemini 2.5 Flash |
| 5 | `vertexai` | `GOOGLE_APPLICATION_CREDENTIALS` | Gemini via Google Cloud |
| 6 | `xai` | `XAI_API_KEY` | Grok 3, Grok 4 |
| 7 | `deepseek` | `DEEPSEEK_API_KEY` | DeepSeek V3, DeepSeek R1 |
| 8 | `mistral` | `MISTRAL_API_KEY` | Mistral Large, Mistral Small |
| 9 | `groq` | `GROQ_API_KEY` | Llama 3.3 70B, Gemma2 (blazing fast inference) |
| 10 | `huggingface` | `HUGGING_FACE_HUB_TOKEN` | Any HF Inference API model |
| 11 | `zai` | `ZAI_API_KEY` | Z.AI / GLM series |
| 12 | `openrouter` | `OPENROUTER_API_KEY` | 600+ models via one endpoint |
| — | `ollama` | *(none)* | Any model — `ollama serve` on port 11434 |
| — | `lmstudio` | *(none)* | Any model — LM Studio on port 1234 |

> **Auto-detection order**: EdgeCrab checks env vars in priority order (1–12). The first matching key sets the default provider. Local providers (ollama, lmstudio) are available regardless.

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

### Google Gemini (`google`)

```bash
GOOGLE_API_KEY=AIza...
```

> **Important**: The env var is `GOOGLE_API_KEY` — not `GEMINI_API_KEY`.

Models:
```
google/gemini-2.5-flash        # Fast, capable
google/gemini-2.5-pro          # Long context, advanced reasoning
```

### Vertex AI (`vertexai`)

Access Gemini models via Google Cloud with enterprise billing and data residency.

```bash
GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
# or: use Application Default Credentials (gcloud auth application-default login)
```

Models: same as `google` provider, routed through Vertex AI API endpoint.

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
deepseek/deepseek-reasoner     # R1 — advanced reasoning
```

### Mistral (`mistral`)

European-headquartered provider with strong multilingual support and GDPR data residency options.

```bash
MISTRAL_API_KEY=...
```

Models:
```
mistral/mistral-large-latest   # Most capable
mistral/mistral-small-latest   # Fast, cost-effective
mistral/codestral-latest       # Code-focused
```

### Groq (`groq`)

Ultra-fast inference via custom LPU chips. Lowest latency of any cloud provider.

```bash
GROQ_API_KEY=...
```

Models:
```
groq/llama-3.3-70b-versatile   # Best balance of speed + quality
groq/llama-3.1-8b-instant      # Extremely fast, lightweight
groq/gemma2-9b-it              # Google Gemma2 via Groq
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
| Large refactor (100+ files) | `anthropic/claude-opus-4-6` |
| Quick one-file fix | `groq/llama-3.3-70b-versatile` or `openai/gpt-4.1-mini` |
| Reasoning / complex logic | `deepseek/deepseek-reasoner` or `openai/o3` |
| Offline / air-gapped | `ollama/llama3.3` or `ollama/codestral` |
| Maximum model variety | `openrouter/...` (600+ models) |
| Budget-conscious | `deepseek/deepseek-chat` or `groq/llama-3.1-8b-instant` |
| Lowest latency | `groq/llama-3.3-70b-versatile` (LPU hardware) |
| European data residency | `mistral/mistral-large-latest` |
| Code generation | `deepseek/deepseek-chat` or `mistral/codestral-latest` |

---

## Pro Tips

- **Use `/model` in the TUI to experiment**: type `/model groq/llama-3.3-70b-versatile` mid-session to switch models without losing conversation history.
- **Groq for speed-sensitive tasks**: Groq's LPU chips deliver 300+ tokens/second — ideal for quick iterations and interactive use where waiting 5 seconds per response breaks flow.
- **OpenRouter for prototyping**: a single `OPENROUTER_API_KEY` unlocks 600+ models. Iterate fast across different providers before committing to one API key.
- **DeepSeek R1 for hard reasoning**: `deepseek/deepseek-reasoner` matches o3-class reasoning at a fraction of the cost. Ideal for algorithm design and complex debugging.
- **Mistral for European compliance**: data stays in EU datacenters. Use `mistral/codestral-latest` for code tasks with GDPR requirements.
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
