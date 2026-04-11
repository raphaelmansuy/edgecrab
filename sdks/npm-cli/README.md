# edgecrab-cli (npm)

> **EdgeCrab** — Super Powerful Personal Assistant inspired by **NousHermes** and **OpenClaw**.  
> Blazing-fast TUI · ReAct tool loop · Multi-provider LLM · ACP protocol · Rust-native prebuilt binary.

[![npm](https://img.shields.io/npm/v/edgecrab-cli.svg)](https://www.npmjs.com/package/edgecrab-cli)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](https://github.com/raphaelmansuy/edgecrab/blob/main/LICENSE)

---

## Installation

```bash
# Global install (recommended) — adds `edgecrab` to your PATH
npm install -g edgecrab-cli

# Or use without a global install
npx edgecrab-cli
```

The postinstall script automatically downloads the correct pre-built Rust binary for
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

# Stream output
edgecrab --quiet "write a Rust hello-world"
```

---

## Why EdgeCrab?

EdgeCrab is a **Rust-native** autonomous coding agent and personal assistant, inspired by
the reasoning depth of **NousHermes** and the tool-use power of **OpenClaw**.

| Feature | Detail |
|---------|--------|
| **Single binary** | Self-contained prebuilt binary, < 50 ms startup, measured macOS release artifact is about 49 MB |
| **Multi-provider LLM** | Copilot · OpenAI · Anthropic · Gemini · xAI · DeepSeek · Ollama |
| **ReAct tool loop** | File, terminal, web search, memory, process, skill tools |
| **ratatui TUI** | 60 fps capable terminal UI with streaming output |
| **ACP protocol** | Built-in JSON-RPC 2.0 stdio adapter for VS Code Copilot |
| **Built-in security** | Path safety, SSRF protection, command scanning, output redaction |

---

## Alternative Installation Methods

| Method | Command |
|--------|---------|
| **npm** | `npm install -g edgecrab-cli` |
| **PyPI** | `pip install edgecrab-cli` |
| **Cargo** | `cargo install edgecrab-cli` |
| **Docker** | `docker pull ghcr.io/raphaelmansuy/edgecrab:latest` |
| **Pre-built binary** | [GitHub Releases](https://github.com/raphaelmansuy/edgecrab/releases) |

---

## Supported Platforms

| Platform | Architecture |
|----------|-------------|
| macOS | Apple Silicon (arm64) · Intel (x64) |
| Linux | x86_64 · arm64 |
| Windows | x64 |

---

## Documentation

Full docs: **[edgecrab.com](https://www.edgecrab.com)**

- [Quick Start](https://www.edgecrab.com/getting-started/quick-start/)
- [Installation Guide](https://www.edgecrab.com/getting-started/installation/)
- [CLI Commands Reference](https://www.edgecrab.com/reference/cli-commands/)
- [Provider Setup](https://www.edgecrab.com/providers/)

---

## License

MIT © [Raphael Mansuy](https://github.com/raphaelmansuy)
