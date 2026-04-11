# Releasing EdgeCrab

## Quick start — one command

```bash
./scripts/release-version.sh set <version>
```

Or via GitHub Actions (no local tools needed):
**Actions → Release — Coordinator → Run workflow → enter version**

Both methods do the exact same thing and are the recommended way to cut every release.

The canonical release version lives in [`Cargo.toml`](/Users/raphaelmansuy/Github/03-working/edgecrab/Cargo.toml) under `[workspace.package].version`.
Every published package version is derived from that source by `./scripts/release-version.sh`.

---

## What happens automatically

Pushing a `v*.*.*` tag triggers all downstream workflows in parallel:

| Workflow | Publishes to | Runner |
|---|---|---|
| `release-binaries.yml` | GitHub Release (5 native archives) | ubuntu / macos / windows |
| `release-docker.yml` | `ghcr.io/raphaelmansuy/edgecrab` | ubuntu-latest + ubuntu-24.04-arm (no QEMU) |
| `release-npm-cli.yml` | npm `edgecrab-cli` | ubuntu-latest |
| `release-pypi-cli.yml` | PyPI `edgecrab-cli` | ubuntu-latest |
| `release-rust.yml` | crates.io `edgecrab-cli` | ubuntu-latest |
| `release-node.yml` | npm `edgecrab` (Node SDK) | ubuntu-latest |
| `release-python.yml` | PyPI `edgecrab` (Python SDK) | ubuntu-latest |

Binary archives are built first; npm/pip wrappers download them lazily at
install time so there is no ordering dependency between workflows.

For manual reruns, pass the exact tag with `workflow_dispatch`.
The release workflows now check out that tag explicitly, so a rerun for `vX.Y.Z`
rebuilds the tagged source instead of the moving `main` branch.

---

## Version authority

All release automation now treats the workspace version in [`Cargo.toml`](/Users/raphaelmansuy/Github/03-working/edgecrab/Cargo.toml) as the single source of truth.
Derived package versions are synced by [`scripts/release-version.sh`](/Users/raphaelmansuy/Github/03-working/edgecrab/scripts/release-version.sh), and CI rejects drift.

| File | Field |
|---|---|
| `Cargo.toml` | canonical `[workspace.package] version` |
| `sdks/node/package.json` | derived `"version"` |
| `sdks/npm-cli/package.json` | derived `"version"` |
| `sdks/pypi-cli/edgecrab_cli/_version.py` | derived `__version__` |
| `sdks/pypi-cli/pyproject.toml` | dynamic version source (`edgecrab_cli._version.__version__`) |
| `sdks/python/pyproject.toml` | derived `version` |

### Commands

```bash
./scripts/release-version.sh print
./scripts/release-version.sh sync
./scripts/release-version.sh check
./scripts/release-version.sh set <version>
```

> The npm CLI wrapper derives its binary tag from `package.json`, and the PyPI
> CLI wrapper derives both package metadata and binary tag from
> `edgecrab_cli._version.__version__`. Those files are derived state, not
> independent release authorities.

---

## Step-by-step (manual fallback)

If you can't use the script or the coordinator workflow:

```bash
# 1. Ensure main is clean and up to date
git checkout main && git pull

# 2. Bump the canonical version and sync all derived package metadata
VERSION=<version>

./scripts/release-version.sh set "$VERSION"
./scripts/release-version.sh check

# 3. Commit, tag, push
git add Cargo.toml sdks/npm-cli/package.json \
        sdks/node/package.json sdks/node/package-lock.json \
        sdks/pypi-cli/edgecrab_cli/_version.py \
        sdks/python/edgecrab/_version.py
git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"
```

---

## After the release

The crates.io workflow publishes crates in dependency order and still keeps an
intentional propagation delay between dependent publishes. It probes the exact
`crates.io/api/v1/crates/<crate>/<version>` endpoint with hard timeouts, then
keeps a short stabilization buffer after visibility so we do not publish faster
than the registry has propagated. If crates.io stays slow, the workflow falls
back to bounded publish retries instead of hanging indefinitely.

### Update the Homebrew formula

Once binaries are live on the GitHub Release, the preferred path is the
automated `release-homebrew-tap.yml` workflow. It downloads
`edgecrab-checksums.txt`, updates `raphaelmansuy/homebrew-tap`, and pushes the
formula change using `HOMEBREW_TAP_PUSH_TOKEN`.

Manual fallback if needed:

```bash
gh release download "v${VERSION}" \
  --repo raphaelmansuy/edgecrab \
  --pattern edgecrab-checksums.txt
cat edgecrab-checksums.txt

# Download both macOS archives and compute SHA256
ARM_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-aarch64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')
X86_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-x86_64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')

echo "ARM SHA256:   $ARM_SHA"
echo "x86_64 SHA256: $X86_SHA"
```

Then update the formula with:

```bash
./scripts/update-homebrew-formula.sh \
  /path/to/homebrew-tap/Formula/edgecrab.rb \
  "$VERSION" \
  "$ARM_SHA" \
  "$X86_SHA"
```

Commit and push the tap repository after verifying the diff.

### Verify all install methods

```bash
# Docker (should pull the arm64 image on Apple Silicon)
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run --rm --entrypoint /bin/sh ghcr.io/raphaelmansuy/edgecrab:latest -lc 'which edgecrab && edgecrab --version'

# npm (fresh install, no cache)
npm install -g edgecrab-cli
which edgecrab
edgecrab --version

# pip
pip install --force-reinstall edgecrab-cli
which edgecrab
edgecrab --version

# cargo
cargo install edgecrab-cli --locked --force
which edgecrab
edgecrab --version

# Homebrew
brew upgrade edgecrab
which edgecrab
edgecrab --version
```

If Homebrew is still behind while npm, PyPI, crates.io, and Docker are current,
the tap sync is the missing step.

---

## Required secrets / environments

| Secret | Where | Used by |
|---|---|---|
| `NPM_TOKEN` | `npm` environment | `release-npm-cli.yml` |
| `CARGO_REGISTRY_TOKEN` | repository secrets | `release-rust.yml` |
| PyPI OIDC trusted publisher | `pypi` environment | `release-pypi-cli.yml` |
| `HOMEBREW_TAP_PUSH_TOKEN` | repository secrets | `release-homebrew-tap.yml` |
| `GITHUB_TOKEN` | auto-provisioned | all workflows |

---

## Versioning policy

EdgeCrab follows [Semantic Versioning](https://semver.org):

- **PATCH** (`0.1.x`) — bug fixes, dependency updates, documentation
- **MINOR** (`0.x.0`) — new features, backwards-compatible changes
- **MAJOR** (`x.0.0`) — breaking CLI / config / API changes
