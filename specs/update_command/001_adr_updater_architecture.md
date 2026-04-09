# ADR 001: Channel-Aware Updater Architecture

## Status

Accepted

## Context

EdgeCrab is distributed through:

- npm wrapper package: `edgecrab-cli`
- PyPI wrapper package: `edgecrab-cli`
- crates.io binary crate: `edgecrab-cli`
- Homebrew formula: `edgecrab`
- source checkout and manual binary installs

Each channel has different ownership semantics. A self-replacing updater would
fight the package manager and create drift.

## Decision

Implement a channel-aware updater with three layers:

1. Install provenance detection
2. Release check and local cache
3. Update execution through the owning channel

### Install provenance

Detection order:

1. explicit runtime metadata from wrappers via environment variables
2. executable path heuristics
3. repository heuristics for source checkouts
4. fallback to `binary`

Canonical install methods:

- `npm`
- `pypi`
- `cargo`
- `brew`
- `source`
- `binary`

Wrapper-provided metadata is authoritative because it is explicit and cheap.

### Version authority

Release version authority:

- canonical source: `Cargo.toml` `[workspace.package].version`
- derived release metadata: Node SDK `package.json`, npm CLI `package.json`, PyPI CLI `_version.py`, Python SDK `pyproject.toml`
- sync mechanism: `scripts/release-version.sh`
- guardrail: CI runs `scripts/release-version.sh check`

Reasoning:

- package managers still require channel-local version metadata
- those files are derived state, not peers
- one canonical source prevents cross-channel drift during release automation

### Release discovery

Use the latest GitHub release API as the release signal:

- endpoint: `https://api.github.com/repos/raphaelmansuy/edgecrab/releases/latest`
- timeout: short and bounded
- result cached on disk with timestamp

Reasoning:

- npm and PyPI CLI wrappers ultimately map to GitHub release binaries
- Homebrew should track GitHub release tags
- workspace version already matches release tags
- startup messaging is about a new release existing, not guaranteed same-minute registry convergence

### Update execution

Update commands by channel:

- `npm` -> `npm install -g edgecrab-cli@<version>`
- `pypi` -> prefer `pipx upgrade edgecrab-cli` when running under pipx, else `python -m pip install --upgrade edgecrab-cli`
- `cargo` -> `cargo install edgecrab-cli --locked --force --version <version>`
- `brew` -> `brew update && brew upgrade edgecrab`
- `source` -> report actionable source-update steps, do not mutate checkout automatically
- `binary` -> report GitHub Releases download URL, do not overwrite the running binary automatically

The CLI subcommand will support:

- check-only mode by default
- apply mode for package-managed channels
- report-only behavior for `source` and `binary`

### Startup notices

Behavior:

- startup never waits for the network result
- if cached data already says a newer release exists and cache is fresh enough, show notice immediately
- otherwise schedule a background check and surface the result only when it arrives
- throttle repeated checks with a configurable interval

### Persistence

Persist state under `~/.edgecrab/update-check.json`.

State includes:

- last check timestamp
- installed version last seen
- latest version
- release URL
- release published timestamp
- install method snapshot
- last error summary, optional

## Consequences

### Positive

- correct package-manager ownership
- no risky in-place self-overwrites
- fast startup
- shared logic across CLI and TUI
- deterministic rendering and testing

### Negative

- `brew` availability may lag until formula/tap propagation
- `source` and `binary` channels cannot be fully automated safely
- wrappers must inject metadata to avoid brittle inference

## Rejected Alternatives

### Self-replacing binary updater

Rejected because:

- package managers would lose ownership
- Windows and Unix replacement semantics differ
- it is easy to corrupt installs during partial failure

### Checking channel registries instead of GitHub release

Rejected as the primary startup signal because:

- it multiplies network logic and failure modes
- npm/PyPI wrappers are binary wrappers around GitHub release assets anyway
- it creates inconsistent user messaging between channels

Registry-specific availability can still be mentioned in `edgecrab update` reports if needed.
