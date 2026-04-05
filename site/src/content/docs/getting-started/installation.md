---
title: Installation
description: Install EdgeCrab on macOS, Linux, or Windows. Choose from npm, pip, crates.io, pre-built binaries, Docker, or build from source. Full prerequisites and verification guide.
sidebar:
  order: 2
---

Get EdgeCrab up and running in under two minutes. Choose the method that fits your environment —
**no Rust toolchain required** when installing via npm or pip.

---

## Option A — npm (recommended, no Rust required)

```bash
npm install -g edgecrab-cli
```

The postinstall script automatically downloads the correct pre-built native binary for your platform.
Requires **Node.js 18+**. No Rust, GCC, or build tools needed.

Verify:

```bash
edgecrab version
# EdgeCrab 0.1.0  (rustc 1.85.0, 2025-02-20)
#   providers: copilot openai anthropic gemini xai deepseek huggingface zai openrouter ollama lmstudio
```

You can also run without a global install:

```bash
npx edgecrab-cli setup
npx edgecrab-cli "summarise the git log for today"
```

---

## Option B — pip (recommended, no Rust required)

```bash
pip install edgecrab-cli
```

On first run the package downloads the correct pre-built binary for your platform.
Requires **Python 3.10+**. No Rust or build tools needed.

```bash
edgecrab version
edgecrab setup
edgecrab "explain this codebase"
```

> **Tip:** Use a virtual environment or `pipx` to keep the install isolated:
> ```bash
> pipx install edgecrab-cli
> ```

---

## Option C — cargo install (from source)

```bash
cargo install edgecrab-cli
```

