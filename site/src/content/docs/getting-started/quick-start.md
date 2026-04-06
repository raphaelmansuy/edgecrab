---
title: Quick Start
description: Get EdgeCrab running in under 90 seconds. Install via npm, pip, or cargo. Run the setup wizard and launch your first autonomous coding session.
sidebar:
  order: 1
---

EdgeCrab is a **Super Powerful Personal Assistant** inspired by NousHermes and OpenClaw —
a single static binary, no runtime dependencies, no daemon, no Python venv required.

```
Install --> edgecrab setup --> edgecrab doctor --> edgecrab
   |              |                  |                |
npm/pip/cargo  writes config     verifies keys    TUI starts
```

---

## Prerequisites

- At least one LLM access method: GitHub Copilot subscription, an API key, or a local Ollama instance
- **For npm install:** Node.js 18+
- **For pip install:** Python 3.10+
- **For cargo install:** Rust 1.85+ (optional — only needed for building from source)

---

## Installation

### Option A: npm (no Rust required)

```bash
npm install -g edgecrab-cli
```

Downloads a pre-built native binary for your platform automatically. This is the fastest path if you already have Node.js.

### Option B: pip (no Rust required)

```bash
pip install edgecrab-cli
```

Downloads a pre-built native binary on first run. Use `pipx install edgecrab-cli` for an isolated install.

### Option C: cargo (compile from source)

```bash
cargo install edgecrab-cli
```

### Option D: Build from source

```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --release
# Binary is at ./target/release/edgecrab
cp ./target/release/edgecrab ~/.local/bin/  # add to PATH
```

### Option E: Docker

```bash
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run -it --rm \
  -e GITHUB_TOKEN="$GITHUB_TOKEN" \
  -v "$HOME/.edgecrab:/root/.edgecrab" \
  ghcr.io/raphaelmansuy/edgecrab:latest
```

See [Docker guide](/user-guide/docker/) for full production setup.

---

## Step 1 — Guided Setup

```bash
edgecrab setup
```

The wizard detects your API keys from the environment, lets you choose a provider, and writes `~/.edgecrab/config.yaml`:

```
EdgeCrab Setup Wizard
──────────────────────────────────────────────────────────
✓ Detected GitHub Copilot (GITHUB_TOKEN)
✓ Detected OpenAI (OPENAI_API_KEY)

Choose LLM provider:
  [1] copilot      (GitHub Copilot — gpt-4.1-mini)  <- auto-detected
  [2] openai       (OpenAI — GPT-4.1, GPT-5, o3/o4)
  [3] anthropic    (Anthropic — Claude 4.5/4.6)
  [4] ollama       (local Ollama — llama3.3)
  ...

Provider [1]: 1

✓ Config written to ~/.edgecrab/config.yaml
✓ Created ~/.edgecrab/memories/
✓ Created ~/.edgecrab/skills/
```

Supported providers: `copilot` · `openai` · `anthropic` · `google` (Gemini) · `vertexai` · `xai` (Grok) · `deepseek` · `mistral` · `groq` · `huggingface` · `zai` · `openrouter` · `ollama` · `lmstudio`. See [Provider Overview](/providers/overview/).

:::note[Already have hermes-agent config?]
Run `edgecrab migrate` to import your existing config, memories, and skills in one step.
:::

---

## Step 2 — Verify Health

```bash
edgecrab doctor
```

```
EdgeCrab Doctor
──────────────────────────────────────────────────────────
✓  Config file         ~/.edgecrab/config.yaml
✓  State directory     ~/.edgecrab/
✓  Memories            ~/.edgecrab/memories/
✓  Skills              ~/.edgecrab/skills/
✓  GitHub Copilot      GITHUB_TOKEN set
✓  OpenAI              OPENAI_API_KEY set
✓  Provider ping       copilot/gpt-4.1-mini --> OK (312 ms)
──────────────────────────────────────────────────────────
All checks passed.
```

If a check fails, see [Configuration](/user-guide/configuration/) or [Provider Overview](/providers/overview/).

---

## Step 3 — Start Chatting

### Interactive TUI (default)

```bash
edgecrab
```

The TUI opens with a full-screen editor. Type your prompt and press **Enter**. Use **Shift+Enter** for multi-line input.

### One-shot prompt (headless)

```bash
edgecrab "summarise the git log for today"
edgecrab --quiet "count lines in src/**/*.rs"   # no banner, pipe-safe
```

### Specify model

```bash
edgecrab --model anthropic/claude-sonnet-4-20250514 "review this PR"
edgecrab --model ollama/llama3.3 "run completely offline"
```

### Continue the last session

```bash
edgecrab -C                          # resume last CLI session
edgecrab -C "refactor-api"           # resume named session
edgecrab --session <id>              # resume by exact session ID
```

### Preload skills

```bash
edgecrab -S git-workflow "review this branch"
edgecrab -S security,refactor        # comma-separated
```

See [Skills](/features/skills/) for creating your own.

### Parallel isolation with worktrees

```bash
edgecrab -w "explore performance improvements"
# Opens EdgeCrab in an isolated git worktree so changes don't pollute main
```

See [Worktrees](/user-guide/worktrees/) for the full workflow.

