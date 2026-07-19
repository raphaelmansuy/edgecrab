# 002 — First Principles (Skills Install)

**Cross-ref:** [000](./000-overview.md) · [001](./001-five-whys.md) · [010](./010-solid-dry-ownership.md)

## Operator questions (skills domain)

Every skills install surface must answer five orthogonal questions. Collapsing them produces “installed but untrusted / trusted but opaque / works in TUI only” failure modes.

```text
  Q1  What can I install?           → search / browse / catalog / taps
  Q2  What am I about to install?   → inspect / preview SKILL.md + files
  Q3  Is it safe enough for me?     → scan verdict + findings + publisher
  Q4  Did install actually commit?  → lock entry + audit + skills reload
  Q5  Can I reverse or refresh it?  → uninstall / update / check / untrust
```

| Question | Hermes (code) | EdgeCrab (code) | Target |
|----------|---------------|-----------------|--------|
| **Q1** | Multi-source + index CI | Multi-source + Hermes index consumer | EdgeCrab index + fallback; CLI browse |
| **Q2** | `inspect` + Rich panels | `inspect [--scan]` + TUI preview | Same DTO on CLI/TUI |
| **Q3** | Trust × verdict; force | + hash-bound `--trust` + overlay | + signed taps |
| **Q4** | lock + audit + cache clear | lock + audit + `notify_hub_skills_mutated` | Same + doctor verify |
| **Q5** | check/update/uninstall | Same + trust/untrust | Surface parity |

## Install invariants (code is law)

```text
  REQUIRED:
  ┌──────────────────────────────────────────────────────────────────┐
  │  No skill lands under ~/.edgecrab/skills/<name>/ without:        │
  │    1. fetch into SkillBundle                                     │
  │    2. stage under .hub/quarantine/                               │
  │    3. skills_guard::scan_skill                                   │
  │    4. should_allow_install_with(InstallPolicyContext)            │
  │    5. atomic commit + lock + audit                               │
  │    6. discovery / prompt-skills cache invalidate                 │
  └──────────────────────────────────────────────────────────────────┘

  FORBIDDEN:
  • CLI/TUI path that writes SKILL.md without quarantine
  • --force overriding Verdict::Dangerous for community/trusted
  • Mutating SessionState.cached_system_prompt on hub install
  • Second TrustLevel enum with divergent semantics
```

## Laws L1–L7

### L1 — One Install Pipeline

```text
fetch → quarantine → scan → gate → commit → invalidate
```

Owner: `edgecrab-tools::tools::skills_hub` (`install_identifier` / `install_skill`).  
Surfaces adapt; they do not reimplement.

### L2 — One Trust Model

```text
allow = f(trust_level, verdict, force, trusted_dangerous, signature_ok)
```

| Input | Meaning |
|-------|---------|
| `trust_level` | builtin / trusted / community / agent-created |
| `verdict` | Safe / Caution / Dangerous |
| `force` | Caution override only |
| `trusted_dangerous` | `--trust` or hash-bound approval |
| `signature_ok` | W4: signed tap manifest verifies |

### L3 — Surface Parity

| Surface | Adapter | Must call façade |
|---------|---------|------------------|
| CLI | `edgecrab skills …` | yes |
| Slash | `/skills …` via `handle_skills_hub_slash` | yes |
| TUI | marketplace + trust overlays | yes (DTOs only) |
| Gateway | same slash handler | yes |
| Agent | `skills_hub` tool | yes |

### L4 — Evidence Before Trust

Dangerous or Caution install UX **must** show:

1. Verdict badge  
2. Top findings (pattern id + severity + file:line when known)  
3. File list with SKILL.md preview  
4. Explicit action: Cancel / Force (caution) / Trust (dangerous) / Install (safe)

### L5 — Cache Law

On hub mutate (`hub_slash_mutates_skills` / install/update/remove/import):

- Invalidate skill discovery index
- Invalidate prompt skills summary cache
- Do **not** rebuild or mutate `cached_system_prompt` / `cached_stable_prompt`

Goals/steers remain message-injected; skills summary rebuild happens at next prompt assembly boundary (session start / explicit reload), never mid-turn system-prefix mutation.

### L6 — Publisher > Pattern

Regex/`threat_patterns` catch obvious malice. They do **not** prove publisher identity. Typosquats that pass the scanner require:

- Hardcoded trusted repos (today) +  
- Hash-bound user trust (today) +  
- **Signed tap manifests + TOFU pin (W4)**

### L7 — DRY / SOLID

| Concern | Single owner |
|---------|--------------|
| Fetch / sources | `skills_hub/sources.rs` + `index.rs` |
| Quarantine / install | `skills_hub/mod.rs` |
| Scan | `skills_guard.rs` ← `threat_patterns` |
| Approvals | `skills_hub/guard_approvals.rs` |
| Slash text UX | `skills_hub/hub_slash.rs` |
| TUI render | `edgecrab-cli` overlays only |
| Bundled seed | `skills_sync.rs` (separate from hub install) |

Ban list: [010-solid-dry-ownership](./010-solid-dry-ownership.md).

## Trust policy matrix (target = current + signatures)

| Trust level | Safe | Caution | Dangerous |
|-------------|------|---------|-----------|
| builtin | allow | allow | allow |
| trusted | allow | allow | need `--trust` / approval (+ sig if signed tap) |
| community | allow | need `--force` | need `--trust` / approval (+ sig if signed tap) |
| agent-created | allow | allow / ask (config) | ask |

`--force` never overrides Dangerous. Signature failure fails closed (W4).

## Relation to harness laws (018)

| 018 law | Skills install mapping |
|---------|------------------------|
| One Done | One `InstallOutcome` / gate decision path |
| Cache law | L5 above |
| Evidence > vibes | L4 scan preview |
| Armed defaults | Dangerous blocked; Caution blocked without force |
| DRY / SOLID | L7 |
