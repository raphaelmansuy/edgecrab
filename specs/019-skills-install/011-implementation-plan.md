# 011 — Implementation Plan (W0–W4)

**Cross-ref:** [005](./005-gap-matrix.md) · [010](./010-solid-dry-ownership.md) · [012](./012-acceptance-criteria.md) · [proof/](./proof/)

## Wave map

```text
W0 Spec pack (this directory)     ── done when docs land
WR Registry super-set             ── Hermes ∪ peer sources + e2e (∥ W1)
W1 Surface parity (CLI ≡ slash)   ── scripting / CI unlock
W2 TUI marketplace polish         ── daily-driver unlock
W3 Doctor + index truth           ── readiness unlock
W4 Signed taps (TOFU)             ── publisher-identity unlock
```

Dependence: WR ∥ W1; W2 after WR façade stable; W3 after W1 flags exist; W4 after trust UI/CLI verbs stable.

**WR detail:** [014](./014-registry-source-matrix.md) · [015](./015-registry-implementation.md) · [proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md)

---

## W0 — Spec + ownership

**Outcome:** High-signal docs; stale pointers called out.

| Task | Detail |
|------|--------|
| Author 000–013 + proof stubs | This pack |
| Mark 028 current-state stale | Note in [013](./013-cross-ref-index.md): taps exist; crypto does not; Hermes also lacks crypto in code |
| No Rust required | — |

**Exit:** Docs merged; team agrees façade ownership in 010.

---

## W1 — Surface parity

**Outcome:** `edgecrab skills <action>` covers slash hub actions; shared flags.

### Code anchors

| File | Change |
|------|--------|
| `crates/edgecrab-cli/src/cli_args.rs` | Expand `SkillsCommand` |
| `crates/edgecrab-cli/src/main.rs` | Dispatch → façade / `handle_skills_hub_slash` or shared do_* |
| `crates/edgecrab-tools/src/tools/skills_hub/hub_slash.rs` | Extract pure action runners if needed for clap reuse |
| `crates/edgecrab-tools/src/tools/skills_hub/mod.rs` | Stage formatter (human + JSON) optional helper |

### Clap actions to add (minimum)

`inspect`, `browse` (or search flags), `check`, `audit`, `trust`, `untrust`, `trusted`, `tap`{list,add,remove}, `snapshot`{export,import}, `opt-out`, `opt-in`, `reset`, `lock` (read), `sources`/`catalog` as needed for parity.

### Flags

`--force`, `--trust`, `-y/--yes`, `--json`, `--deep` (audit), `--scan` (inspect).

### Tests

- Parse tests in `cli_args.rs` for each subcommand  
- Table test: clap action set ⊇ slash mutating/non-mutating hub actions  
- Install local fixture via CLI in TempDir  

**Proof:** [proof/w1-cli-slash-parity.md](./proof/w1-cli-slash-parity.md)

---

## W2 — TUI marketplace polish

**Outcome:** One overlay journey with install stages + existing Guard as gate.

### Code anchors

| File | Change |
|------|--------|
| `edgecrab-cli/src/app/skills_marketplace.rs` | **New** FSM + render |
| `edgecrab-cli/src/app/skill_trust_overlay.rs` | Reuse as GuardReview stage |
| `edgecrab-cli/src/app/remote_skill_guard.rs` | Feed marketplace; dedupe |
| `edgecrab-cli/src/app/browser_selectors.rs` | Thin wrappers / remove dup |
| `edgecrab-cli/src/app/key_dispatch.rs` / `frame_render.rs` | Wire overlay |
| `skills_hub/install_preview.rs` | Ensure DTO complete for stages |

### Behavior

1. Search/browse remote + installed in one chrome.  
2. Install runs staged async: Fetch → Quarantine → Scan → Gate → Commit.  
3. Caution/Dangerous → Skill Guard overlay (existing).  
4. Keybindings per [008](./008-tui-expert-lens.md); copy per [009](./009-ux-ui-designer-lens.md).

### Tests

- Pure keymap transition tests  
- Render smoke with fixture preview  
- No network in unit tests  

**Proof:** [proof/w2-tui-marketplace.md](./proof/w2-tui-marketplace.md)

---

## W3 — Doctor + index truth

**Outcome:** `/doctor` reports hub health; index freshness visible; token guidance.

### Code anchors

| File | Change |
|------|--------|
| `edgecrab-cli/src/doctor.rs` | Replace/extend `check_skills` |
| `skills_hub/index.rs` | Expose cache age / last fetch status helpers |
| Optional CI | EdgeCrab skills-index workflow (or documented mirror) |

### Doctor checks (minimum)

| Check | Pass / warn / fail |
|-------|--------------------|
| Skills dir exists | warn if missing |
| Installed count | info |
| Lockfile parse | fail if corrupt |
| Quarantine orphans | warn if stale dirs |
| Taps count / parse | warn on error |
| `GITHUB_TOKEN`/`GH_TOKEN` | warn if unset when hub used |
| Index cache age | warn if > TTL×2 |

**Proof:** [proof/w3-doctor-hub.md](./proof/w3-doctor-hub.md)

---

## W4 — Signed publisher taps

**Outcome:** Real Ed25519 (or equivalent) signed tap manifests + TOFU pin; install verifies sha256 + signature. Fail closed.

### Code anchors

| File | Change |
|------|--------|
| `skills_hub/signed_taps.rs` | **New** — manifest parse, verify, pin store |
| `skills_hub` tap add/install paths | Verify before commit |
| `guard_approvals` / lock | Record publisher key id + sig meta |
| CLI/TUI | Show publisher pin on first add; rotation confirm |
| Supersede | [`028` implementation plan](../001-gap-analysis-v14/028-skills-hub-trusted-taps/004-implementation-plan.md) points here |

### Manifest sketch

```json
{
  "tap": "example.com/skills",
  "publisher_key_id": "ed25519:…",
  "skills": [
    { "name": "foo", "sha256": "…", "path": "foo/SKILL.md", "sig": "…" }
  ]
}
```

Unsigned taps remain allowed as **community** (current behavior). Signed taps elevate trust only after verify.

**Proof:** [proof/w4-signed-taps.md](./proof/w4-signed-taps.md)

---

## Explicit deferrals

| Item | When |
|------|------|
| `publish` to GitHub/ClawHub | After W2; optional |
| Blueprints / cron suggestions | After W2 |
| Own full index CI (if costly) | W3 minimum = freshness + Hermes fallback; own CI nice-to-have |
| Desktop/web hub | Never required for 019 |

## Sequencing estimate (eng days, indicative)

| Wave | Effort |
|------|--------|
| W0 | 1 (docs) |
| W1 | 2–4 |
| W2 | 3–5 |
| W3 | 1–2 |
| W4 | 4–7 |

## Definition of done (program)

EdgeCrab exceeds Hermes on skills install when:

1. CLI ≡ slash (W1)  
2. TUI marketplace + theatre (W2)  
3. Doctor hub truth (W3)  
4. Signed taps fail-closed (W4)  
5. Ban list in 010 holds under review
