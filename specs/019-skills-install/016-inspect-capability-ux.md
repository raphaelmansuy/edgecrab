# 016 — Inspect Capability UX (TUI)

**Cross-ref:** [008](./008-tui-expert-lens.md) · [009](./009-ux-ui-designer-lens.md) · [010](./010-solid-dry-ownership.md) · [proof/wi-inspect-capability.md](./proof/wi-inspect-capability.md)

## Thesis

Installing a skill is two decisions:

1. **Orientation** — What does it claim to do? (capability — SKILL.md first)
2. **Evidence** — What risks did the scanner find? (trust — Skill Guard)

W2 shipped marketplace chrome + Guard theatre. This wave makes **Inspect** a real dossier, not a mode flag.

## Product rule

No remote install starts until the user has seen (or explicitly waited on) an **Inspect** composition for that identifier — catalog + SKILL.md excerpt when `preview_install_scan` cache is ready, or a loading/failed state with retry. Safe skills must not silent-commit past orientation.

## Inspect dossier (one composition)

```text
Title · source · [trust]
provenance · N files · hash · verdict
WHAT IT CLAIMS — frontmatter + SKILL.md body preview
CAPABILITIES — headings / fences / notable paths (Claims/Contents, not Permissions)
FILES — SKILL.md first
TRUST TEASER — verdict + finding counts; e = full Guard
Footer: i install · e evidence · Esc back · ↑↓ scroll
```

## Keys

| Key | Inspect | SearchRemote |
|-----|---------|--------------|
| Enter | — | Inspect |
| `i` | Install (preview ready) | Enter Inspect first if needed |
| `e` | Full Guard evidence (review) | — |
| `s` | Retry preview scan | — |
| Esc | SearchRemote | BrowseInstalled |

## Ban list

- No new HTTP from TUI (reuse proactive `preview_install_scan`)
- No second allow/deny policy
- No emoji-only verdict as sole signal
- Never invent tool permissions from heuristics

## Acceptance

- [x] Enter on a result shows SKILL.md excerpt when scan cache ready
- [x] Verdict visible without installing (`e` or teaser)
- [x] `i` from SearchRemote enters Inspect before install when not yet inspected
- [x] Safe path uses Confirm strip before commit
- [x] Done banner includes remove aftercare
- [x] Pure unit tests for `SkillInspectModel` + Inspect keymap; no network
- [x] Proof [proof/wi-inspect-capability.md](./proof/wi-inspect-capability.md) filled
