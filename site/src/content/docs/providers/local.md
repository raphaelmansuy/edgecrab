---
title: Local Models (Mac & Desktop)
description: Run EdgeCrab offline with Ollama, LM Studio, oMLX, MTPLX, llama-server, vLLM-MLX, and mlx_lm.server. Ports, env vars, /endpoint, agent harness, and Apple Silicon tips.
sidebar:
  order: 2
---

EdgeCrab first-classes several **local OpenAI-compatible** servers. No cloud API key is required for local inference (optional bearer keys are supported where the server enforces auth). All local providers share the same agent harness: non-streaming tool turns when needed, no dual-request on timeout, zero-cost pricing, prefix/KV tool-schema freeze, live `/v1/models` discovery, and `/endpoint` base-URL overrides.

---

## Quick chooser

| Goal | Provider id | Typical port | Notes |
|------|-------------|--------------|--------|
| Easiest install | `ollama` | **11434** | Ubiquitous; Metal / MLX path on recent Ollama |
| GUI + MLX speed | `lmstudio` | **1234** | Desktop app; load a model then Start Server |
| Best agent TTFT / SSD KV | `omlx` | **9050** | oMLX menu bar; reads `~/.omlx/settings.json` |
| Speculative decode (MTP) | `mtplx` | settings / **8000** | MTPLX app; often `settings.port` = 8002 |
| Hermes-style GGUF control | `llamacpp` | **8080** | `llama-server` (llama.cpp Metal) |
| Dev batching without oMLX app | `vllm-mlx` | **8000** | May collide with MTPLX — use `/endpoint` |
| Official Apple one-liner | `mlx-lm` | **8080** | `mlx_lm.server` — may collide with llama-server |

```bash
edgecrab --model ollama/llama3.3 "work offline"
edgecrab --model omlx/<id>
edgecrab --model mtplx/<id>
edgecrab --model llamacpp/<id>
edgecrab --model vllm-mlx/<id>
edgecrab --model mlx-lm/<id>
```

**Port collisions:** TCP open ≠ product identity. Prefer `edgecrab doctor` and `GET /v1/models` labeled by configured provider. Override with TUI `/endpoint` (aliases `/endpoints`, `/provider-url`, `/base-url`).

---

## Shared local family behavior

| Behavior | Detail |
|----------|--------|
| Live discovery | `GET {host}/v1/models` (short local cache TTL) |
| Cost | $0 for all local provider ids |
| Tool turns | Prefer non-streaming completion for large tool JSON |
| Timeouts | Do **not** retry transport failures (avoids stacked generations) |
| Prefix / KV | Tool wire schemas frozen after first annotate for the session |
| Base URL | Config `provider_endpoints.<id>.base_url` or env host vars |

Canonical ids in `LOCAL_INFERENCE_PROVIDERS`:  
`ollama`, `lmstudio`, `omlx`, `mtplx`, `vllm`, `vllm-mlx`, `llamacpp`, `mlx-lm`.

---

## Ollama

