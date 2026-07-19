# Proof W2 — TUI Marketplace

**Status:** implemented + 008 polish (2026-07-18)  
**Criteria:** [012](../012-acceptance-criteria.md) · [008](../008-tui-expert-lens.md) · [009](../009-ux-ui-designer-lens.md)

## Claim

A Skills Marketplace FSM in `skills_marketplace.rs` owns BrowseInstalled ↔ SearchRemote ↔ import-from ↔ staged install theatre → Skill Guard gate → done, reusing Skill Guard for policy (no duplicate allow/deny).

## Evidence checklist (MVP + polish)

- [x] Module path: `crates/edgecrab-cli/src/app/skills_marketplace.rs`
- [x] Stage UI shows Fetch → Quarantine → Scan → Gate → Commit
- [x] Guard overlay reused; default focus ≠ Force (Cancel)
- [x] Esc preserves search query from Guard
- [x] `BrowseInstalled` ↔ `SearchRemote` (`/`/`s` / Esc / `l`)
- [x] Provider filter cycle (`p`) wired into `search_hub` source_filter
- [x] Import-from picker (`m`) → `import_skills_from` (same pipeline)
- [x] Guard `f` = force (caution), `t` = trust+install (dangerous), `v`/`g` jump-to-finding
- [x] Theatre / Done / Error / import chrome share marketplace accent colors
- [x] Footer: Enter=inspect / `I`=install (Shift) while typing; Inspect keeps lowercase `i`
- [x] Keymap + render unit tests; no network

## Keymap tests

| Test name | Asserts |
|-----------|---------|
| `browse_slash_goes_search` | `/` from BrowseInstalled → GoSearchRemote |
| `browse_shift_s_goes_search_lowercase_s_is_noop` | `S` → search; lowercase `s` → Noop (types into filter) |
| `open_remote_skill_selector_none_opens_search_remote` | empty hub/`R` → SearchRemote |
| `open_skill_selector_sets_browse_installed_marketplace_home` | bare `/skills` → BrowseInstalled |
| `search_esc_returns_to_installed` | Esc → GoBrowseInstalled |
| `search_shift_p_cycles_provider_lowercase_p_is_noop` | `P` cycles; lowercase `p` types |
| `search_enter_inspects_when_selected` | Enter → InspectSelected |
| `search_shift_i_requests_install_lowercase_i_is_noop` | `I` → RequestInstall; `i` → Noop |
| `search_remote_lowercase_letters_type_into_query_not_install` | typing `si` stays in query |
| `browse_installed_lowercase_s_stays_in_filter` | `s` filters installed list |
| `inspect_mode_lowercase_a_does_not_mutate_query` | Inspect does not push_char |
| `import_digit_selects_peer` | `2` → ImportPeer(1) |
| `default_trust_action_is_cancel` | Cancel default |
| `render_install_theatre_smoke` | Stage labels in buffer |
| `f_forces_caution_t_trusts_dangerous` | Guard shortcuts (skill_trust_overlay) |

## Manual demo script

1. Bare `/skills` → **Skills · Installed** (local); footer shows `/ S marketplace`  
2. Press `/` or `S` → **SearchRemote** — skills list fills **without typing** (browse)  
3. `/skills hub` or `/skills search` (no query) → SearchRemote browse directly  
4. `[` `]` / chips / `S` pick source → rebrowse that marketplace; type lowercase to filter  
5. Enter → Inspect; `i` → theatre / confirm / Guard (Inspect modal)  
6. In SearchRemote, `I` (Shift) installs; lowercase `i` types into the filter  
7. `M` from Installed → import-from peer picker  
8. Esc / `L` from SearchRemote → Installed; Done banner → Enter back to installed  

## Entry discoverability (2026-07-18)

- [x] `open_remote_skill_selector(None)` → SearchRemote (not BrowseInstalled)
- [x] Bare `open_skill_selector` sets BrowseInstalled and deactivates remote
- [x] Tests: `open_skill_selector_sets_browse_installed_marketplace_home`, `open_remote_skill_selector_none_opens_search_remote`

