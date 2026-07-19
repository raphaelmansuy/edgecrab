# Proof WI — Inspect Capability UX

**Status:** implemented (2026-07-18)  
**Criteria:** [016](../016-inspect-capability-ux.md) · [008](../008-tui-expert-lens.md) · [009](../009-ux-ui-designer-lens.md)

## Claim

`MarketplaceMode::Inspect` is a SKILL.md-first dossier (capability before trust). Install from SearchRemote requires Inspect (session set) or ConfirmSafe; Safe path uses a confirm strip. No new HTTP from TUI — reuses proactive `preview_install_scan` / Guard cache.

## Evidence checklist

- [x] Spec: [016-inspect-capability-ux.md](../016-inspect-capability-ux.md)
- [x] Pure model: `crates/edgecrab-cli/src/app/skill_inspect_view.rs` (`SkillInspectModel`, SKILL.md-first files, capabilities)
- [x] `RemoteSkillEntry` plumbs `url` / `repo` / `path` from `SkillMeta`
- [x] Inspect render uses dossier when mode is `Inspect { preview_scroll }`
- [x] SearchRemote `i` → `RequestInstall` (Inspect first if not yet inspected)
- [x] Inspect `e` → Guard review-only; Esc returns to Inspect
- [x] Safe cached preview → `ConfirmSafe` strip before commit
- [x] Done banner aftercare: `/skills remove {name}`
- [x] Empty/notice CTAs via `marketplace_notice_cta`
- [x] Footer: Enter=inspect / `i`=install (no “Enter installs” lie)
- [x] Unit tests offline (keymap + `SkillInspectModel` fixture)

## Keymap / model tests

| Test | Asserts |
|------|---------|
| `search_enter_inspects_when_selected` | Enter → InspectSelected |
| `search_i_requests_install_when_selected` | `i` → RequestInstall |
| `inspect_e_opens_evidence` | `e` → OpenEvidence |
| `inspect_scroll_and_retry` | ↓ scroll / `s` retry |
| `confirm_safe_enter_commits` | Enter → ConfirmSafeInstall; Esc → Back |
| `inspect_footer_mentions_evidence` | Footer truth |
| `skill_md_first_ordering` | SKILL.md first |
| `parse_frontmatter_and_capabilities` | Frontmatter + bullets + dossier lines |
| `notice_cta_token` | Rate-limit / timeout CTAs |

## Manual demo script

1. `/skills hub` → `/` search → select a result → **Enter**
2. Confirm **WHAT IT CLAIMS** shows SKILL.md (or “Fetching…”) when scan ready
3. Press **e** → Guard evidence → Esc → back to Inspect
4. Press **i** on Safe skill → Confirm strip → Enter → Done with remove aftercare
5. From SearchRemote, first **i** on a never-inspected row opens Inspect (no blind install)

## Ban list verification

- No new HTTP paths in CLI for Inspect body
- Guard remains sole allow/deny policy
- Capability block labeled claims/contents, not permissions
