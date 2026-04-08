# edgecrab-cli

> **Why this crate?** The most capable agent in the world is useless if interacting with it  
> feels like work. `edgecrab-cli` is the face of EdgeCrab: a full-screen ratatui TUI, guided  
> setup wizard, 42 slash commands, YAML theming, live streaming output, and instant startup  
> (< 50 ms). It packages the `Agent` from `edgecrab-core` into a single 15 MB static binary  
> with zero runtime dependencies.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## Install

```bash
# npm (no Rust required)
npm install -g edgecrab-cli

# pip (no Rust required)
pip install edgecrab-cli

# cargo
cargo install edgecrab-cli

# build from source
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build --release
./target/release/edgecrab --version
```

## First run

```bash
edgecrab                  # launches guided setup on first run
edgecrab --model anthropic/claude-opus-4.6
edgecrab --profile work   # isolated config + memory namespace
edgecrab migrate          # import hermes-agent config/sessions/skills
```

## Key slash commands

| Category | Commands |
|----------|----------|
| Session | `/new` `/retry` `/undo` `/history` `/save` `/export` `/resume` |
| Model | `/model [provider/model]` `/reasoning [effort]` |
| Config | `/config` `/prompt` `/personality` `/verbose` |
| Tools | `/tools` `/toolsets` `/reload-mcp` `/mcp-token` |
| Skills | `/skills list\|view\|install\|remove\|hub` |
| Memory | `/memory` |
| Analysis | `/cost` `/usage` `/compress` `/insights` |
| Gateway | `/platforms` `/approve` `/deny` |
| Scheduling | `/cron` |
| Media | `/voice on\|off\|status` |
| Misc | `/rollback [checkpoint]` `/background` `/queue` `/theme` `/paste` |

Full list: type `/help` inside the TUI.

## Theming

Create `~/.edgecrab/skin.yaml` to customise colors, spinner text, prompt symbol, and tool prefix:

```yaml
name: cyberpunk
colors:
  banner_border: "#FF00FF"
  banner_title:  "#00FFFF"
spinner:
  thinking_verbs: ["jacking in", "decrypting"]
branding:
  agent_name: "Cyber Agent"
  prompt_symbol: "⚡ "
```

Activate with `/theme cyberpunk`.

## Configuration

`~/.edgecrab/config.yaml` — key options:

```yaml
model: anthropic/claude-opus-4.6
max_iterations: 90
streaming: true
save_trajectories: false
skip_context_files: false   # set true to skip AGENTS.md / SOUL.md injection
skip_memory: false
```

Override any option via env var: `EDGECRAB_MODEL`, `EDGECRAB_SAVE_TRAJECTORIES`, etc.

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