## Browse without typing + by source ([017](../017-marketplace-browse.md))

- [x] Empty query schedules `search_hub("")` browse (index + live fan-out)
- [x] Source chip strip (wide) + `S` SourcePick overlay + `[`/`]` / digits
- [x] Footer: source pick / inspect truth
- [x] Tests: `search_remote_s_opens_source_pick`, `search_hub_empty_query_browses_seeded_index`

## Virtual scrolling (large catalogs)

- [x] `browser_virtual_window` / `browser_virtual_range_label` in `overlay_layout.rs`
- [x] `render_virtualized_list` (viewport rows + scrollbar) in `browser_chrome.rs`
- [x] Marketplace remote list, installed skills, remote plugins: only viewport `ListItem`s
- [x] Footer range chip (`12–31 of N`) + compact keys; full keymap on `?`
- [x] Mouse wheel scrolls virtual list selection (skills marketplace / installed / plugins)
- [x] `FuzzySelector::list_viewport_rows` + `page_by` / `page_up` / `page_down`
- [x] Tests: `browser_virtual_window_clamps_and_labels`, `marketplace_scale_catalog_only_materializes_viewport`, `page_by_virtual_scrolls_large_marketplace_lists`

## Keyboard: type vs action (2026-07-18)

- [x] SearchRemote / BrowseInstalled: lowercase → filter; Shift+letter → actions (`I`/`R`/`P`/`M`/`L`/`S`)
- [x] Inspect modal keeps lowercase `i`/`e`/`s` and does not mutate the search query
- [x] List columns pad source (14) + action (8) so labels never smash (`Unified Index` / `install`)

## Browse resilience + local-first typing (2026-07-18)

- [x] skills.sh: ≤2 sequential seeds + 15m disk cache; partial/stale on 429 (no 16-way fan-out)
- [x] Source-aware CTAs: skills.sh 429 ≠ `GITHUB_TOKEN` (`marketplace_notice_cta`)
- [x] Browse fetch up to `MARKETPLACE_BROWSE_FETCH_MAX` (10k); no GitHub-100 / registry-200 clamp — TUI virtual scroll + CLI `--page` navigate the full set; typed search still ≤50
- [x] Empty groups keep notices
- [x] VoltAgent: README GitHub-link harvest (or honest empty notice)
- [x] Typing: local fuzzy filter; network only on source/`r`/query ≥2 chars; browse snapshot restore
- [x] Apply preserves selection by identifier; list empty-state taxonomy (loading / fail+CTA / empty / no matches)
- [x] Tests: `skills_sh_browse_is_frugal_two_seeds`, `harvest_awesome_list_extracts_github_skill_refs`, `notice_cta_skills_sh_429_does_not_suggest_github_token`, e2e notice/harvest guards

## Loading + mouse dismiss (2026-07-18)

- [x] In-flight browse/search advances overlay spinner via `tick_spinner` even while `DisplayState::Idle` (async `search_hub` already non-blocking)
- [x] Loading chrome: `compact_spinner_frame` + elapsed in title / empty list / detail (`loading_status_line`)
- [x] `dismiss_skills_marketplace` cancels in-flight request id + clears FSM; left-click dismisses SearchRemote/Inspect/BrowseInstalled
- [x] Modal click-outside (`marketplace_popup_rect` / `rect_contains_cell`): SourcePick / ImportFrom / ConfirmSafe → Back; Done/Error → DismissDone; Installing ignores click
- [x] Tests: `marketplace_loading_spinner_ticks_while_inflight`, `mouse_dismisses_skills_marketplace_search_remote`, `mouse_dismiss_skips_installing_theatre`, `mouse_outside_source_pick_goes_back`, `marketplace_popup_rect_hit_test`

## Wave C overlay polish (2026-07-18)

