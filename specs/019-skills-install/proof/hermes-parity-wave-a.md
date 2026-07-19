# Proof — Hermes skills parity Wave A (search / browse / install)

**Date:** 2026-07-18  
**Plan:** Hermes ↔ EdgeCrab skills parity (Wave A)

## Acceptance

| Criterion | Evidence |
|-----------|----------|
| Empty skills.sh browse uses sitemap when network allows | `browse_skills_sh_sitemap` in `skills_hub/mod.rs` — index `https://www.skills.sh/sitemap.xml` → `sitemap-skills-*.xml`, gzip Accept-Encoding, cache key `skills_sh_sitemap_v1` |
| Seed/cache fallback when sitemap fails | `browse_skills_sh_seed_fallback` (2 seeds + 15m cache); no GITHUB_TOKEN CTA on skills.sh 429 |
| `edgecrab skills browse` + `/skills browse` | clap `SkillsCommand::Browse` + hub_slash `browse` (`--source/--page/--page-size/--json`) |
| Short-name install resolve / ambiguity | `resolve_short_name_identifier` — exact name match; ambiguity table; no silent wrong pick |
| TUI/CLI polish intact | Prior marketplace keyboard + CTA work unchanged |
| Browse/hub unified (Hermes brand) | `hub` aliases `browse` (not search). TUI `/skills browse\|hub` → SearchRemote empty. Slash empty `search` → usage tip pointing at browse. Search limit 25. CLI `skills hub` = `skills browse`. |

## Tests

```bash
cargo test -p edgecrab-tools skills_sh_sitemap
cargo test -p edgecrab-tools short_name_resolve
cargo test -p edgecrab-tools hub_alias_dispatches
cargo test -p edgecrab-tools --test skills_hub_sources_e2e
cargo test -p edgecrab-cli skills_browse_opens_remote
cargo test -p edgecrab-cli parse_skills_hub_alias
```

Unit fixtures (no network):

- `skills_sh_sitemap_index_locs_filter_skill_maps`
- `skills_sh_sitemap_skill_metas_parse_owner_repo_skill`
- `short_name_resolve_skips_qualified_identifiers`
- `hub_alias_dispatches_to_browse_not_search`

## Remaining (explicit — not Wave A)

See Wave B/C in `proof/hermes-parity-wave-bc.md`.
