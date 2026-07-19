# 004 — EdgeCrab Current State (Code Is Law)

**Cross-ref:** [000](./000-overview.md) · [003](./003-hermes-reference.md) · [005](./005-gap-matrix.md)

Surveyed: EdgeCrab workspace July 2026.

## Core modules

| Path | Role |
|------|------|
| `crates/edgecrab-tools/src/tools/skills_hub/mod.rs` | Bundle, quarantine, install, lock, search entry |
| `skills_hub/sources.rs` | ClawHub, browse.sh, skills.sh, well-known, URL safety |
| `skills_hub/index.rs` | Unified index cache; seeds from Hermes skills-index URL |
| `skills_hub/hub_slash.rs` | Shared `/skills` dispatch (TUI + gateway) |
| `skills_hub/install_preview.rs` | Install scan preview DTOs |
| `skills_hub/snapshot.rs` | Snapshot export/import |
| `skills_hub/guard_approvals.rs` | Hash-bound dangerous trust store |
| `tools/skills_guard.rs` | Scan + `InstallPolicyContext` / `should_allow_install_with` |
| `tools/skills_sync.rs` | Bundled seed + opt-out |
| `tools/skills_ast_audit.rs` | Optional deep Python audit |
| `tools/skills.rs` | Agent tools: list/view/manage/hub |
| `edgecrab-security/src/threat_patterns.rs` | Threat pattern SoT |
| `edgecrab-tools/src/skills/` | Discovery, bundles, write_approval, curator, invocation |
| `edgecrab-cli/src/app/skill_trust_overlay.rs` | Skill Guard ratatui overlay |
| `edgecrab-cli/src/app/remote_skill_guard.rs` | Remote search → guard bridge |
| `edgecrab-cli/src/cli_args.rs` | Thin `SkillsCommand` enum |
| `edgecrab-cli/src/doctor.rs` | `check_skills` (count only) |
| `edgecrab-gateway/src/run.rs` | Gateway `/skills` → same hub slash |

**Note:** Specs that cite `skills_hub.rs` as a single file are stale — implementation is `skills_hub/`.

## Install pipeline

```text
User / agent
  ├─ TUI: /skills … → app handlers + overlays
  ├─ Gateway: /skills … → handle_skills_hub_slash
  ├─ CLI: edgecrab skills install | edgecrab install <spec>
  └─ Agent: skills_hub tool
        │
        ▼
normalize_source_identifier
        │
        ▼
fetch_bundle_for_identifier
  • curated: edgecrab:… / hermes-agent:… / openai:… / anthropics:…
  • GitHub: owner/repo[/path]  (GITHUB_TOKEN / GH_TOKEN)
  • registries: clawhub, browse.sh, skills.sh, agentskills.io, well-known
  • local path / optional-skills / official embedded
        │
        ▼
stage_bundle_in_quarantine → ~/.edgecrab/skills/.hub/quarantine/<name>-<uuid>/
        │
        ▼
skills_guard::scan_skill
        │
        ▼
should_allow_install_with(InstallPolicyContext { force, trusted_dangerous })
        │
        ▼
rename → ~/.edgecrab/skills/<name>/
update .hub/lock.json, audit.log
notify_hub_skills_mutated
```

`InstallGate { force, trust }` — `--force` = caution; `--trust` = dangerous (or pre-approved hash).

## Storage layout

```text
~/.edgecrab/skills/
  <skill>/SKILL.md
  .bundled_manifest
  .hub/
    lock.json
    taps.json
    audit.log
    guard_approvals.json
    quarantine/
    index-cache/
```

Opt-out bundled seed: `~/.edgecrab/.no-bundled-skills`.

## Slash surface (rich — TUI + gateway)

From `hub_slash.rs` (representative):

`inspect`, `hub`/`search`, `install`, `trust`/`untrust`/`trusted`, `check`, `snapshot`, `update`, `remove`/`uninstall`, `tap`/`taps`, `audit`, plus related handlers for reset/opt-out/opt-in/lock/index/catalog/sources/review/pending/approve (config/pending may live in `skills/` helpers).

