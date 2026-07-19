# 006 — Product Owner Lens

**Cross-ref:** [001](./001-five-whys.md) · [005](./005-gap-matrix.md) · [012](./012-acceptance-criteria.md)

## Jobs to be done

| Job | User | Success looks like |
|-----|------|--------------------|
| **Find a skill** | Operator / power user | Search returns ranked results with source badge in <2s (cached) or clear “rate limited — set GITHUB_TOKEN” |
| **Install safely** | Operator | Sees verdict → chooses Install / Force / Trust / Cancel without leaving the flow |
| **Install from anywhere** | CLI/CI user | `edgecrab skills install …` same flags as `/skills install` |
| **Trust once** | Security-conscious user | Dangerous approve is hash-bound; content change re-prompts |
| **Stay healthy** | Admin | `/doctor` says hub is OK or tells exact fix |
| **Agent self-serve** | Agent + human oversight | Agent can search/install; dangerous still needs human trust path |

## Value propositions (exceed Hermes)

1. **One product, every door** — CLI, TUI, gateway, agent: same capability (Hermes fragments TUI).  
2. **Trust you can see** — Skill Guard theatre as the default install path, not an afterthought.  
3. **Trust you can prove** — Signed taps (W4): publisher identity, not only regex vibes.  
4. **Local-first** — First-class path install (already ahead of Hermes).  
5. **Ready machine** — Doctor makes hub failures actionable before the user is stuck mid-search.

## Personas

| Persona | Pain today | EdgeCrab win |
|---------|------------|--------------|
| **TUI daily driver** | Knows `/skills`; strong overlay | Marketplace FSM completes the story |
| **Headless / CI** | CLI missing trust/tap/audit | W1 clap parity |
| **Gateway-only** | Text slash OK | Keep shared handler; no fork |
| **Skill author** | Local install OK; publish missing | Local + inspect; publish later |
| **Security reviewer** | Hash trust OK; no publisher sig | W4 TOFU |

## Prioritization (RICE-style, relative)

| Initiative | Reach | Impact | Confidence | Effort | Wave |
|------------|-------|--------|------------|--------|------|
| CLI ≡ slash | High | High | High | M | W1 |
| Marketplace TUI | High | High | High | M | W2 |
| Doctor hub | Med | Med | High | S | W3 |
| Signed taps | Med | High | Med | L | W4 |
| Own index CI | Med | Med | Med | M | W3 |
| Publish | Low | Med | Med | L | later |

## Kill criteria

Stop or re-scope a wave if:

- A second install path appears that skips quarantine (architectural failure).  
- `--force` is taught as “always works” for Dangerous (policy failure).  
- TUI reimplements fetch/scan instead of consuming façade DTOs (DRY failure).  
- Signed taps ship without fail-closed verification (security theatre).  
- W1 expands into desktop/web scope (non-goal creep).

## Positioning vs Hermes (message)

> Hermes has a broad skills CLI and a great registry story. EdgeCrab matches the pipeline, leads on **dangerous-trust with evidence**, and will **unify every surface** while shipping **real publisher signatures** Hermes does not have in code today.

## Out of scope for PO (this pack)

- Monetized marketplace  
- Desktop Electron hub  
- Replacing agent progressive disclosure model  
- Guaranteeing zero false positives from regex scan  

## Success narrative (demo script)

1. `edgecrab doctor` → hub healthy / guided token fix.  
2. `edgecrab skills search pdf` → table with sources.  
3. `edgecrab skills install <id>` → staged output; Caution → `--force` or preview.  
4. TUI `/skills hub` → select → Skill Guard → Trust → installed → `/skill-name` works.  
5. (W4) `edgecrab skills tap add signed:…` → pin key → install verifies sig.