[Ollama](https://ollama.com) is the easiest way to run LLMs locally.

### Installation

```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh
```

### Start and pull

```bash
ollama serve
# Listening on http://127.0.0.1:11434

ollama pull llama3.3
ollama pull codestral
ollama pull qwen2.5-coder:7b
```

### EdgeCrab

```bash
edgecrab --model ollama/llama3.3
edgecrab setup   # choose ollama
```

```yaml
# config.yaml
provider: ollama
model: llama3.3
```

Env: `OLLAMA_HOST` / `OLLAMA_BASE_URL` (default `http://127.0.0.1:11434`).

---

## LM Studio

1. Download [LM Studio](https://lmstudio.ai)
2. Download a model in the app
3. **Start Server** (default `http://127.0.0.1:1234`)

```bash
edgecrab --model lmstudio/<loaded-model-id>
```

Env: `LMSTUDIO_HOST` / `LMSTUDIO_BASE_URL`.

---

## oMLX (Apple Silicon MLX)

Menu-bar MLX server with multi-model support and agent-friendly KV behavior.

| | |
|--|--|
| Id | `omlx` |
| Default | `http://127.0.0.1:9050` |
| Settings | `~/.omlx/settings.json` (`server.port`, `auth.api_key`) |
| Env | `OMLX_HOST`, `OMLX_BASE_URL`, `OMLX_API_KEY`, `OMLX_TIMEOUT_SECONDS` |

```bash
edgecrab --model omlx/<model-from-/v1/models>
```

---

## MTPLX (native MTP)

Speculative decoding (MTP) on Apple Silicon.

| | |
|--|--|
| Id | `mtplx` |
| Default | `http://127.0.0.1:8000` (often overridden by settings.port, e.g. 8002) |
| Settings | `~/Library/Application Support/MTPLX/settings.json` |
| Env | `MTPLX_HOST`, `MTPLX_BASE_URL`, `MTPLX_API_KEY`, `MTPLX_TIMEOUT_SECONDS` |
| Offline inventory | `~/.mtplx/models` when API is down |

```bash
edgecrab --model mtplx/<id>
```

---

## llama-server (`llamacpp`)

[llama.cpp](https://github.com/ggerganov/llama.cpp) **llama-server** — Metal GGUF path (Hermes Mac guide parity).

| | |
|--|--|
| Id | `llamacpp` |
| Aliases | `llama-server`, `llama.cpp`, `llamacpp-server` |
| Default | `http://127.0.0.1:8080` |
| Env | `LLAMACPP_HOST`, `LLAMA_SERVER_HOST`, `LLAMACPP_BASE_URL`, `LLAMACPP_API_KEY`, `LLAMACPP_TIMEOUT_SECONDS` |

```bash
# example: start llama-server with OpenAI API, then
edgecrab --model llamacpp/<model-id>
```

Optional hard e2e: `LLAMACPP_E2E=1 cargo test -p edgecrab-core --test local_mac_providers_citizenship`.

---

## vLLM-MLX (`vllm-mlx`)

MLX-backed continuous batching / paged-KV style server (developer install, not the oMLX app).

| | |
|--|--|
| Id | `vllm-mlx` |
| Aliases | `vllm_mlx`, `vllmmx` |
| Default | `http://127.0.0.1:8000` |
| Env | `VLLM_MLX_HOST`, `VLLM_MLX_BASE_URL`, `VLLM_MLX_API_KEY`, `VLLM_MLX_TIMEOUT_SECONDS` |

```bash
edgecrab --model vllm-mlx/<id>
```

Optional e2e: `VLLM_MLX_E2E=1`.

---

## mlx-lm (`mlx-lm`)

Official Apple [mlx-lm](https://github.com/ml-explore/mlx-lm) OpenAI server (`mlx_lm.server`).

| | |
|--|--|
| Id | `mlx-lm` |
| Aliases | `mlx_lm`, `mlxlm` |
| Default | `http://127.0.0.1:8080` |
| Env | `MLX_LM_HOST`, `MLX_LM_BASE_URL`, `MLX_LM_API_KEY`, `MLX_LM_TIMEOUT_SECONDS` |

```bash
# python -m mlx_lm.server --model … --port 8080
edgecrab --model mlx-lm/<id>
```

Optional e2e: `MLX_LM_E2E=1`.

---

## `/endpoint` and config overrides

```yaml
# ~/.edgecrab/config.yaml
provider_endpoints:
  llamacpp:
    base_url: "http://127.0.0.1:8081"
  vllm-mlx:
    base_url: "http://127.0.0.1:8010"
  mlx-lm:
    base_url: "http://127.0.0.1:8082"
```

In the TUI: `/endpoint` → select provider → set base URL → probe `/v1/models`.

---

## Doctor

`edgecrab doctor` probes local ports for Ollama, LM Studio, oMLX, MTPLX, and additional TCP opens for llama-server / vLLM-MLX / mlx-lm (labeled cautiously when ports may be shared).

---

## Performance tips

### Context length

Local models often use 8K–128K windows depending on load flags. EdgeCrab compresses history near the limit; you can also cap:

```yaml
session:
  max_context_tokens: 32000
```

### Quantization / Metal

- GGUF via **llama-server** / Ollama / LM Studio  
- MLX via **oMLX**, **MTPLX**, **vLLM-MLX**, **mlx-lm**, LM Studio MLX engine  

### Cold start

First request after load can take seconds to minutes for large models. Subsequent turns benefit from **stable tool schemas** (EdgeCrab freezes them on local providers).

---

## Offline toolset

```bash
edgecrab --model ollama/llama3.3 --toolset file,terminal,memory,skills
```

---

## Architecture (DRY)

```text
LocalOpenAiIdentity { id, default_host, env keys… }
        │
        ▼
LocalOpenAiProvider  (edgequake-llm)  →  OpenAI-compatible HTTP
        │
        ▼
catalog · discovery · local harness · /endpoint  (EdgeCrab)
```

Product-specific settings files remain only where needed (`omlx`, `mtplx`). Thin citizens (`llamacpp`, `vllm-mlx`, `mlx-lm`) use env + `/endpoint` only.

Spec pack (in the EdgeCrab repo): `specs/023-omlx/` — landscape [014](https://github.com/raphaelmansuy/edgecrab/blob/main/specs/023-omlx/014-apple-silicon-local-landscape.md), assessment [015](https://github.com/raphaelmansuy/edgecrab/blob/main/specs/023-omlx/015-wave-def-implementation-assessment.md).

---

## FAQ

**Which should I use on a MacBook for coding agents?**  
Start with **oMLX** (agent TTFT / multi-model) or **Ollama** (simplest). Use **llama-server** for GGUF control; **MTPLX** when you want MTP speed; **vLLM-MLX** / **mlx-lm** for research/dev servers.

**My model outputs garbage with tools.**  
Prefer models with function-calling support. Local harness forces non-streaming tool turns on these providers; still pick tool-capable checkpoints.

**Port 8080 is already in use.**  
Could be llama-server or mlx_lm.server. Use `/endpoint` to point each id at a distinct host/port.

**Does EdgeCrab implement MLX kernels?**  
No. It is a thin OpenAI-compatible client + agent harness around your local server.

---

## See Also

- [Provider Overview](/providers/overview/)
- [Environment Variables](/reference/environment-variables/)
- [Configuration](/user-guide/configuration/)
