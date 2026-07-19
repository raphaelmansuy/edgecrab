# Proof W3 — Doctor Hub Health

**Status:** implemented (2026-07-18)  
**Criteria:** [012](../012-acceptance-criteria.md) · [011](../011-implementation-plan.md)

## Claim

`edgecrab doctor` reports actionable Skills Hub health via façade [`assess_hub_health`](../../../crates/edgecrab-tools/src/tools/skills_hub/hub_health.rs): lock integrity (fail if corrupt), quarantine orphans, `GITHUB_TOKEN`/`GH_TOKEN` guidance, and index age > 2×TTL warn — not merely “N SKILL.md files found.”

## Evidence checklist

- [x] Doctor uses `assess_hub_health` (DRY — not reimplemented summary logic)  
- [x] Corrupt lock → fail (`HubHealthSeverity::Fail`)  
- [x] Missing `GITHUB_TOKEN`/`GH_TOKEN` → warn with exact names in detail  
- [x] Stale quarantine dirs → warn (orphan count)  
- [x] Index age warn when beyond TTL×2 (`INDEX_TTL_SECS * 2`)  
- [x] Unit tests: `validate_lock_rejects_garbage`, `assess_detects_corrupt_lock_and_orphans` (TempDir + `EDGECRAB_HOME`)  

## Transcripts

```text
$ EDGECRAB_HOME=<tmpdir> edgecrab doctor
… Skills  ✗  … lock CORRUPT: … ; warn: set GITHUB_TOKEN or GH_TOKEN …
# healthy: Skills ✓ … lock ok ; index age …s ; GITHUB_TOKEN/GH_TOKEN set
```

## Sign-off

| Role | Date | OK |
|------|------|----|
| Product Owner | 2026-07-18 | yes |
| AI Engineer | 2026-07-18 | yes |
