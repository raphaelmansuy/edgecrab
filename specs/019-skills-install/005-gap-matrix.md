# 005 — Gap Matrix (Hermes / EdgeCrab / Exceed Target)

**Cross-ref:** [003](./003-hermes-reference.md) · [004](./004-edgecrab-current-state.md) · [011](./011-implementation-plan.md)

Legend: **H** Hermes lead · **E** EdgeCrab lead · **T** tie · **∅** neither · **→** target wave

## Capability matrix

| Capability | Hermes | EdgeCrab | Target | Wave |
|------------|--------|----------|--------|------|
| Quarantine → scan → commit | Yes | Yes | Keep | — |
| Local path install | No (first-class) | Yes | Keep lead | — |
| `--force` caution override | Yes | Yes | Keep | — |
| Dangerous override with audit | No (hard block) | `--trust` + hash | Keep lead | — |
| Multi-source search | Yes | Yes | Keep | — |
| Central index (own CI) | Yes | Consumes Hermes URL | EdgeCrab index + Hermes fallback | W3 |
| GitHub taps (unsigned) | Yes | Yes | Keep | — |
| Signed publisher taps | **No in code** | No | Ed25519 + TOFU | W4 |
| CLI browse/inspect/audit/tap/trust/snapshot | Yes | Partial/missing | Full clap ≡ slash | W1 |
| Slash hub surface | Yes | Yes (rich) | Keep | — |
| TUI scan theatre | Thin | Strong overlay | Unified marketplace FSM | W2 |
| CLI install storytelling | Rich panels | Thin | Structured stages + `--json` | W1–W2 |
| Lock + audit log | Yes | Yes | Keep | — |
| Snapshot export/import | Yes | Yes (slash) | CLI + slash | W1 |
| Publish to registry | Yes | No | Optional post-W2 | later |
| Bundled sync / opt-out | Yes | Yes | Keep | — |
| Doctor hub health | Weak/indirect | Count only | Lock/taps/token/orphans | W3 |
| Agent `skills_hub` tool | Via CLI/tools | Yes | Keep; façade only | — |
| Blueprints post-install | Yes | No | Optional | later |
| Write-approval staging | Yes | Yes | Keep | — |

## Surface parity matrix

| Action | Hermes CLI | EdgeCrab slash | EdgeCrab CLI | Target CLI |
|--------|------------|----------------|--------------|------------|
| list | ✓ | ✓ | ✓ | ✓ |
| view / inspect | ✓ | ✓ | view only | inspect + `--scan` |
| search / browse | ✓ | ✓ | search (weaker) | search + browse |
| install | ✓ | ✓ | ✓ | + `--force/--trust/-y/--json` |
| trust / untrust | — | ✓ | ✗ | ✓ |
| check / update | ✓ | ✓ | update | ✓ |
| audit / audit log | ✓ | ✓ | ✗ | ✓ |
| tap list/add/remove | ✓ | ✓ | ✗ | ✓ |
| snapshot export/import | ✓ | ✓ | ✗ | ✓ |
| opt-out / opt-in / reset | ✓ | ✓ | ✗ | ✓ |
| remove / uninstall | ✓ | ✓ | ✓ | ✓ |
| publish | ✓ | ✗ | ✗ | optional later |

## Trust & security matrix

| Concern | Hermes | EdgeCrab | Exceed |
|---------|--------|----------|--------|
| Pattern scanner | ~121 regex | ~65 pattern_ids + brainworm | Keep SoT in `threat_patterns`; grow with evidence |
| Scan cache attestation | Yes | Partial / preview path | Align cache key = content hash + scanner version |
| Force vs Dangerous | Force ≠ Dangerous | Force ≠ Dangerous | Invariant tests |
| User dangerous approve | No | Hash-bound | Keep + TUI |
| Publisher identity | Hardcoded trusted repos | Same + taps list | **Signed manifests** |
| SSRF on URL fetch | Yes | Yes | Keep |
| Symlink / path escape | Yes | Yes | Keep |

## UX / TUI matrix

| Dimension | Hermes | EdgeCrab | Exceed |
|-----------|--------|----------|--------|
| Installed browser | Ink categories | ratatui browser | Keep |
| Remote search overlay | Weak in Ink hub | Remote browser | Merge into marketplace |
| Scan findings UI | CLI Rich | Skill Guard overlay | Stream stages into marketplace |
| Keybindings consistency | Ink overlay keys | Trust overlay keys | Single keymap doc |
| Progress stages visible | CLI yes / TUI no | Partial | Always: Fetch→Quarantine→Scan→Gate→Commit |
| Gateway text UX | Slash | Shared slash | Keep; no policy fork |

## Architecture / DRY matrix

| Concern | Hermes | EdgeCrab | Exceed |
|---------|--------|----------|--------|
| Shared do_* façade | `hermes_cli/skills_hub.py` | `hub_slash` + free fns | Explicit `SkillsInstallFacade` API surface |
| Dual trust enums | Lower risk (Python) | Dual Rust models | Collapse to one |
| Severity SoT | threat_patterns aligned | Mapping debt | One Severity |
| TUI policy in UI | Low (RPC) | Overlay uses DTOs | Enforce: no GitHub in CLI UI |

## Scoring (honest, unweighted)

```text
  Area                 Hermes   EdgeCrab   Target after W4
  ───────────────────  ───────  ────────   ───────────────
  Pipeline integrity   A        A          A
  Trust gates          B+       A−         A
  Publisher identity   C        C          A−
  CLI surface          A        C+         A
  TUI install craft    C+       A−         A
  Index sovereignty    A        B          A−
  Doctor               C        D+         A−
  DRY ownership        B        B−         A
  ───────────────────  ───────  ────────   ───────────────
  Composite            B+       B+         A
```

EdgeCrab does **not** need to win every Hermes column (publish/blueprints). It must win **parity + theatre + publisher truth + doctor**.

## Priority order (value / effort)

| Rank | Gap | Wave | Rationale |
|------|-----|------|-----------|
| 1 | CLI ≡ slash | W1 | Unblocks scripting, CI, non-TUI users |
| 2 | Marketplace TUI FSM | W2 | Converts trust lead into daily habit |
| 3 | Doctor hub | W3 | Prevents silent rate-limit / orphan failure |
| 4 | Signed taps | W4 | Real exceed; closes typosquat class |
| 5 | Own index CI | W3 | Sovereignty; fallback keeps Hermes URL |
| 6 | Publish | later | Ecosystem nice-to-have |
| 0 | Registry super-set (WR) | WR | Hermes source parity + peer bridges before W4 |

## Registry super-set (WR)

See [014](./014-registry-source-matrix.md). Target: Hermes 10 sources + default taps + `git:`/`npm:`/`@owner/slug` + `import-from` peer homes + mocked e2e.
