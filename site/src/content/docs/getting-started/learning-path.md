---
title: Learning Path
description: Find the right EdgeCrab documentation for your experience level — from zero to autonomous coding agent in the fastest possible route.
sidebar:
  order: 4
---

Use this page to find the fastest path through the documentation based on your background.

---

## Path 1 — Complete Beginner

You've never used an AI coding agent or Rust tools before.

**Time: ~15 minutes**

1. **[Installation](/getting-started/installation/)** — Install the binary and API key
2. **[Quick Start](/getting-started/quick-start/)** — Your first TUI session
3. **[CLI Interface](/user-guide/cli/)** — Learn all commands and slash commands  
4. **[Configuration](/user-guide/configuration/)** — Set your preferred model, memory, and tools

**When you're ready to explore further:**
- [TUI Interface](/features/tui/) — Master keyboard navigation, themes, session management
- [Security Model](/user-guide/security/) — Understand what EdgeCrab can and cannot do without your approval

---

## Path 2 — Python / Node.js Developer

You want to use EdgeCrab as a library in your own code.

**Time: ~10 minutes**

1. **[Quick Start](/getting-started/quick-start/)** → read the SDK tabs
2. **[Python SDK](/integrations/python-sdk/)** — Async Agent, streaming, tool customization
3. **[Node.js SDK](/integrations/node-sdk/)** — TypeScript Agent, streaming, CLI

**Key things to know:**
- Both SDKs are thin wrappers that talk to the EdgeCrab binary — install the binary first
- The Python SDK supports async/await and streaming out of the box
- The Node.js SDK ships with full TypeScript types

---

## Path 3 — AI Agent Power User

You're already using Hermes Agent or another AI agent and want to switch.

**Time: ~5 minutes**

1. **[Migrating from Hermes](/user-guide/migration/)** — One-command import of config, memories, skills
2. **[CLI Interface](/user-guide/cli/)** — How EdgeCrab's commands map to Hermes commands
3. **[Skills System](/features/skills/)** — How EdgeCrab extends and improves on skills

**The migration preserves:**
- All your `~/.hermes/memories/` files  
- All your `~/.hermes/skills/` directories  
- Your `config.yaml` and `.env`  
- Session history is not migrated (SQLite schema differs)

---

## Path 4 — Security / DevOps Engineer

You're deploying EdgeCrab in a team or production environment.

**Time: ~20 minutes**

1. **[Security Model](/user-guide/security/)** — Path safety, SSRF, command scanning, redaction
2. **[Docker Deployment](/user-guide/docker/)** — Container setup, volumes, environment variables
3. **[Self-Hosting with Docker](/guides/self-hosting/)** — docker-compose, reverse proxy, monitoring
4. **[Configuration Reference](/reference/configuration/)** — Every config option and its security implications

**Team deployment checklist:**
- [ ] Set `tools.file.allowed_roots` to project directories only
- [ ] Enable `security.approval_required` for destructive operations
- [ ] Use `allowed_users` in each messaging gateway config
- [ ] Mount `~/.edgecrab` as a persistent volume in Docker
- [ ] Set API keys via environment variables, not `config.yaml`

---

## Path 5 — Open-Source Contributor

You want to extend or contribute to EdgeCrab.

**Time: ~30 minutes**

1. **[Architecture](/developer/architecture/)** — Crate graph, module boundaries, data flow
2. **[ReAct Tool Loop](/features/react-loop/)** — How the agent loop works internally
3. **[Building Your First Skill](/guides/first-skill/)** — End-to-end skill authoring
4. **[Contributing](/contributing/)** — Code style, PR workflow, testing requirements

**Development setup:**
```bash
git clone https://github.com/raphaelmansuy/edgecrab
cd edgecrab
cargo build                    # debug build (fast)
cargo test --workspace         # all tests
cargo test -p edgecrab-core    # specific crate
```

---

## Full Documentation Map

```
EdgeCrab Docs
+-- Getting Started
|   +-- Quick Start            <- start here
|   +-- Installation           <- all install methods
|   +-- Updating & Uninstalling
|   \-- Learning Path          <- you are here
|
+-- Using EdgeCrab
|   +-- CLI Interface          <- commands, flags, slash commands
|   +-- Configuration          <- config.yaml reference
|   +-- Sessions & Memory      <- history, memory, compression
|   +-- Docker Deployment      <- container operation
|   +-- Security Model         <- defense-in-depth layers
|   \-- Migrating from Hermes  <- import your existing data
|
+-- Features
|   +-- Overview               <- feature comparison
|   +-- ReAct Tool Loop        <- autonomous reasoning engine
|   +-- TUI Interface          <- ratatui keyboard guide
|   +-- Skills System          <- reusable learned skills
|   \-- SQLite State & Search  <- session persistence & FTS5
|
+-- LLM Providers
|   +-- Provider Overview      <- all 14 providers
|   \-- Local Models           <- Ollama & LM Studio
|
+-- Integrations
|   +-- ACP / VS Code Copilot  <- agent protocol & IDE
|   +-- Python SDK             <- pip install edgecrab-sdk
|   \-- Node.js SDK            <- npm install edgecrab-sdk
|
+-- Guides & Tutorials
|   +-- Building Your First Skill
|   +-- Autonomous Coding Workflows
|   \-- Self-Hosting with Docker
|
+-- Developer Guide
|   +-- Architecture
|   \-- Contributing
|
\-- Reference
    +-- CLI Reference
    +-- Configuration Reference
    \-- Changelog
```

---

## "I'm Stuck" Quick Fixes

| Symptom | Fix |
|---------|-----|
| `edgecrab: command not found` | Add `~/.cargo/bin` to PATH; re-run `source ~/.zshrc` |
| Doctor shows provider error | Add key to `~/.edgecrab/.env`, not just the shell |
| Agent loop exceeds budget | Increase `model.max_iterations` in config, or break task into steps |
| Model says context is too long | Enable `compression.enabled: true` or reduce `session.max_context_tokens` |
| Messaging gateway not receiving | Check `edgecrab gateway status`; verify env vars set correctly |
| Skills not loading | Ensure skill is a *directory* with `SKILL.md` inside, not a bare `.md` file |

Still stuck? Post in [GitHub Discussions](https://github.com/raphaelmansuy/edgecrab/discussions).
