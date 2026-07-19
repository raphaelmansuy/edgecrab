# Proof WR — Registry Super-Set E2E

**Status:** green (2026-07-18; router owns search+fetch)  
**Criteria:** [014](../014-registry-source-matrix.md) · [015](../015-registry-implementation.md)

## Claim

Every Hermes `source_id` plus peer aliases (`git:`, `@owner/slug`, `npm:`) and `import-from` peer homes go through one façade + quarantine pipeline; `SkillSourceRouter` dispatches **search and fetch** adapters; curated catalog / TTL / provider filters derive from `HUB_CATALOG`; mocked e2e suite is green without live network.

## Evidence checklist

- [x] `cargo test -p edgecrab-tools --test skills_hub_sources_e2e` green (**17 passed**)
- [x] `SkillSourceRouter` registers all `ALL_SOURCE_IDS` + `classify_source_id`
- [x] `fetch_bundle_for_identifier` routes through router (adapters own concrete paths)
- [x] `search_hub` live fan-out via `SkillSourceRouter::search_groups`
- [x] Default taps include huggingface + NVIDIA + gstack (`HUB_CATALOG`)
- [x] `well-known:` install path tested (mock HTTP; SSRF may block loopback — asserted)
- [x] `normalize_identifier` for git:/npm:/@owner
- [x] `import-from` fixture uses quarantine
- [x] Provider filter helpers (`openai` excludes clawhub-as-provider)
- [x] Path traversal bundle rejected on install (e2e)
- [x] npm extract → local bundle → install nested skill

## Test inventory

| Test | Status |
|------|--------|
| hermes_source_ids_catalogued | pass |
| normalize_peer_aliases | pass |
| default_taps_include_hermes_parity | pass |
| sources_catalog_lists_peers_and_providers | pass |
| peer_external_presets_cover_agents | pass |
| import_from_uses_quarantine_not_raw_copy | pass |
| local_install_goes_through_guard | pass |
| dangerous_still_needs_trust | pass |
| npm_spec_parse | pass |
| npm_fixture_tarball_extract_finds_skills | pass |
| npm_extract_then_install_nested_skill | pass |
| federation_endpoints_default | pass |
| well_known_install_via_mock_http | pass |
| skill_source_router_classifies_and_registers | pass |
| search_hub_dispatches_via_router_adapters | pass |
| provider_filter_openai_excludes_clawhub_helper | pass |
| path_traversal_bundle_rejected_on_install | pass |

## Sign-off

| Role | Date | OK |
|------|------|----|
| AI Engineer | 2026-07-18 | yes |
| Product Owner | 2026-07-18 | yes |
|