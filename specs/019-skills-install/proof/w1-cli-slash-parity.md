# Proof W1 — CLI ≡ Slash Parity

**Status:** implemented (2026-07-18 close-out)  
**Criteria:** [012](../012-acceptance-criteria.md) · [011](../011-implementation-plan.md)

## Claim

Skills Hub actions available via `/skills …` (`handle_skills_hub_slash`) are reachable via `edgecrab skills …`. New clap variants map to the same slash façade (DRY); list/view/search/install/update/remove/import-from/sources keep dedicated CLI paths that call the same install/search APIs. Install supports `--force`, `--trust`, `-y`/`--yes`, and `--json` with shared stage vocabulary.

## Evidence checklist

- [x] Action coverage table (slash vs clap) attached below  
- [x] Clap parse tests for tap/trust/inspect/audit/snapshot/lock  
- [x] Hub actions delegate through `skills_command_to_hub_slash` → `handle_skills_hub_slash`  
- [x] Install supports `--force` / `--trust` / `-y` / `--json` on dedicated CLI path  
- [x] Caution/dangerous TempDir fixture coverage (tools lib + WR e2e)  
- [x] Unit: clap parse tests green  
- [x] Smoke: `EDGECRAB_HOME` TempDir for `skills tap list` / `skills lock`  

## Action coverage table

| Action | Slash | CLI | Notes |
|--------|-------|-----|-------|
| list | `/skills` (local browser) | `edgecrab skills list` | Dedicated |
| view | `/skills view` | `edgecrab skills view` | Dedicated |
| inspect | `/skills inspect [--scan]` | `edgecrab skills inspect [--scan]` | Via hub_slash |
| search / hub | `/skills search\|hub` | `edgecrab skills search [-S]` | Dedicated + source filter |
| install | `/skills install [--force\|-y\|--trust]` | `edgecrab skills install [-f\|-y\|--trust\|--json]` | Dedicated façade + stages |
| import-from | `/skills import-from` | `edgecrab skills import-from [-y\|--json]` | Dedicated |
| sources / catalog | `/skills sources\|catalog` | `edgecrab skills sources` | Dedicated |
| trust / untrust / trusted | `/skills trust\|untrust\|trusted` | `edgecrab skills trust\|untrust\|trusted` | Via hub_slash |
| check | `/skills check` | `edgecrab skills check` | Via hub_slash |
| update | `/skills update` | `edgecrab skills update` | Dedicated |
| remove | `/skills remove` | `edgecrab skills remove` | Dedicated |
| audit | `/skills audit [--deep\|log]` | `edgecrab skills audit [--deep\|--log]` | Via hub_slash |
| tap list/add/remove | `/skills tap …` | `edgecrab skills tap list\|add\|remove` | Via hub_slash |
| snapshot export/import | `/skills snapshot …` | `edgecrab skills snapshot export\|import` | Via hub_slash |
| opt-out / opt-in / reset | `/skills opt-out\|opt-in\|reset` | `edgecrab skills opt-out\|opt-in\|reset` | Via hub_slash |
| lock | `/skills lock` | `edgecrab skills lock` | Via hub_slash |
| index | `/skills index refresh\|status` | `edgecrab skills index refresh\|status` | Via hub_slash |

## Transcripts

```text
$ EDGECRAB_HOME=<tmpdir> edgecrab skills tap list
# lists default / configured taps (Hermes DEFAULT_TAPS seed when empty)

$ EDGECRAB_HOME=<tmpdir> edgecrab skills lock
No hub-installed skills (lock file empty).

$ EDGECRAB_HOME=<tmpdir> edgecrab skills --help
# shows tap, trust, untrust, trusted, audit, snapshot, check, inspect,
# opt-out, opt-in, reset, lock, index alongside list/view/search/install/…

# Caution / dangerous (TempDir; tools lib + WR e2e — not live CLI binary):
#   install_with_force_caution_only / install_with_yes_caution_only_not_dangerous
#   install_dangerous_blocked_without_trust / install_dangerous_with_trust_flag
#   WR e2e: dangerous_still_needs_trust
# Policy: -y ≡ caution override only; Dangerous still requires --trust.
```

Clap unit tests:

- `parse_skills_tap_add_subcommand`
- `parse_skills_trust_and_inspect_scan`
- `parse_skills_audit_snapshot_lock`
- `parse_skills_install_yes_json_flags`

## Sign-off

| Role | Date | OK |
|------|------|----|
| AI Engineer | 2026-07-18 | yes |
| Product Owner | 2026-07-18 | yes (clap ≡ slash mapping) |
