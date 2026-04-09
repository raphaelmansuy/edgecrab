# ADR 002: Edge Cases, Roadblocks, and Resolutions

## Status

Accepted

## Edge Cases

### New GitHub release exists, but channel package is not yet visible

Risk:

- startup says a new release exists
- channel upgrade command may still fail briefly

Resolution:

- startup notice says "new release available", not "upgrade guaranteed now"
- `edgecrab update` surfaces channel-specific commands and any execution failure verbatim

### Running inside a source checkout

Risk:

- automatic `git pull` or rebuild could destroy local work or conflict with dirty trees

Resolution:

- never auto-update source checkouts
- report branch, dirty status, and safe next steps

### Manual binary install

Risk:

- EdgeCrab cannot know target location ownership or replacement permissions

Resolution:

- never self-overwrite
- provide the exact release URL and replacement guidance

### `pipx` versus plain `pip`

Risk:

- `pipx` installs should be upgraded with `pipx upgrade`, not `pip install`

Resolution:

- wrappers set provenance env vars
- updater also checks `PIPX_HOME` or executable path hints where possible
- choose `pipx` upgrade when confidently detected

### Homebrew installed via symlink

Risk:

- executable path may be `/opt/homebrew/bin/edgecrab` symlinked into Cellar

Resolution:

- canonicalize the executable path before classification
- treat any path containing `Cellar/edgecrab` or Homebrew prefix symlink target as `brew`

### CI, headless, or scripted invocations

Risk:

- noisy update notices pollute machine-readable output

Resolution:

- suppress startup notices when stdout is not a TTY or when `--quiet` command output must remain clean
- allow explicit `edgecrab update` to print full detail

### Rate limits and no network

Risk:

- startup delays or error spam

Resolution:

- strict timeout
- cache-first reads
- error-tolerant background task
- no startup warning on transient fetch failure

### Prerelease tags

Risk:

- stable users should not be nagged about prereleases

Resolution:

- ignore prereleases in startup checks
- compare only stable semver tags unless the current version itself is prerelease

## Roadblocks

### Wrapper metadata is currently missing

Resolution:

- update npm and PyPI launchers to export install provenance metadata before delegating to the native binary

### Homebrew release flow is partially external to this repo

Resolution:

- document the contract clearly
- emit release assets and version metadata that make tap updates trivial
- keep the updater tolerant of brew formula lag

### Existing `/update` behavior is git-only

Resolution:

- replace with shared updater rendering
- preserve source-checkout details only as the `source` channel branch of the new report

### Version drift between Rust workspace and wrappers

Risk:

- npm or PyPI wrapper version can lag the workspace release version
- published artifacts can disagree about which release they represent

Resolution:

- make `Cargo.toml` `[workspace.package].version` the only release authority
- generate wrapper version metadata from that source via `scripts/release-version.sh`
- enforce with CI via `scripts/release-version.sh check`