---

## TUI Quick Reference

Once inside the TUI, type `/help` to see all commands. Key shortcuts:

| Key | Action |
|-----|--------|
| `Enter` | Submit prompt |
| `Shift+Enter` | Insert newline |
| `Ctrl+C` | Interrupt running tool |
| `Ctrl+L` | Clear screen |
| `Ctrl+U` | Clear input line |
| `Alt+Up/Down` | Scroll output |
| `Ctrl+Home` / `Ctrl+End` | Jump to top / bottom |

| Slash command | Action |
|---------------|--------|
| `/model provider/model` | Hot-swap LLM mid-session |
| `/new` | Start a fresh session |
| `/help` | Full command reference |
| `/theme [name]` | List or switch skin preset |
| `/memory` | View loaded memories |
| `/tools` | List active tools |

Full slash command reference: [Slash Commands](/reference/slash-commands/).

---

## SDK Quick Start

### Python SDK

```bash
pip install edgecrab-sdk
```

```python
from edgecrab import Agent

agent = Agent(model="anthropic/claude-sonnet-4-20250514")
reply = agent.chat("Explain Rust ownership in 3 sentences")
print(reply)
```

Full docs: [Python SDK](/integrations/python-sdk/).

### Node.js / TypeScript SDK

```bash
npm install edgecrab-sdk
```

```typescript
import { Agent } from "edgecrab-sdk";

const agent = new Agent({ model: "openai/gpt-4o" });
const reply = await agent.chat("Write a TypeScript hello-world");
console.log(reply);
```

Full docs: [Node.js SDK](/integrations/node-sdk/).

---

## Next Steps

```
Quick Start --> Configuration --> Features --> Reference
                     |                |
               providers/          skills/
               overview.md        tools.md
```

| Goal | Where to go |
|------|-------------|
| Configure providers, toolsets, memory | [Configuration](/user-guide/configuration/) |
| Switch or add providers | [Provider Overview](/providers/overview/) |
| Understand the TUI in depth | [TUI Interface](/features/tui/) |
| Enable messaging platforms | [Messaging Gateways](/user-guide/messaging/) |
| Create custom skills | [Your First Skill](/guides/first-skill/) |
| Run on a server | [Self-Hosting](/guides/self-hosting/) |
| All CLI flags | [CLI Reference](/reference/cli-commands/) |

---

## Pro Tips

**Write better prompts.** Include context upfront: "In this Rust workspace using tokio 1.x, add a health-check endpoint to the axum server in crates/api/src/main.rs." The agent reads your AGENTS.md automatically, so put project conventions there once.

**Use one-shot mode for scripts.** `edgecrab --quiet "count total test files"` pipes perfectly into shell scripts — no banner, no TUI, just the answer.

**Alias common invocations.** Add to your shell config:
```bash
alias ec='edgecrab'
alias ecs='edgecrab --quiet'
alias ecc='edgecrab -C'   # continue last session
```

**Let the agent see your codebase.** Start `edgecrab` from your project root — it auto-loads `AGENTS.md` from the current directory and all parent directories. The more context it has, the fewer clarifying questions it asks.

**Control costs.** Add `display: { show_cost: true }` to `config.yaml` to see token usage after every response. Use `copilot` or `ollama` for exploration, `claude-opus` for final review.

---

## Frequently Asked Questions

**Q: I ran `edgecrab setup` but `edgecrab doctor` shows my API key is missing.**

Check that you exported the key in the same shell session (or have it in `~/.edgecrab/.env`). `edgecrab` reads `~/.edgecrab/.env` automatically at startup — add `OPENAI_API_KEY=sk-...` there for a persistent solution.

**Q: The agent ran a command that changed my files. How do I undo?**

All file writes use atomic operations (write temp → rename). Undo is via git: `git diff` to see changes, `git checkout -- .` to revert. Use `edgecrab -w` (worktree mode) for risky operations — changes are isolated to a separate branch.

**Q: How do I stop the agent mid-task?**

Press `Ctrl+C`. This cancels the current tool execution and LLM generation. The session history is preserved — you can continue from where you left off.

**Q: Can I use EdgeCrab without an internet connection?**

Yes. Point it at a local Ollama instance:
```bash
# Start Ollama with any model
ollama pull llama3.3
edgecrab --model ollama/llama3.3 "explain this code"
```
Set `EDGECRAB_MODEL=ollama/llama3.3` in `~/.edgecrab/.env` for a permanent default.

**Q: How do I run the same prompt on multiple files?**

Write a prompt that uses tool calls, or use the `--quiet` flag in a shell loop:
```bash
for f in src/**/*.rs; do
  edgecrab --quiet "summarise $f in one sentence" >> summaries.txt
done
```

**Q: The agent keeps exceeding the context window.**

Reduce `model.max_iterations` or break the prompt into smaller steps. For large codebases, use [Worktrees](/user-guide/worktrees/) to scope each session or preload only the relevant [Skills](/features/skills/).

**Q: Where are my conversation logs stored?**

In `~/.edgecrab/state.db` (SQLite). Browse them with `edgecrab sessions list` and search with `edgecrab sessions search "auth bug"`.

