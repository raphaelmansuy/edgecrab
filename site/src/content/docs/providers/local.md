---
title: Local Models (Ollama & LM Studio)
description: Run EdgeCrab completely offline with Ollama or LM Studio. Model recommendations, configuration, performance tips, and GPU acceleration.
sidebar:
  order: 2
---

EdgeCrab works with local LLMs through Ollama and LM Studio — no API keys, no internet, no billing. This is the recommended setup for air-gapped environments, privacy-sensitive work, or learning without cost.

---

## Ollama

[Ollama](https://ollama.com) is the easiest way to run LLMs locally. It handles model downloads, quantization, GPU acceleration, and serves a local OpenAI-compatible API.

### Installation

```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.com/install.sh | sh

# Windows
# Download from https://ollama.com/download
```

### Start the Server

```bash
ollama serve
# Listening on http://127.0.0.1:11434
```

Keep this running in a terminal (or as a background service) while using EdgeCrab.

### Pull Models

```bash
ollama pull llama3.3          # General-purpose flagship (4.9 GB, 4-bit)
ollama pull codestral         # Code-specialized model (4.1 GB)
ollama pull gemma3:27b        # Google Gemma 3 27B (16 GB)
ollama pull qwen2.5-coder:7b  # Qwen 2.5 Coder 7B (4.4 GB, fast)
ollama pull phi4              # Microsoft Phi-4 (8.1 GB)
```

Check available models:

```bash
ollama list
```

### Using with EdgeCrab

```bash
# One-off session
edgecrab --model ollama/llama3.3

# Make Ollama the default
edgecrab setup   # Choose "ollama" when prompted
```

In `config.yaml`:

```yaml
provider: ollama
model: llama3.3
```

### Recommended Models by Use Case

| Use Case | Model | Size | Notes |
|----------|-------|------|-------|
| General coding | `codestral` | 4.1 GB | Best code model for Ollama |
| General purpose | `llama3.3` | 4.9 GB | Meta's latest, excellent quality |
| Fast, lightweight | `qwen2.5-coder:7b` | 4.4 GB | Great performance for size |
| High quality (needs 16+ GB) | `gemma3:27b` | 16 GB | Excellent reasoning |
| Fastest response | `phi4` | 8.1 GB | Microsoft's efficient model |

### GPU Acceleration

Ollama automatically uses your GPU if available:
- **Apple Silicon (M1/M2/M3/M4)**: uses Metal via llama.cpp — full GPU acceleration
- **NVIDIA**: CUDA support automatic with compatible drivers
- **AMD**: ROCm support on Linux

Check GPU utilization:

```bash
# macOS
sudo powermetrics --samplers gpu_power -i1000 -n1

# Linux (NVIDIA)
nvidia-smi
```

---

## LM Studio

[LM Studio](https://lmstudio.ai) is a desktop application for downloading and running local models with a GUI.

### Setup

1. Download LM Studio from [lmstudio.ai](https://lmstudio.ai)
2. Install and open it
3. Search for and download a model (e.g. "Llama 3.3", "Codestral")
4. Click **"Start Server"** in the Local Server tab

The server starts on `http://localhost:1234` with an OpenAI-compatible API.

### Using with EdgeCrab

```bash
# Use whatever model is currently loaded in LM Studio
edgecrab --model lmstudio/local-model
```

In `config.yaml`:

```yaml
provider: lmstudio
model: local-model    # Any string — LM Studio ignores the model name and uses the loaded model
```

---

## Performance Tips

### Context Length

Local models typically support 8K–32K context windows (much less than cloud models). EdgeCrab automatically compresses history when approaching the limit, but you can also reduce it:

```yaml
session:
  max_context_tokens: 8000    # Match your local model's context window
```

### Model Quantization

Smaller quantizations (Q4, Q5) are faster with slightly lower quality. Larger (Q8, fp16) are slower but more accurate. For coding tasks, Q5_K_M or Q6_K are a good balance.

### Threads

For CPU-only inference, set the number of threads to your CPU core count:

```bash
# Ollama — set via environment
OLLAMA_NUM_THREAD=8 ollama serve
```

### Cold Start

Ollama keeps the model loaded in memory after first use. Cold start (first request after pulling the model) can take 5–30 seconds depending on model size and hardware. Subsequent requests are fast.

---

## Offline Mode

When using local models, you may want to disable web tools to ensure no outbound connections:

```bash
edgecrab --model ollama/llama3.3 --toolset file,terminal,memory,skills
```

Or in `config.yaml`:

```yaml
tools:
  enabled:
    - file
    - terminal
    - memory
    - skills
    # web and session omitted
```

---

## Pro Tips

- **Choose model size by VRAM/RAM**: A rule of thumb is 6 GB VRAM for 7B models at Q4, 12 GB for 13B, 24 GB for 34B. On CPU, double these values as RAM requirements.
- **Set a low max_iterations for local testing**: Local models are slower, so `EDGECRAB_MAX_ITERATIONS=10` keeps experiments snappy while you pick the right model.
- **Use Ollama's model aliases**: `ollama pull codestral:latest` pins to the latest release. Omit the tag if you want `ollama pull codestral` to auto-upgrade.
- **LM Studio model name doesn't matter**: EdgeCrab sends `model: local-model` but LM Studio uses whatever model it has loaded. Only the server URL matters.
- **Keep the Ollama server warm**: Use `OLLAMA_KEEP_ALIVE=24h ollama serve` so the model stays loaded in VRAM between sessions.
- **Monitor GPU memory**: `ollama ps` shows which models are loaded and their VRAM usage.

---

## FAQ

**Which model should I start with?**
For coding: `codestral` (Ollama) is the best all-round local coding model at 4 GB. For general tasks: `llama3.3` gives the best quality. For low-RAM machines: `qwen2.5-coder:7b` fits in 4 GB.

**My model fits in RAM but generation is slow. Why?**
The model is running on CPU because the GPU isn't detected. Check `ollama ps` — a `(CPU)` label means no GPU offloading. Install CUDA drivers (NVIDIA) or verify Metal is enabled (macOS).

**Can I use a local model for some tasks and a cloud model for others?**
Yes. Use `--model ollama/codestral` for local tasks and override per-session. No global config change needed.

**Does EdgeCrab support OpenAI-format custom base URLs?**
Yes. Set `base_url: http://localhost:11434/v1` in `config.yaml` to point at any OpenAI-compatible local server.

**My local model outputs garbage with tool calls. What's wrong?**
Not all local models support function calling (tool use). Stick to models with `:tools` variants in Ollama (e.g. `llama3.1:8b-instruct-q4_K_M`) or models tested with OpenAI function calling format.

---

## See Also

- [Provider Overview](/providers/overview/) — full list of supported providers
- [Environment Variables](/reference/environment-variables/) — `EDGECRAB_MODEL` and `OLLAMA_*` vars
- [Offline Mode toolset config](/reference/configuration/) — disable web tools for air-gapped use
