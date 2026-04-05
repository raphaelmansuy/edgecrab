# edgecrab-cli (PyPI)

> **EdgeCrab** — Super Powerful Personal Assistant inspired by **NousHermes** and **OpenClaw**.  
> Blazing-fast TUI · ReAct tool loop · Multi-provider LLM · ACP protocol · Single 15 MB static binary.

[![PyPI](https://img.shields.io/pypi/v/edgecrab-cli.svg)](https://pypi.org/project/edgecrab-cli/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/raphaelmansuy/edgecrab/blob/main/LICENSE)

---

## Installation

```bash
pip install edgecrab-cli
```

On first run, the package automatically downloads the correct pre-built Rust binary for
your platform (macOS arm64/x64, Linux x64/arm64, Windows x64) from
[GitHub Releases](https://github.com/raphaelmansuy/edgecrab/releases).

No Rust, GCC, or build tools are required.

---

## Quick Start

```bash
# First-run setup wizard — detects API keys, writes ~/.edgecrab/config.yaml
edgecrab setup

# Verify your environment
edgecrab doctor

# Start the interactive TUI
edgecrab

# One-shot query (pipe-friendly)
edgecrab "summarise the git log for today"

# Pick a specific LLM provider
edgecrab --model anthropic/claude-opus-4 "explain this codebase"

# Quiet / pipe mode
edgecrab --quiet "write a Rust hello-world"
```

---

## Why EdgeCrab?

EdgeCrab is a **Rust-native** autonomous coding agent and personal assistant, inspired by
the reasoning depth of **NousHermes** and the tool-use power of **OpenClaw**.

| Feature | Detail |
|---------|--------|
| **Single binary** | 15 MB static binary, < 50 ms startup, ~15 MB resident memory |
| **Multi-provider LLM** | Copilot · OpenAI · Anthropic · Gemini · xAI · DeepSeek · Ollama |
| **ReAct tool loop** | File, terminal, web search, memory, process, skill tools |
| **ratatui TUI** | 60 fps capable terminal UI with streaming output |
| **ACP protocol** | Built-in JSON-RPC 2.0 stdio adapter for VS Code Copilot |
| **Built-in security** | Path safety, SSRF protection, command scanning, output redaction |

---

## Supported Providers

`copilot` · `openai` · `anthropic` · `gemini` · `xai` · `deepseek` · `huggingface` · `zai` · `openrouter` · `ollama` · `lmstudio`

---

## Alternative Installation Methods

| Method | Command |
|--------|---------|
| **PyPI** | `pip install edgecrab-cli` |
| **npm** | `npm install -g edgecrab-cli` |
| **Cargo** | `cargo install edgecrab-cli` |
| **Docker** | `docker pull ghcr.io/raphaelmansuy/edgecrab:latest` |
| **Pre-built binary** | [GitHub Releases](https://github.com/raphaelmansuy/edgecrab/releases) |

---

## Documentation

Full docs: **[edgecrab.com](https://www.edgecrab.com)**

- [Quick Start](https://www.edgecrab.com/getting-started/quick-start/)
- [Installation Guide](https://www.edgecrab.com/getting-started/installation/)
- [CLI Commands Reference](https://www.edgecrab.com/reference/cli-commands/)

---

## License

MIT © [Raphael Mansuy](https://github.com/raphaelmansuy)
