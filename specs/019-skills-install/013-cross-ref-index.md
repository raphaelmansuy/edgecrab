# 013 — Cross-Reference Index

**Cross-ref:** [000](./000-overview.md)

## Spec pack (019)

| Doc | Path |
|-----|------|
| Overview | [000-overview.md](./000-overview.md) |
| Five WHYs | [001-five-whys.md](./001-five-whys.md) |
| First principles | [002-first-principles.md](./002-first-principles.md) |
| Hermes reference | [003-hermes-reference.md](./003-hermes-reference.md) |
| EdgeCrab current | [004-edgecrab-current-state.md](./004-edgecrab-current-state.md) |
| Gap matrix | [005-gap-matrix.md](./005-gap-matrix.md) |
| Product Owner | [006-product-owner-lens.md](./006-product-owner-lens.md) |
| AI Engineer | [007-ai-engineer-lens.md](./007-ai-engineer-lens.md) |
| TUI expert | [008-tui-expert-lens.md](./008-tui-expert-lens.md) |
| UX / UI designer | [009-ux-ui-designer-lens.md](./009-ux-ui-designer-lens.md) |
| SOLID / DRY | [010-solid-dry-ownership.md](./010-solid-dry-ownership.md) |
| Implementation | [011-implementation-plan.md](./011-implementation-plan.md) |
| Acceptance | [012-acceptance-criteria.md](./012-acceptance-criteria.md) |
| Proofs | [proof/](./proof/) |
| Registry matrix | [014-registry-source-matrix.md](./014-registry-source-matrix.md) |
| Registry impl | [015-registry-implementation.md](./015-registry-implementation.md) |
| Inspect capability UX | [016-inspect-capability-ux.md](./016-inspect-capability-ux.md) |
| Marketplace browse by source | [017-marketplace-browse.md](./017-marketplace-browse.md) |
| WR proof | [proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md) |
| Inspect proof | [proof/wi-inspect-capability.md](./proof/wi-inspect-capability.md) |

## Hermes Agent (absolute paths)

| Concern | Path |
|---------|------|
| Hub core | `/Users/raphaelmansuy/Github/03-working/hermes-agent/tools/skills_hub.py` |
| Guard | `…/tools/skills_guard.py` |
| Sync | `…/tools/skills_sync.py` |
| CLI do_* | `…/hermes_cli/skills_hub.py` |
| Argparse | `…/hermes_cli/subcommands/skills.py` |
| Ink TUI | `…/ui-tui/src/components/skillsHub.tsx` |
| Index CI | `…/.github/workflows/skills-index.yml` |
| User docs | `…/website/docs/user-guide/features/skills.md` |

## EdgeCrab (repo-relative)

| Concern | Path |
|---------|------|
| Hub module | `crates/edgecrab-tools/src/tools/skills_hub/` |
| Slash | `…/skills_hub/hub_slash.rs` |
| Sources / index | `…/skills_hub/sources.rs`, `index.rs` |
| Preview / approvals | `…/install_preview.rs`, `guard_approvals.rs` |
| Guard | `crates/edgecrab-tools/src/tools/skills_guard.rs` |
| Sync | `crates/edgecrab-tools/src/tools/skills_sync.rs` |
| Agent tools | `crates/edgecrab-tools/src/tools/skills.rs` |
| Threat SoT | `crates/edgecrab-security/src/threat_patterns.rs` |
| Skills subsystem | `crates/edgecrab-tools/src/skills/` |
| Clap | `crates/edgecrab-cli/src/cli_args.rs` (`SkillsCommand`) |
| Doctor | `crates/edgecrab-cli/src/doctor.rs` (`check_skills`) |
| Trust overlay | `crates/edgecrab-cli/src/app/skill_trust_overlay.rs` |
| Marketplace FSM | `crates/edgecrab-cli/src/app/skills_marketplace.rs` |
| Remote guard | `crates/edgecrab-cli/src/app/remote_skill_guard.rs` |
| Gateway | `crates/edgecrab-gateway/src/run.rs` |
| Config | `crates/edgecrab-core/src/config.rs` (`SkillsConfig`) |
| User docs | `site/src/content/docs/features/skills.md` |
| AGENTS overview | `AGENTS.md` (Skills Hub section) |

## Related EdgeCrab specs

| Spec | Relation |
|------|----------|
| [`../001-gap-analysis-v14/028-skills-hub-trusted-taps/`](../001-gap-analysis-v14/028-skills-hub-trusted-taps/) | Crypto taps intent → **superseded for implementation by 019 W4** |
| [`../001-gap-analysis-v14/033-skill-bundles/`](../001-gap-analysis-v14/033-skill-bundles/) | Bundles (runtime), not install hub |
| [`../001-gap-analysis-v14/021-curator-subsystem/`](../001-gap-analysis-v14/021-curator-subsystem/) | Curator of agent-created skills |
| [`../001-gap-analysis-v14/031-promptware-brainworm-defense/`](../001-gap-analysis-v14/031-promptware-brainworm-defense/) | Threat pattern SoT feeding guard |
| [`../002-tui-hemes-vs-edgecrab/`](../002-tui-hemes-vs-edgecrab/) | TUI craft baseline |
| [`../017-hermes-vs-edgecrab/`](../017-hermes-vs-edgecrab/) | Harness first-principles rubric |
| [`../018-agent-harness/`](../018-agent-harness/) | DRY/SOLID culture; cache law |

## Stale documentation notes (code is law)

| Claim | Reality (July 2026) |
|-------|---------------------|
| 028: EdgeCrab has “no tap concept” | **Stale** — `taps.json` + `/skills tap` exist; unsigned |
| 028: Hermes ships Ed25519 signed tap manifests | **Not found in Hermes code** — taps are GitHub repo lists + regex guard |
| Docs citing `skills_hub.rs` single file | **Stale** — module is `skills_hub/` |
| AGENTS.md “23 threat patterns” | Pattern inventory lives in `threat_patterns`; count may differ — cite SoT file |

When implementing W4, update 028 overview with a pointer to 019 rather than duplicating plans.

## External URLs

| Resource | URL |
|----------|-----|
| Hermes skills index (consumed by EdgeCrab) | `https://hermes-agent.nousresearch.com/docs/api/skills-index.json` |

## Proof status

| Wave | File | Status |
|------|------|--------|
| WR | [proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md) | Green (router + e2e; live HTTP optional) |
| W1 | [proof/w1-cli-slash-parity.md](./proof/w1-cli-slash-parity.md) | Implemented (`-y`/`--json` shipped) |
| W2 | [proof/w2-tui-marketplace.md](./proof/w2-tui-marketplace.md) | Implemented + 008 polish |
| W3 | [proof/w3-doctor-hub.md](./proof/w3-doctor-hub.md) | Implemented |
| W4 | [proof/w4-signed-taps.md](./proof/w4-signed-taps.md) | Implemented |