- [x] Source chips show loading accent (`[⠋ Label]`)
- [x] Detail pane skeleton placeholders while browsing/searching
- [x] Install theatre shows current-stage elapsed (`skills_install_stage_started`)
- [x] `?` help expands to multi-line footer (4 rows), not a single truncated line
- [x] Mouse: list click selects row; detail click focuses pane; header/footer dismiss; ConfirmSafe `[Install]`/`[Cancel]` hit targets; double-click row → Inspect
- [x] Tests: `mouse_list_click_selects_remote_skill_row`

## On-demand browse pages + sliding cache (2026-07-18)

- [x] `search_hub_progressive`: sync index/skills.sh cache first paint; stream live groups via `FuturesUnordered` (no remote index await on critical path)
- [x] TUI: `RemoteSkillSearchPartial` merges rows while `inflight`; initial browse requests `MARKETPLACE_BROWSE_PAGE_SIZE` (80), not 10k
- [x] `BrowsePageCache` + `maybe_extend_browse_cache` on scroll/PgDn/wheel from unified-index / skills.sh disk slices
- [x] Footer: `Browsing…` / `N–M of T · loading…` — never `0 of 0` while inflight
- [x] Tests: `search_hub_progressive_emits_index_before_return`, `progressive_partial_paints_rows_while_still_inflight`, `browse_extend_advances_fetch_cursor_without_clearing`, `footer_never_zero_of_zero_while_inflight`

## Unblock browse (2026 TUI practice, 2026-07-18)

- [x] No blank wipe on rebrowse — retain prior rows until first partial of new `request_id`; empty+inflight shows list skeleton stubs
- [x] Cheap apply: lock + top-level installed-dir `HashSet` once (no recursive `find_skill_md` per row); guard preview debounced ~250ms on partials
- [x] True cross-source fan-out: curated + registry + taps share one `FuturesUnordered` (no curated→registry phase barrier); taps stream per tap
- [x] Browse skips GitHub description `join_all` hydrate; cold skills.sh returns first page then finishes sitemap cache in background
- [x] Cancel-on-navigate: chip / `[` `]` / SourcePick select aborts prior task (`AbortHandle`) and rebrowses
- [x] Tests: `browse_reschedule_retains_prior_rows_until_first_partial`, `build_remote_skill_entries_uses_dir_set_not_recursive_walk`, `source_jump_cancels_and_rebrowses_while_catalog_loading`, `cross_source_pool_emits_fast_before_slow`, `skills_sh_page_first_slice_sorted_and_capped`

## Browse pagination SoT (2026-07-19)

- [x] Root cause: page-first returned ~80 rows without writing `skills_sh_sitemap_v1`; extend only reads that cache → empty → sticky `exhausted` → Down stuck at `of 78`
- [x] Page-first writes partial sitemap cache **before** return; finish upgrades mid-walk + on complete
- [x] `skills_sh_sitemap_cache_len` / `unified_index_len` — SoT for exhaust + footer
- [x] `maybe_extend`: exhaust only when `fetch_cursor >= catalog_len`; cache miss stays retryable; tick retries when cache grows
- [x] Footer `marketplace_browse_range_label_with_catalog` uses SoT total when larger than loaded window
- [x] Tests: `skills_sh_page_first_writes_cache_enabling_extend_slice`, `browse_extend_grows_past_first_page_from_sitemap_cache`, `browse_extend_exhausts_only_past_catalog_end`, `footer_uses_catalog_total_when_larger_than_loaded`

## Per-source CatalogStore paging (2026-07-19)

