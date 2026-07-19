# 003 — Hermes Reference (Code Is Law)

**Cross-ref:** [000](./000-overview.md) · [004](./004-edgecrab-current-state.md) · [005](./005-gap-matrix.md)

Surveyed: `/Users/raphaelmansuy/Github/03-working/hermes-agent` (July 2026).

## Core modules

| Path | Role |
|------|------|
| `tools/skills_hub.py` | Sources, quarantine, lock, taps, search/install (~4.2k LOC) |
| `tools/skills_guard.py` | Scanner + install policy (~121 regex patterns) |
| `tools/skills_sync.py` | Bundled seed via `.bundled_manifest` |
| `tools/skills_tool.py` | Agent: `skills_list` / `skill_view` |
| `tools/skill_manager_tool.py` | Agent writes: `skill_manage` + optional guard |
| `tools/skills_ast_audit.py` | Deep Python AST audit |
| `tools/threat_patterns.py` | Shared threat patterns |
| `tools/blueprints.py` | Post-install automation suggestions |
| `hermes_cli/skills_hub.py` | Shared `do_*` + `/skills` slash (~2k LOC) |
| `hermes_cli/subcommands/skills.py` | Argparse for `hermes skills …` |
| `hermes_cli/skills_config.py` | Interactive enable/disable |
| `ui-tui/src/components/skillsHub.tsx` | Ink TUI overlay (list/inspect/install) |
| `tui_gateway/server.py` | RPC `skills.manage` / `skills.reload` |
| `.github/workflows/skills-index.yml` | Twice-daily centralized index rebuild |
| `scripts/build_skills_index.py` | Index builder |
| `website/docs/user-guide/features/skills.md` | Primary user docs |

## Install pipeline

```text
do_install(identifier)
  → create_source_router()  # Optional, HermesIndex, SkillsSh, WellKnown,
                            # Url, GitHub(+taps), ClawHub, ClaudeMarketplace,
                            # LobeHub, BrowseSh
  → resolve short name (unified_search / ambiguity table)
  → src.fetch → SkillBundle
  → quarantine_bundle → ~/.hermes/skills/.hub/quarantine/<skill>/
  → scan_skill_cached → ScanResult + provenance
  → should_allow_install(result, force=…)
  → TTY confirm (or --yes / slash skip)
  → install_from_quarantine
       → move to ~/.hermes/skills/[category/]<name>/
       → HubLockFile.record_install()
       → append_audit_log("INSTALL")
       → optional blueprint → /suggestions
       → clear_skills_system_prompt_cache()
```

## Storage layout

```text
~/.hermes/skills/
├── <category>/<skill>/SKILL.md
├── .hub/
│   ├── lock.json
│   ├── quarantine/
│   ├── audit.log
│   ├── taps.json
│   ├── index-cache/
│   └── scan-cache/
├── .bundled_manifest
└── .no-bundled-skills
```

## CLI surface (`hermes skills`)

| Action | Notes |
|--------|-------|
| `browse` | `--page`, `--size`, `--source` |
| `search` | query, `--source`, `--limit`, `--json` |
| `install` | identifier\|URL, `--category`, `--name`, `--force`, `-y` |
| `inspect` | identifier |
| `list` | `--source all\|hub\|builtin\|local` |
| `check` / `update` | optional skill name |
| `audit` | optional name, `--deep` |
| `uninstall` | name |
| `reset` / `list-modified` / `diff` | bundled edit workflows |
| `opt-out` / `opt-in` | bundled seed control |
| `repair-official` | official skill repair |
| `publish` | `--to github\|clawhub` |
| `snapshot export\|import` | hub skill set |
| `tap list\|add\|remove` | GitHub repo taps |
| `config` | interactive enable/disable |

Slash `/skills` mirrors hub actions; install uses `skip_confirm=True` (no TTY `input()`).

## Trust model

| Level | Sources | Safe | Caution | Dangerous |
|-------|---------|------|---------|-----------|
| builtin | official optional | allow | allow | allow |
| trusted | openai/anthropics/huggingface/NVIDIA skills | allow | allow | **block** |
| community | else | allow | **block** | **block** |
| agent-created | `skill_manage` (if guarded) | allow | allow | ask |

`--force` overrides Caution only; **not** Dangerous for community/trusted.

**Important:** Hermes taps are GitHub repo lists + content scan. There is **no** Ed25519/GPG signed tap manifest verification in current `skills_hub.py` / `skills_guard.py`. Gap analysis 028 overstated this.

## TUI (Ink)

`skillsHub.tsx`:

- Stages: category → skill → actions
- RPC: `skills.manage` `{ action: list|inspect|install }`
- Install swallows Rich console progress (`_Q().print` no-op in gateway path)
- No scan findings pane comparable to EdgeCrab Skill Guard overlay

Desktop/web hubs exist (`apps/desktop`, `web/…/SkillsPage.tsx`) — out of EdgeCrab CLI scope.

## Index

- CI builds `website/static/api/skills-index.json`
- Runtime: `https://hermes-agent.nousresearch.com/docs/api/skills-index.json` via `HermesIndexSource`
- When available, parallel search skips live GitHub/skills.sh/ClawHub crawls (rate-limit relief)

## Strengths to respect

1. Broad multi-registry router + short-name resolution  
2. Rich CLI storytelling (panels, scan report, provenance line)  
3. Own index CI (ecosystem control)  
4. Bundled sync hygiene (opt-out, reset, diff, curator suppression)  
5. Publish + snapshot + blueprints  
6. Mature docs (`skills.md`, catalogs, creating-skills)

## Gaps / rough edges

1. No first-class local-path `install ./dir` (copy/sync/agent-create instead)  
2. TUI install UX thin vs CLI  
3. Regex false positives → force culture  
4. Tap docs path drift (`~/.hermes/.hub` vs `~/.hermes/skills/.hub`)  
5. No cryptographic publisher identity despite aspirational gap docs elsewhere  

## Notable APIs

```text
SkillBundle, SkillSource (ABC), GitHubSource, UrlSource, SkillsShSource,
WellKnownSkillSource, ClawHubSource, HermesIndexSource, OptionalSkillSource
quarantine_bundle(), install_from_quarantine(), create_source_router()
unified_search(), HubLockFile, TapsManager
scan_skill_cached(), should_allow_install(), format_scan_report()
do_install / do_search / do_browse / do_inspect / handle_skills_slash()
```