Flags: `--force`, `--trust`; inspect supports `--scan`.

## CLI surface (thin)

`SkillsCommand` in `cli_args.rs`:

| Subcommand | Status |
|------------|--------|
| `list` / `view` / `search` / `install` / `update` / `remove` | Shipped |
| `browse` / `inspect` / `check` / `audit` / `tap` / `trust` / `snapshot` / `opt-out` / … | **Missing** (slash-only) |

Also: `edgecrab install <spec>` Pi-class alias → same installer.

## TUI strengths

| Piece | Module |
|-------|--------|
| Installed skill browser | `/skills` empty / list overlay |
| Remote skill browser | `/skills search\|hub` |
| Skill Guard overlay | `skill_trust_overlay.rs` — verdict, findings, file inspector, actions |
| Trust key mapping | `skill_trust_overlay` (crate root) + `remote_skill_guard.rs` |

Exceeds Hermes Ink hub on scan theatre. Gap: stages (fetch/quarantine/scan) not streamed as a single marketplace FSM; install progress storytelling weaker than Hermes Rich CLI.

## Security model (shipped)

1. Path validation (`safe_relative_join`)  
2. Quarantine before scan  
3. `threat_patterns` + `.skillignore` (SKILL.md never ignored)  
4. Verdict: empty → Safe; Critical or ≥3 High → Dangerous; else Caution  
5. Policy: trust × verdict × `{force, trusted_dangerous}`  
6. Hash-bound approvals in `guard_approvals.json`  
7. Audit JSON lines  
8. Optional `GITHUB_TOKEN` / `GH_TOKEN`  
9. URL SSRF checks on registry fetches  
10. AST audit diagnostic (not install gate)  
11. Optional write-approval for agent `skill_manage`  
12. Opt-in `skills.inline_shell`

## Trust duality (DRY debt)

| Location | Model |
|----------|-------|
| Hub / guard | `trust_level` strings: trusted / community / … |
| `skills.rs` agent tools | `TrustLevel` enum: Builtin / Official / Trusted / Community |
| `skills_guard` | Local `Severity` mapping from security crate |

Target: one `TrustLevel` + one Severity SoT — [010](./010-solid-dry-ownership.md).

## Doctor

`check_skills`: pass if any `SKILL.md` under skills dir; else warn.  
**Does not** check: `.hub` lock integrity, taps, `GITHUB_TOKEN` for hub fetch, quarantine orphans, guard approvals, index cache age.

## Index

`skills_hub/index.rs` uses:

```text
REMOTE_INDEX_URL = https://hermes-agent.nousresearch.com/docs/api/skills-index.json
```

EdgeCrab does not yet host its own index CI (Hermes lead).

## Notable APIs

```text
SkillBundle, SkillMeta, LockEntry, Tap, SearchReport
InstallGate, InstallOutcome, InstallScanPreview, SkillUpdateCheck
search_hub, install_identifier, install_skill, install_github_skill
preview_install_scan, inspect_identifier_scan
trust_identifier, update_installed_skill, check_for_skill_updates
uninstall_skill, add_tap / remove_tap / read_taps
export_hub_snapshot / import_hub_snapshot
handle_skills_hub_slash, hub_slash_mutates_skills
notify_hub_skills_mutated, append_audit_log
```

## What already exceeds Hermes

1. First-class **local path** install  
2. **`--trust`** + hash-bound dangerous approvals (Hermes hard-blocks)  
3. **Skill Guard TUI** with file inspector  
4. Shared slash handler across TUI + gateway  
5. Typed `InstallGate` / preview DTOs for UI binding  

## What lags Hermes

1. CLI clap breadth  
2. Own skills-index CI  
3. Rich CLI install storytelling  
4. `publish` to GitHub/ClawHub  
5. Bundled `list-modified` / `diff` CLI parity (partial via sync helpers)
