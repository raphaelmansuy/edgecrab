# Proof — Hermes skills parity Wave B / C

**Date:** 2026-07-18

## Wave B — CLI lifecycle

| Feature | EdgeCrab surface |
|---------|------------------|
| `list-modified` | `edgecrab skills list-modified` / `/skills list-modified` → `skills_sync::list_user_modified_bundled_skills` |
| `diff` (bundled) | `edgecrab skills diff <name>` → `/skills diff-bundled`; `/skills diff` also falls through from pending-id miss |
| `repair-official` | `/skills repair-official <name\|all> [--restore]` |
| `publish` | `/skills publish <path> --to github\|clawhub [--repo owner/repo]` |
| write_approval slash | Existing `/skills pending\|approve\|reject\|diff\|approval` (CLI + TUI) |
| `--now` / `--deferred` | `InstallGate.now`; install flags on CLI + slash |
| Lock provenance | `LockEntry.source_url` + `scanner_version` (`skills-guard-v1`) |

## Wave C — authoring / ops

| Feature | Status |
|---------|--------|
| Web hub APIs | Documented catalog via `/skills web-hub` — **not mounted** on agent CLI by default (`web_hub.rs`) |
| Post-install blueprints | `blueprints::post_install_suggestion` — suggestion only, never silent cron |
| GitHub App auth | `github_auth::resolve_github_token` — PAT → `gh auth token` → App installation via `gh api` |
| Skill bundles | `/skills bundles show\|install <file>` — light JSON/txt groups; full marketplace deferred |
| Rich disclaimer panels | Unchanged caution/dangerous gate copy + publish scan refuse on Dangerous |

## Explicit non-goals (still)

- Replacing EdgeCrab TUI with Hermes Ink overlay
- Building EdgeCrab’s own skills-index CI (keep consuming Hermes index)
- Full dashboard HTTP API without product opt-in
