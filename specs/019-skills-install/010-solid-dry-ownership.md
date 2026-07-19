# 010 — SOLID / DRY Ownership (Code Is Law)

**Cross-ref:** [002](./002-first-principles.md) · [007](./007-ai-engineer-lens.md) · [011](./011-implementation-plan.md) · [`../018-agent-harness/003-dry-solid-ownership.md`](../018-agent-harness/003-dry-solid-ownership.md)

## Ownership matrix

| Concern | Owner module | Consumers (adapters only) |
|---------|--------------|---------------------------|
| Fetch / registries / index | `skills_hub/sources.rs`, `index.rs` | façade |
| Bundle validate + quarantine + commit | `skills_hub/mod.rs` | façade |
| Scan + verdict + policy | `skills_guard.rs` ← `threat_patterns` | façade, preview |
| Dangerous hash approvals | `skills_hub/guard_approvals.rs` | façade |
| Lock / audit / taps / snapshot | `skills_hub/*` | façade |
| Slash text UX | `skills_hub/hub_slash.rs` | CLI TUI, gateway |
| Bundled seed (not hub) | `skills_sync.rs` | runtime startup |
| Discovery / slash skill cmds | `edgecrab-tools/src/skills/` | prompt, CLI |
| Agent tools | `tools/skills.rs` | ReAct loop |
| TUI render | `edgecrab-cli` overlays | — |
| Doctor checks | `edgecrab-cli/src/doctor.rs` | reads hub state; no policy |

## Façade (single entry)

Promote a clear public surface in `skills_hub` (functions already largely exist — name/group for adapters):

```text
edgecrab_tools::tools::skills_hub::
  search_hub
  install_identifier
  preview_install_scan / inspect_identifier_scan
  trust_identifier / …
  check_for_skill_updates / update_*
  uninstall_skill
  tap_* / snapshot_*
  handle_skills_hub_slash   # text adapter
```

CLI clap and agent tool **must not** open HTTP or write quarantine except via these.

## DRY debt to burn

| Debt | Today | Fix |
|------|-------|-----|
| Dual trust models | Hub used parallel enum | Hub uses `skills::TrustLevel` (close-out C6) |
| Severity mapping | Local enum in `skills_guard` | `Severity` = `ThreatSeverity` SoT (close-out C6) |
| CLI vs slash actions | Parallel incomplete clap | W1: clap → same handlers as slash |
| Install storytelling | Ad-hoc prints | `install_stages` human + JSON (close-out C4) |
| Spec path drift | Docs cite `skills_hub.rs` | Cite `skills_hub/` module |
| Curated catalogs | Triple lists | `HUB_CATALOG` SoT (close-out C1) |
| Search fan-out | Façade bypassed router | `SkillSourceRouter::search_groups` (close-out C2) |

## SOLID checklist

| Principle | Rule |
|-----------|------|
| **S**ingle responsibility | Guard does not fetch; TUI does not scan; doctor does not install |
| **O**pen/closed | New registry = new source impl behind router |
| **L**iskov | Every source yields `SkillBundle` with validated relative paths |
| **I**nterface segregation | Agent tool exposes subset actions; not raw quarantine APIs |
| **D**ependency inversion | Overlays depend on `InstallScanPreview`, not GitHub client |

## Ban list

1. Second install path that skips quarantine.  
2. TUI/CLI calling GitHub Contents API directly.  
3. Gateway-only policy (different allow rules than CLI).  
4. `--force` overriding `Verdict::Dangerous`.  
5. Mutating `cached_system_prompt` on skill install.  
6. New Severity enum in CLI.  
7. Duplicating threat regexes outside `threat_patterns`.  
8. Signed-tap verify as “warn only” (must fail closed).  
9. Reintroducing Electron/desktop as required path for hub.  
10. Growing `app.rs` with marketplace logic — extract module.

## Crate direction

```text
edgecrab-security   threat_patterns, Severity
        ↑
edgecrab-tools      skills_guard, skills_hub, skills_sync, skills/
        ↑
edgecrab-core       config SkillsConfig, prompt skills summary hooks
        ↑
edgecrab-cli        clap, doctor, TUI overlays
edgecrab-gateway    slash → hub_slash
```

No new crate required for W1–W3. W4 may add `skills_hub/signed_taps.rs` inside tools.

## Review gate (PR checklist)

- [ ] Diff touches only owner modules for the concern  
- [ ] Policy tests updated if gate matrix changes  
- [ ] Slash action table and clap enum stay in lockstep (W1+)  
- [ ] TUI uses DTOs only  
- [ ] `EDGECRAB_HOME` TempDir in tests — never `~/.edgecrab`
