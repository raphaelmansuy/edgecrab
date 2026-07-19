# 015 — Registry Super-Set Implementation

**Cross-ref:** [014](./014-registry-source-matrix.md) · [010](./010-solid-dry-ownership.md) · [011](./011-implementation-plan.md)

## Wave WR (parallel with W1)

```text
WR0 docs → WR1 trait+parity → WR2 peer bridges → WR3 e2e → WR4 surfaces
```

## Module ownership

| Module | Role |
|--------|------|
| `skills_hub/source_trait.rs` | `SkillSource` trait + `SkillSourceRouter` |
| `skills_hub/normalize.rs` | `normalize_identifier` → canonical form |
| `skills_hub/default_taps.rs` | Hermes DEFAULT_TAPS + ensure on hub init |
| `skills_hub/import_from.rs` | Peer filesystem import via quarantine |
| `skills_hub/sources.rs` | Registry HTTP adapters |
| `skills_hub/mod.rs` | Façade: search/install/catalog |
| `skills_hub/npm_pack.rs` | `npm:` tarball → SKILL.md dirs |

## SkillSource trait (code is law)

Implemented in `skills_hub/source_trait.rs` + router adapters in `router.rs`:

```rust
#[async_trait]
pub trait SkillSource: Send + Sync {
    fn source_id(&self) -> &'static str;
    async fn search(&self, query: &str, limit: usize) -> Vec<SkillMeta>;
    async fn fetch(&self, identifier: &str) -> Result<SkillBundle, String>;
    fn trust_level_for(&self, identifier: &str) -> &'static str;
}
```

**Deferred:** `inspect` on the trait — inspect/scan stays on the façade (`preview_install_scan` / `inspect_identifier_scan`). Fetch-only sources (url/local/npm) may return empty search results.

## E2E matrix (mocked HTTP)

Test binary: `crates/edgecrab-tools/tests/skills_hub_sources_e2e.rs`

| Case | Assert |
|------|--------|
| Each source_id search+fetch | Bundle has SKILL.md |
| well-known: install | Commits under EDGECRAB_HOME |
| git: / @owner/slug / skills-sh: | Normalize + fetch |
| npm: fixture | Installs nested skill |
| Default taps | huggingface + NVIDIA present |
| Provider filter openai | Excludes clawhub |
| Dangerous policy | Needs --trust |
| Path traversal | Rejected |
| import-from fixture | Uses quarantine |

## Proof

[proof/wr-registry-e2e.md](./proof/wr-registry-e2e.md)