- [x] `SkillCatalogStore` / `FilterCatalogStore::for_filter` in `catalog_store.rs` — `page` / `total` / `complete` / `ensure` per marketplace chip
- [x] GitHub chips: `browse_github_cache_slice` + `github_cache_len` / `complete` from full tree cache; PgDn is local skip/take
- [x] skills.sh: `ensure_skills_sh_sitemap_catalog` outside 12s timeout; `.complete` marker; **seed never poisons** sitemap SoT
- [x] ClawHub: `ensure_clawhub_listing_catalog` (large `limit`) + `browse_clawhub_cache_slice`; truncated-80 first paint is not complete
- [x] CLI `maybe_extend_browse_cache` uses CatalogStore for all filters; Ready/tick spawn ensure; chip cancel aborts ensure + resets cursor
- [x] **Dedup-aware virtual scroll:** `fetch_cursor` is SoT offset (not Vec index); after merge-by-identifier, empty unique growth still advances cursor and skips up to N duplicate pages so scroll cannot stall
- [x] Exhaust only when `complete && offset >= total`; incomplete cache shows footer `loading…`
- [x] **Gzip decode fix (live `of 78` bug):** setting `Accept-Encoding: gzip` with workspace `reqwest` (no `gzip` feature) left sitemap bodies as `\x1f\x8b…`; parse → 0 skills → ensure failed → seed-only UI stuck at ~78 with `loading…`. Fixed via `decode_skills_sh_sitemap_bytes` / `fetch_skills_sh_sitemap_text` (flate2). Live ensure → ~20k rows.
- [x] Tests: `catalog_page_github_slices_second_page`, `catalog_page_dedups_multi_root_provider`, `browse_openai_extend_grows_from_github_cache_without_network`, `browse_extend_incomplete_catalog_does_not_exhaust`, `browse_extend_skips_duplicate_sot_pages`, `chip_switch_resets_catalog_ensure_and_cursor`, `advance_after_page_moves_cursor_even_when_all_dups`, `clawhub_cache_slice_pages_past_first`, `decode_skills_sh_sitemap_bytes_handles_gzip_payload`, `ensure_skills_sh_sitemap_catalog_live` (ignored)

## TUI assessment (2026-07-19 Phase 0)

**How:** `EDGECRAB_HOME=~/.edgecrab/profiles/homelab cargo test -p edgecrab-tools --test assess_marketplace_catalog -- --ignored --nocapture` against live SoT on disk (CatalogStore `page`/`total`/`complete`).

**Binary:** PATH `~/.cargo/bin/edgecrab` is **Jun 12** (stale). Fresh build: `target/debug/edgecrab` **Jul 19**. Interactive TUI must use the workspace binary.

| Chip | total | complete | page2 unique | Verdict |
|------|------:|:--------:|-------------:|---------|
| all | 19913 | yes | 157 | PASS |
| openai | 44 | yes | — | PASS (catalog ≤80) |
| anthropic | 17 | yes | — | PASS (catalog ≤80) |
| huggingface | 25 | yes | — | PASS (catalog ≤80) |
| nvidia | 299 | yes | 80 | PASS |
| gstack | 59 | yes | — | PASS (catalog ≤80) |
| voltagent | 200 | yes | 80 | PASS page2 — **WARN** harvest may be capped at 200 |
| minimax | 1 | yes | — | PASS (catalog ≤80) |
| clawhub | 79 | yes | — | PASS small-complete — **WARN** 79 looks like truncated API marked complete |
| skills-sh | 19913 | yes | 80 | PASS (gzip ensure fixed; seed=78 left in `skills_sh_browse` only) |

**Not TUI chips (disk SoT exists, no filter paging path):**

| Cache | rows | Gap |
|-------|-----:|-----|
| browse_sh_catalog | 440 | No chip / CatalogStore filter |
| lobehub_index | 505 | No chip / CatalogStore filter |
| claude_marketplace_* | 3 | No chip / CatalogStore filter |
| federation_agentskills.io | 1 | No chip / CatalogStore filter |

**Interactive checklist (skills-sh on fresh binary):** with warm sitemap, footer should show `of 19913` (not `of 78`); PgDn past first page must grow loaded rows. If UI still shows `of 78`, the process is still the Jun 12 cargo binary.

## Sign-off

| Role | Date | OK |
|------|------|----|
| TUI expert | 2026-07-18 | yes |
| UX designer | 2026-07-18 | yes |
| AI Engineer | 2026-07-19 | yes (CatalogStore + dedup cursor + Phase 0 assess) |
