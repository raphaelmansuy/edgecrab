# 019 — Skills Install: Exceed Hermes

**Date:** 2026-07-18  
**Thesis:** One install law, one trust model, every surface equally capable — with a polished ratatui marketplace that makes trust decisions legible. EdgeCrab already leads on typed `--trust` + TUI guard theatre; Hermes leads on CLI breadth and install storytelling. This pack defines how EdgeCrab exceeds both.

## First principles (summary)

```text
Skills = progressive-disclosure expertise injected into agent context
Install = untrusted text → host-effecting agent behavior
Harness = controller: fetch → quarantine → scan → gate → commit → invalidate
```

| Law | Meaning |
|-----|---------|
| **L1 One Pipeline** | fetch → quarantine → scan → gate → commit → invalidate |
| **L2 One Trust Model** | `trust_level × verdict × {force, trusted_dangerous, signatures}` |
| **L3 Surface Parity** | CLI ≡ slash ≡ TUI ≡ gateway ≡ agent tool |
| **L4 Evidence Before Trust** | findings + file preview before install/trust |
| **L5 Cache Law** | hub mutate → discovery + prompt skills cache only; never touch `cached_system_prompt` mid-turn |
| **L6 Publisher > Pattern** | signatures/TOFU beat regex-only for typosquats |
| **L7 DRY / SOLID** | one façade; UI is renderer; no parallel scanners |

Full laws: [002-first-principles](./002-first-principles.md). Five WHYs: [001-five-whys](./001-five-whys.md).

## Honest baseline (code is law)

| Area                               | Hermes                                    | EdgeCrab                                      | Leader                        |
| ------------------------------------| -------------------------------------------| -----------------------------------------------| -------------------------------|
| Quarantine → scan → install        | Mature                                    | Mature (+ local path)                         | Tie                           |
| Trust gate (`--force` / dangerous) | force; dangerous hard-block               | `--force` + hash-bound `--trust`              | **EdgeCrab**                  |
| TUI scan theatre                   | Thin Ink list/install                     | Skill Guard overlay + file inspector          | **EdgeCrab**                  |
| CLI breadth                        | Full `hermes skills …`                    | Clap ≡ slash (tap/trust/audit/…); `-y`/`--json` close-out | Tie → EdgeCrab closing residual |
| Central skills index               | Own CI + hosted JSON                      | Consumes Hermes index URL                     | **Hermes**                    |
| Signed publisher taps              | **Not in code** (docs/gap-028 overstated) | Ed25519 + TOFU (`signed_taps.rs`)             | **EdgeCrab**                  |
| Doctor hub health                  | Indirect via workflows                    | Lock/orphans/index age/token (`hub_health`)   | **EdgeCrab**                  |
| TUI marketplace FSM                | Thin Ink list/install                     | BrowseInstalled + theatre + Guard + polish    | **EdgeCrab**                  |

## Document map

| Doc | Lens | Purpose |
|-----|------|---------|
| [001-five-whys](./001-five-whys.md) | PO + Eng | Root cause → success metrics |
| [002-first-principles](./002-first-principles.md) | All | Operator questions + install invariants |
| [003-hermes-reference](./003-hermes-reference.md) | AI Eng | Hermes code map |
| [004-edgecrab-current-state](./004-edgecrab-current-state.md) | AI Eng | EdgeCrab code map |
| [005-gap-matrix](./005-gap-matrix.md) | PO + Eng | Hermes / EdgeCrab / Target |
| [006-product-owner-lens](./006-product-owner-lens.md) | **Product Owner** | JTBD, value, kill criteria |
| [007-ai-engineer-lens](./007-ai-engineer-lens.md) | **AI Engineer** | Agent tools, disclosure, cache |
| [008-tui-expert-lens](./008-tui-expert-lens.md) | **TUI expert** | Overlay FSM, keys, chrome DRY |
| [009-ux-ui-designer-lens](./009-ux-ui-designer-lens.md) | **UX/UI designer** | Journey, hierarchy, copy |
| [010-solid-dry-ownership](./010-solid-dry-ownership.md) | Eng | Owners, ban list |
| [011-implementation-plan](./011-implementation-plan.md) | Eng | Waves W0–W4 |
| [012-acceptance-criteria](./012-acceptance-criteria.md) | PO + QA | Measurable gates |
| [013-cross-ref-index](./013-cross-ref-index.md) | All | Paths + related specs |
| [014-registry-source-matrix](./014-registry-source-matrix.md) | Eng | Hermes ∪ peer registries |
| [015-registry-implementation](./015-registry-implementation.md) | Eng | WR wave code anchors + e2e |
| [proof/](./proof/) | QA | Wave proofs (W1–W4 + WR filled; see [013](./013-cross-ref-index.md)) |

## Waves (summary)

| Wave | Outcome |
|------|---------|
| **W0** | This pack; mark gap 028 current-state stale |
| **WR** | Registry super-set: Hermes sources + Pi/OpenClaw/Claude/Codex bridges + e2e |
| **W1** | CLI clap ≡ slash (surface parity) |
| **W2** | Unified TUI marketplace (search → inspect → scan → trust → done) |
| **W3** | Doctor hub health + index freshness |
| **W4** | Ed25519/TOFU signed taps (true publisher identity) |

Detail: [011-implementation-plan](./011-implementation-plan.md).

## Explicit non-goals

- Electron/desktop Skills page clone
- Full ClawHub publish marketplace product (optional after W2)
- Replacing regex guard (stays as defense-in-depth under signatures)
- Changing progressive-disclosure *runtime* semantics outside install/trust
- Chasing Hermes breadth for its own sake without surface parity

## Related

- [`../001-gap-analysis-v14/028-skills-hub-trusted-taps/`](../001-gap-analysis-v14/028-skills-hub-trusted-taps/) — crypto taps intent; **current-state partially stale** (see [013](./013-cross-ref-index.md))
- [`../002-tui-hemes-vs-edgecrab/`](../002-tui-hemes-vs-edgecrab/) — TUI craft baseline
- [`../017-hermes-vs-edgecrab/`](../017-hermes-vs-edgecrab/) — harness first-principles rubric
- [`../018-agent-harness/`](../018-agent-harness/) — DRY/SOLID ownership culture
- User docs: [`site/src/content/docs/features/skills.md`](../../site/src/content/docs/features/skills.md)
