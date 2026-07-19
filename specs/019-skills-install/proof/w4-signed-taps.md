# Proof W4 — Signed Publisher Taps

**Status:** implemented (2026-07-18)  
**Criteria:** [012](../012-acceptance-criteria.md) · [011](../011-implementation-plan.md) · supersedes gap [028](../../001-gap-analysis-v14/028-skills-hub-trusted-taps/)

## Claim

Signed tap manifests verify **sha256 + Ed25519** before skill commit when identifier is `signed:<manifest>`. TOFU pins publisher key on first verify/`tap add`. Tamper fails closed. Unsigned taps remain community trust (no silent elevation).

## Evidence checklist

- [x] Module: `crates/edgecrab-tools/src/tools/skills_hub/signed_taps.rs`  
- [x] Fixture: valid manifest + key → verify + TOFU pin succeeds  
- [x] Fixture: bad signature → verify fails; no elevation  
- [x] Fixture: content hash mismatch → `assert_content_hash` fail closed  
- [x] TOFU: first pin stored; rotation requires `--rotate` / `allow_rotation`  
- [x] Audit log records `signed-verify` with `key_id=` on install from `signed:`  
- [x] Slash/CLI: `/skills tap add signed:<manifest.json> [--rotate]`  
- [x] 028 overview points to 019 W4  

## Test fixtures

| Fixture / test | Expected |
|----------------|----------|
| `valid_manifest_verifies_and_hash_matches` | verify + hash + TOFU ok |
| `bad_signature_fails_closed` | Err |
| `content_hash_mismatch_fails_closed` | Err |
| `tofu_rotation_requires_confirm` | Err without rotate; Ok with |
| `add_signed_tap_registers_trusted` | tap `signed-<publisher>` trust=trusted |

```bash
cargo test -p edgecrab-tools --lib signed_taps -- --nocapture
```

## Sign-off

| Role | Date | OK |
|------|------|----|
| Security reviewer | 2026-07-18 | yes (fail-closed + TOFU) |
| AI Engineer | 2026-07-18 | yes |
| Product Owner | 2026-07-18 | yes |
