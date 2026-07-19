# 009 — UX / UI Designer Lens

**Cross-ref:** [001](./001-five-whys.md) · [006](./006-product-owner-lens.md) · [008](./008-tui-expert-lens.md)

## Design problem

Installing a skill is a **trust decision**, not a download. Users need:

1. Orientation — what is this skill?  
2. Evidence — what did the scanner find?  
3. Agency — clear, irreversible-feeling actions with reversible outcomes (uninstall).  
4. Continuity — same language in CLI, TUI, and gateway.

Hermes teaches well in **CLI Rich panels**. EdgeCrab must teach equally well in **TUI** (already strong) and **CLI stages** (weak today).

## Primary journey (happy path)

```text
Discover → Inspect → Evidence → Decide → Confirm → Use
```

| Step | User sees | Feels |
|------|-----------|-------|
| Discover | Name, source badge, trust hint | “I found something real” |
| Inspect | SKILL.md summary, file count, URL | “I know what it claims” |
| Evidence | Verdict + findings + file preview | “I am not guessing” |
| Decide | Install / Force / Trust / Cancel | “I chose with eyes open” |
| Confirm | Installed path + how to invoke | “I can use it now” |
| Use | `/skill-name` or agent picks it up | “It worked” |

## Visual hierarchy (TUI)

One composition per overlay — not a dashboard of competing panels.

```text
  Title (skill name) + verdict accent
  ────────────────────────────────────
  One-line provenance (source · trust · hash short)
  ────────────────────────────────────
  Primary body (findings OR files — Tab switches)
  ────────────────────────────────────
  Action row (Cancel · Force · Trust · Install)
  Footer keys
```

Rules:

- **Verdict owns color** — Safe / Caution / Dangerous palette from existing `skill_trust_verdict_palette` (do not invent purple glow themes).  
- **No badge spam** — at most: source, trust level, cached/fresh scan.  
- **SKILL.md first** in file list.  
- **Findings before poetry** — pattern id + severity beat long prose.

## Copy system (voice)

| Situation | Do | Don't |
|-----------|-----|-------|
| Caution | “Scanner found caution-level patterns. Review findings, then Force only if you accept the risk.” | “Probably fine with --force” |
| Dangerous | “Dangerous patterns blocked. Trust binds to this content hash and will re-prompt if the skill changes.” | “Override security” |
| Rate limit | “GitHub rate limit. Set GITHUB_TOKEN or GH_TOKEN, then retry.” | “Request failed” |
| Success | “Installed `foo`. Run `/foo` or ask the agent to use it.” | “OK” |
| Ambiguous | “Multiple matches — pick a full identifier.” | Dump raw JSON |

Tone: direct, calm, specific. No emoji in policy copy (skin may add tool prefixes elsewhere).

## CLI storytelling (parity with Hermes Rich)

Stage lines (human):

```text
Fetching owner/repo/path …
Quarantined → ~/.edgecrab/skills/.hub/quarantine/…
Scanning (skills-guard) …
Verdict: CAUTION (3 findings)
  HIGH  exfil.pattern  SKILL.md:42
Install blocked. Re-run with --force after review, or:
  edgecrab skills inspect owner/repo/path --scan
```

`--json` emits the same stages as structured events for CI.

## Trust theatre principles

1. **Slow the dangerous path** — Dangerous requires explicit Trust action, not a buried flag in muscle memory alone (CLI `--trust` still allowed; TUI should not auto-select Trust).  
2. **Make Force feel costly** — Caution list visible; Force is never the default focused button.  
3. **Hash transparency** — show short content hash when trusting.  
4. **Aftercare** — success screen always includes uninstall hint.

## Gateway / constrained UI

Text-only platforms:

- Same copy system, compressed.  
- Link to `inspect --scan` output truncated to top 5 findings.  
- Never claim “installed securely” without verdict line.

## Accessibility / terminal reality

| Constraint | Design response |
|------------|-----------------|
| 80×24 | Overlay max width ~100 but shrinks; findings scroll |
| No mouse | Full keyboard (008) |
| Color-blind | Verdict labels in text, not color-only |
| Termux / narrow | Collapse file preview; keep actions |

## Skin / brand

- Use semantic skin colors (status, warning, error) — not hardcoded neon.  
- Overlay background may keep current near-black panel for contrast with transcript.  
- Avoid: purple-on-white AI cliché, sticker badges on hero regions, emoji verdict icons as sole signal.

## Success metrics (UX)

| Metric | Measure |
|--------|---------|
| Comprehension | User can state verdict + one finding after one Guard view |
| Mis-click Force | Default focus ≠ Force |
| Drop-off | Esc from Guard returns to search with query preserved |
| Cross-surface | Same verbs: install, force, trust, inspect |

## Deliverables for W2 (design acceptance)

- [x] Marketplace footer key help matches 008 table *(chrome; Inspect dossier in [016](./016-inspect-capability-ux.md))*  
- [ ] Guard action order: Cancel · Inspect files · Force(if caution) · Trust(if dangerous) · Install(if safe) *(partial: f/t/Cancel; Files via Tab)*  
- [x] CLI stage copy documented (`install_stages` + `--json`)  
- [x] Empty states: no results, no token, offline — one next step each *(016 W-I3)*  

**Note:** W2 proof covers marketplace FSM/theatre. Capability Inspect (SKILL.md-first before install) is [016](./016-inspect-capability-ux.md) + [proof/wi-inspect-capability.md](./proof/wi-inspect-capability.md) (closed).
