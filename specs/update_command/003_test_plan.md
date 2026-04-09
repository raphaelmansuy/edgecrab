# Update Command Test Plan

## Unit Tests

- detect `npm` from wrapper env metadata
- detect `pypi` from wrapper env metadata
- detect `cargo` from canonical executable path
- detect `brew` from canonical executable path
- detect `source` from git checkout heuristics
- fallback detect `binary`
- parse stable semver tags with leading `v`
- reject malformed or prerelease tags when current build is stable
- compare versions correctly across patch and minor bumps
- render update notice for newer release
- render up-to-date report
- render source checkout guidance
- choose `pipx` command when pipx is detected
- choose `pip` command when pipx is not detected
- cache freshness logic honors configured interval
- stale cache triggers background refresh
- fetch failure returns cached result without panic

## Integration Tests

- `edgecrab update` prints a channel-aware report
- `edgecrab update --apply` executes the expected command builder for `cargo`
- `edgecrab update --apply` executes the expected command builder for `brew`
- startup path does not block when network fetch hangs beyond timeout
- TUI `/update` routes through the shared updater and emits the rendered result

## Manual Verification

- install via npm and run `edgecrab update`
- install via pip or pipx and run `edgecrab update`
- install via cargo and run `edgecrab update`
- install via brew and run `edgecrab update`
- run from a dirty git checkout and verify no destructive action is attempted
- disconnect network and ensure startup remains fast

## Regression Checks

- `edgecrab version` still prints the existing version information
- `status` still works
- `/update` in TUI no longer shells out to git for non-source installs
- quiet mode does not print unsolicited update banners

## Quality Gates

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
