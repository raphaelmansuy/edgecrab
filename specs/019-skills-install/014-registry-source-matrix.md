# 014 — Registry Source Matrix (Hermes ∪ Peers)

**Cross-ref:** [000](./000-overview.md) · [015](./015-registry-implementation.md) · [005](./005-gap-matrix.md)

## Target: broadest SKILL.md client

EdgeCrab must support every Hermes `source_id` plus peer-agent identifier and filesystem bridges (Pi, OpenClaw, Claude Code, Codex, generic agentskills).

## Hermes sources (required)

| `source_id` | Identifier forms | Trust default |
|-------------|------------------|---------------|
| `official` | `official/<category>/<skill>` | builtin |
| `hermes-index` / unified-index | index entry ids | from entry |
| `skills-sh` | `skills.sh:owner/repo/skill`, `skills-sh:…` | community (GitHub trust if trusted repo); empty browse = sitemap catalog (`skills_sh_sitemap_v1`) + seed fallback |
| `well-known` | `well-known:https://host/.../name`, hub URL search | community |
| `url` | `https://…/SKILL.md` | community |
| `github` | `owner/repo[/path]`, curated aliases, taps | trusted / community |
| `clawhub` | `clawhub:slug`, `@owner/slug` (OpenClaw) | community |
| `claude-marketplace` | `claude-marketplace:…` | via marketplace repo trust |
| `lobehub` | `lobehub:<agent_id>` | community |
| `browse-sh` | `browse-sh:<slug>` | community |

## EdgeCrab extras

| `source_id` | Notes |
|-------------|-------|
| `edgecrab` curated | `edgecrab:<rel>` → `raphaelmansuy/edgecrab` |
| `hermes-agent` curated | `hermes-agent:<rel>` |
| `openai` / `anthropics` curated | Codex / Claude Code catalogs |
| `agentskills.io` | Federation via `/.well-known/skills/index.json` |
| `npm` | Pi-style `npm:pkg` → pack + find `SKILL.md` dirs |
| local | path / `./dir` → always quarantine |

## Default taps (Hermes DEFAULT_TAPS parity)

| Repo | Root | Trust |
|------|------|-------|
| `openai/skills` | `skills/.curated/` | trusted |
| `openai/skills` | `skills/.system/` | trusted |
| `anthropics/skills` | `skills/` | trusted |
| `huggingface/skills` | `skills/` | trusted |
| `NVIDIA/skills` | `skills/` | trusted |
| `garrytan/gstack` | `""` (repo root) | community |

Plus EdgeCrab curated trees (not taps): `raphaelmansuy/edgecrab`, `NousResearch/hermes-agent`.

## Provider filters (`--source` / search filter)

| Filter | Maps to |
|--------|---------|
| `openai` | openai/skills taps + curated |
| `anthropic` | anthropics/skills |
| `huggingface` | huggingface/skills |
| `nvidia` | NVIDIA/skills |
| `voltagent` | `voltagent/awesome-agent-skills` (if tapped) |
| `gstack` | garrytan/gstack |
| `minimax` | `minimax-ai/cli` (if tapped) |

## Peer agent bridges

| Peer | Registry | EdgeCrab |
|------|----------|----------|
| OpenClaw | ClawHub + `git:` + local | `@owner/slug` → clawhub; `git:` normalize |
| Pi | `git:` / `npm:` packages | same + npm pack extract |
| Claude Code | marketplace + `~/.claude/skills` | marketplace + `import-from claude` |
| Codex | openai/skills + `~/.codex/skills` | curated + `import-from codex` |
| agentskills | `~/.agents/skills` + federation | `import-from agents` + `federation_hubs` |
| OpenClaw home | `~/.openclaw/skills` | `import-from openclaw` |

## Identifier grammar (`normalize_identifier`)

```text
git:owner/repo[@ref][/path]
git:https://github.com/owner/repo...
npm:<package>[@version]
@owner/slug                    → clawhub:slug (OpenClaw)
skills.sh:|skills-sh: owner/repo/skill
well-known:<base>/<name>
claude-marketplace:|clawhub:|browse-sh:|lobehub:|agentskills.io:
edgecrab:|hermes-agent:|openai:|anthropics: <rel>
official/<category>/<skill>
owner/repo[/path]
https://…/SKILL.md
./local | /abs | ~/path
```

## Import-from homes

| Alias | Default path |
|-------|--------------|
| `claude` | `~/.claude/skills` |
| `codex` | `~/.codex/skills` |
| `pi` | `~/.pi/agent/skills` |
| `agents` | `~/.agents/skills` |
| `openclaw` | `~/.openclaw/skills` |
| path | arbitrary directory of skill folders |
