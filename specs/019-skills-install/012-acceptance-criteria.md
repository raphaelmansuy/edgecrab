# 012 — Acceptance Criteria

**Cross-ref:** [011](./011-implementation-plan.md) · [proof/](./proof/) · [006](./006-product-owner-lens.md)  
**Honesty note (2026-07-18 close-out):** W1–W4 + WR criteria match code+proofs. Program gates below are **Met**. DRY/SOLID close-out (catalog SoT, router search/fetch, marketplace apply, trust/severity) is structural polish on top of shipped waves.

## Program gates

| ID | Gate | Wave | Measurable | Status |
|----|------|------|------------|--------|
| G0 | Spec pack complete | W0 | Docs 000–015 + proof files exist | **Met** |
| G-WR | Registry super-set | WR | Hermes ∪ peer sources + e2e ([014](./014-registry-source-matrix.md)/[015](./015-registry-implementation.md)) | **Met** (router owns search+fetch; `HUB_CATALOG` SoT) |
| G1 | CLI ≡ slash | W1 | See W1 criteria | **Met** |
| G2 | Marketplace TUI | W2 | See W2 criteria | **Met** (MVP + 008 polish) |
| G3 | Doctor hub | W3 | See W3 criteria | **Met** |
| G4 | Signed taps | W4 | See W4 criteria | **Met** |
| G5 | Invariants held | all | Policy + ban-list tests green | Ongoing |

---

## G-WR — Registry super-set

- [x] Identifier normalize (`git:`, `@owner/slug`, `skills-sh:`, `npm:`, `well-known:`) + unit tests.  
- [x] Hermes DEFAULT_TAPS seed + trusted repos (incl. huggingface/NVIDIA).  
- [x] Peer `import-from` through quarantine→scan→gate.  
- [x] Offline e2e suite [`skills_hub_sources_e2e`](../../crates/edgecrab-tools/tests/skills_hub_sources_e2e.rs) + [proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md).  
- [x] `SkillSourceRouter` dispatches search/fetch (adapters wrap existing paths).  
- [x] Expanded e2e: router registration, path-traversal reject, npm extract→install *(live per-source HTTP still optional / `#[ignore]`)*.  

Proof: [proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md).

---

## W1 — CLI / slash parity

- [x] Every hub action available in `handle_skills_hub_slash` has a clap path under `edgecrab skills` (or documented alias).  
- [x] `install` supports `--force`, `--trust`, `-y`, `--json`.  
- [x] `inspect` supports `--scan` and prints verdict + findings.  
- [x] `tap` / `trust` / `snapshot` / `audit` / `check` work without entering TUI.  
- [x] Local path install still goes through quarantine (regression test).  
- [x] Dangerous still blocked without `--trust` / approval.  
- [x] Proof doc [proof/w1-cli-slash-parity.md](./proof/w1-cli-slash-parity.md) filled.

## W2 — TUI marketplace

### MVP (shipped)

- [x] Marketplace FSM + install stages module (`skills_marketplace.rs`).  
- [x] Stages visible: Fetch → Quarantine → Scan → Gate → Commit.  
- [x] Caution/Dangerous opens existing Skill Guard (findings + files).  
- [x] Default focused action is never Force (Cancel default).  
- [x] Esc returns to prior state with query preserved.  
- [x] Pure unit tests for keymap; no network.  
- [x] Proof [proof/w2-tui-marketplace.md](./proof/w2-tui-marketplace.md) MVP section completed.

### 008 polish

- [x] Single marketplace overlay owns **BrowseInstalled** + SearchRemote + install stages.  
- [x] Keybindings match [008](./008-tui-expert-lens.md) footer (`/`/`s` search, provider filter, import-from, Guard `f`/`t`).  
- [x] Provider filter + import-from first-class in TUI.  
- [x] Theatre / Done / Error chrome DRY with marketplace accent colors.  

## W3 — Doctor + index

- [x] `edgecrab doctor` reports lock parse, taps, quarantine orphans, token guidance, index age.  
- [x] Missing token → warn with exact env vars (`GITHUB_TOKEN`, `GH_TOKEN`).  
- [x] Corrupt lock → fail (not silent pass).  
- [x] Index helper exposes freshness for doctor/UI (`index_age_secs` / `INDEX_TTL_SECS`).  
- [x] Proof [proof/w3-doctor-hub.md](./proof/w3-doctor-hub.md) completed.

## W4 — Signed taps

- [x] Tap add of signed manifest pins publisher key (TOFU).  
- [x] Install from signed tap verifies sha256 + signature before commit (`signed:` identifier).  
- [x] Tampered blob → install fails closed; quarantine cleaned.  
- [x] Key rotation requires explicit user confirm (`--rotate`).  
- [x] Unsigned taps remain community (no silent elevating).  
- [x] Proof [proof/w4-signed-taps.md](./proof/w4-signed-taps.md) completed.

---

## Cross-cutting invariants (always)

| Invariant | Test idea |
|-----------|-----------|
| No install without quarantine | Attempt CLI write bypass → impossible via public API |
| Force ≠ Dangerous | Policy matrix unit test |
| Cache law | Install does not change `cached_system_prompt` bytes |
| Path traversal rejected | Bundle with `../` fails |
| `EDGECRAB_HOME` isolation | All FS tests use TempDir |

## Non-acceptance (explicit)

- Desktop-only hub  
- “Signed” taps that warn but still install on bad sig  
- CLI that shells out to `cp` into skills dir  
- Divergent gateway allow rules  

## Sign-off

| Role | Signs | When |
|------|-------|------|
| Product Owner | G1–G3 value | After W2 polish demo |
| AI Engineer | façade + agent tool | Each wave |
| TUI expert | G2 keymap/render | W2 polish |
| Security-minded reviewer | G4 + invariants | W4 |