Pulls and compiles the latest stable release from [crates.io](https://crates.io/crates/edgecrab-cli).
Requires **Rust 1.85+**. The binary is placed in `~/.cargo/bin/edgecrab`.

> **No Rust?** Install it in one command:
> ```bash
> curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
> ```

Verify:

```bash
edgecrab version
# EdgeCrab 0.1.0  (rustc 1.85.0, 2025-02-20)
#   providers: copilot openai anthropic gemini xai deepseek huggingface zai openrouter ollama lmstudio
```

---

## Option D — Pre-built Binary

Download the archive for your platform from [GitHub Releases](https://github.com/raphaelmansuy/edgecrab/releases):

| Platform | Archive |
|----------|---------|
| macOS arm64 (Apple Silicon) | `edgecrab-aarch64-apple-darwin.tar.gz` |
| macOS x86_64 (Intel) | `edgecrab-x86_64-apple-darwin.tar.gz` |
| Linux x86_64 | `edgecrab-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `edgecrab-aarch64-unknown-linux-gnu.tar.gz` |
| Windows x86_64 | `edgecrab-x86_64-pc-windows-msvc.zip` |

```bash
# macOS example
curl -L https://github.com/raphaelmansuy/edgecrab/releases/latest/download/edgecrab-aarch64-apple-darwin.tar.gz \
  | tar -xz -C /usr/local/bin
chmod +x /usr/local/bin/edgecrab
```

---

## Option E — Docker

```bash
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run --rm -it \
  -e OPENAI_API_KEY="$OPENAI_API_KEY" \
  -v ~/.edgecrab:/root/.edgecrab \
  ghcr.io/raphaelmansuy/edgecrab:latest
```

See [Docker Deployment](/user-guide/docker/) for full configuration, docker-compose, and gateway deployment.

---

## Option F — Build from Source

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --release       # ~30 s on modern hardware
./target/release/edgecrab version
```

For development (incremental, unoptimized):

```bash
cargo build
./target/debug/edgecrab version
```

---

## Installation Methods Summary

| Method | Command | Requires |
|--------|---------|---------|
| **npm** | `npm install -g edgecrab-cli` | Node.js 18+ |
| **pip** | `pip install edgecrab-cli` | Python 3.10+ |
| **cargo** | `cargo install edgecrab-cli` | Rust 1.85+ |
| **Docker** | `docker pull ghcr.io/raphaelmansuy/edgecrab:latest` | Docker |
| **Pre-built binary** | Download from GitHub Releases | Nothing |

---

## After Installation

### 1. Run the Setup Wizard

```bash
edgecrab setup
```

The wizard:
- Scans your environment for API keys
- Prompts you to choose an LLM provider
- Writes `~/.edgecrab/config.yaml`
- Creates the memories and skills directories

```
EdgeCrab Setup Wizard
────────────────────────────────────────────────────────────────
✓ Detected GitHub Copilot (GITHUB_TOKEN)
✓ Detected OpenAI     (OPENAI_API_KEY)

Choose LLM provider:
  [1] copilot     (GitHub Copilot — gpt-4.1-mini)  ← auto-detected
  [2] openai      (OpenAI — gpt-4o)
  [3] anthropic   (Anthropic — claude-opus-4-5)
  [4] ollama      (local Ollama — llama3.3)
  ...
Provider [1]: 1

✓ Config written to /Users/you/.edgecrab/config.yaml
✓ Created /Users/you/.edgecrab/memories/
✓ Created /Users/you/.edgecrab/skills/

Run `edgecrab` to start chatting!
```

### 2. Verify Your Installation

```bash
edgecrab doctor
```

```
EdgeCrab Doctor
────────────────────────────────────────────────────────────────
✓  Config file          /Users/you/.edgecrab/config.yaml
✓  State directory      /Users/you/.edgecrab/
✓  Memories directory   /Users/you/.edgecrab/memories/
✓  Skills directory     /Users/you/.edgecrab/skills/
✓  GitHub Copilot       GITHUB_TOKEN set
✓  OpenAI               OPENAI_API_KEY set
✓  Provider ping        copilot/gpt-4.1-mini → OK (312 ms)
────────────────────────────────────────────────────────────────
All checks passed.
```

If any check fails, see the [Configuration guide](/user-guide/configuration/) for troubleshooting.

---

## Shell Completion (optional)

EdgeCrab can generate tab-completion scripts for bash, zsh, fish, and PowerShell:

```bash
# zsh
edgecrab completion zsh >> ~/.zshrc
# source ~/.zshrc

# bash
edgecrab completion bash >> ~/.bashrc
# source ~/.bashrc

# fish
edgecrab completion fish > ~/.config/fish/completions/edgecrab.fish
```

---

## What's Next?

- **[Quick Start](/getting-started/quick-start/)** — Your first conversation in 90 seconds
- **[Configuration](/user-guide/configuration/)** — Customize models, tools, memory
- **[CLI Reference](/reference/cli-commands/)** — Every flag and subcommand explained

---

## Troubleshooting

### `edgecrab: command not found` after `cargo install`

`~/.cargo/bin` is not in your PATH. Fix it:
```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc   # zsh
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc  # bash
source ~/.zshrc   # reload immediately
```
Or run `source $HOME/.cargo/env` which Rustup creates.

### Cargo build fails with `error[E0554]: #![feature]` or edition error

You need Rust ≥ 1.85. Upgrade:
```bash
rustup update stable
rustup default stable
rustc --version   # confirm 1.85+
```

### `edgecrab doctor` shows provider ping failure

This usually means the API key is set in a different shell than the one running `edgecrab`. Persistent fix: add the key to `~/.edgecrab/.env`:
```bash
echo 'OPENAI_API_KEY=sk-...' >> ~/.edgecrab/.env
```
EdgeCrab reads this file automatically at every startup.

### Docker: permission denied on `~/.edgecrab`

The container user (root by default) and your host user have different UIDs. Fix with explicit UID mapping:
```bash
docker run --rm -it \
  -u "$(id -u):$(id -g)" \
  -e OPENAI_API_KEY="$OPENAI_API_KEY" \
  -v ~/.edgecrab:/root/.edgecrab \
  ghcr.io/raphaelmansuy/edgecrab:latest
```

### Build from source is slow

Use `cargo build --release` only for production. For development, `cargo build` (debug) is 5-10× faster. The first build downloads and compiles all deps (~30 s on fast hardware). Subsequent builds are incremental (seconds).

### Pre-built binary: `Illegal instruction` on macOS Intel

You downloaded the Apple Silicon binary by mistake. Use `edgecrab-x86_64-apple-darwin.tar.gz` for Intel Macs. Verify your arch: `uname -m` (returns `x86_64` for Intel, `arm64` for Apple Silicon).

---

## Frequently Asked Questions

**Q: Do I need to keep Rust installed after `cargo install`?**

No. The binary is fully self-contained. The Rust toolchain is only needed to compile. After `cargo install edgecrab-cli`, you can remove Rust if you want (though you'll need it for updates via `cargo`).

**Q: How do I install a specific version?**

```bash
cargo install edgecrab-cli --version 0.1.0
```
Or download a tagged release from GitHub Releases.

**Q: Can I install EdgeCrab system-wide (for all users)?**

Yes. Build from source and copy the binary to `/usr/local/bin`:
```bash
cargo build --release
sudo cp target/release/edgecrab /usr/local/bin/
```

**Q: How much disk space does EdgeCrab use?**

- Binary: ~15 MB
- State database: grows with session history, typically < 100 MB per year of heavy use
- Skills directory: ~1 KB per skill (just Markdown files)
- Total: `~/.edgecrab/` is typically < 50 MB
