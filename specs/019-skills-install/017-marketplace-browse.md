# 017 — Marketplace Browse Without Typing + By Source

**Cross-ref:** [008](./008-tui-expert-lens.md) · [016](./016-inspect-capability-ux.md) · [proof/w2-tui-marketplace.md](./proof/w2-tui-marketplace.md)

## Product rule

```text
Open marketplace → Browse catalog for current source (no typing required)
Type → filter / search within source
[ ] or S → change source · list refreshes
Enter → Inspect (016)
```

## Behavior

| Action | Result |
|--------|--------|
| `/skills hub` / `R` / empty SearchRemote | Schedules `search_hub("")` browse |
| Empty query + index | Lists unified-index skills when seeded |
| Empty query + thin index | Live fan-out via `SkillSourceRouter` |
| Source chips (wide) / `[` `]` / `p` | Cycle `source_filter` + rebrowse |
| `S` | Source picker overlay |
| Digits `1`–`9` (query empty) | Jump to Nth source |
| Type query | Normal search; digits type into query |

## Owners

| Layer | Module |
|-------|--------|
| Hub browse | `skills_hub::search_hub` empty-query path |
| Schedule | `App::schedule_remote_skill_search` / `poll_remote_skill_search` |
| Chrome | `browser_selectors::render_remote_skill_selector` + `skills_marketplace` |

## Ban list

- No second allow/deny policy
- Inspect / Guard / ConfirmSafe unchanged
- CLI empty `/skills search` still shows sources catalog text (hub_slash); TUI uses browse

## Acceptance

- [x] Empty marketplace open schedules browse (not blank tip-only forever)
- [x] Source picker + chip/bracket cycle update filter and rebrowse
- [x] Footer mentions source pick / inspect
- [x] Offline tests: keymap + hub empty index browse
