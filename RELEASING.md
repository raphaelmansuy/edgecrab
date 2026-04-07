# Releasing EdgeCrab

## Quick start — one command

```bash
./scripts/bump-version.sh 0.2.0
```

Or via GitHub Actions (no local tools needed):
**Actions → Release — Coordinator → Run workflow → enter version**

Both methods do the exact same thing and are the recommended way to cut every release.

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

---

## Files touched by every release

All version strings are kept in sync by `scripts/bump-version.sh` and the
`release.yml` coordinator. Never edit them manually in isolation.

| File | Field |
|---|---|
| `Cargo.toml` | `[workspace.package] version` |
| `sdks/npm-cli/package.json` | `"version"` |
| `sdks/npm-cli/scripts/install.js` | `BINARY_VERSION` |
| `sdks/pypi-cli/pyproject.toml` | `version` |
| `sdks/pypi-cli/edgecrab_cli/_version.py` | `__version__` |
| `sdks/pypi-cli/edgecrab_cli/_binary.py` | `BINARY_VERSION` |
| `sdks/python/pyproject.toml` | `version` |

> **`BINARY_VERSION`** (in npm and pip wrappers) controls which GitHub Release
> tag the wrapper downloads the native binary from.  It must always match the
> release tag so `npm install edgecrab-cli` gets the right binary.

---

## Step-by-step (manual fallback)

If you can't use the script or the coordinator workflow:

```bash
# 1. Ensure main is clean and up to date
git checkout main && git pull

# 2. Bump versions (replace 0.2.0 with the actual new version)
VERSION=0.2.0

sed -i "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" sdks/npm-cli/package.json
sed -i "s/const BINARY_VERSION = '[^']*'/const BINARY_VERSION = '$VERSION'/" sdks/npm-cli/scripts/install.js
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" sdks/pypi-cli/pyproject.toml
printf '__version__ = "%s"\n' "$VERSION" > sdks/pypi-cli/edgecrab_cli/_version.py
sed -i "s/^BINARY_VERSION = \".*\"/BINARY_VERSION = \"$VERSION\"/" sdks/pypi-cli/edgecrab_cli/_binary.py
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" sdks/python/pyproject.toml

# 3. Commit, tag, push
git add Cargo.toml sdks/npm-cli/package.json sdks/npm-cli/scripts/install.js \
        sdks/pypi-cli/pyproject.toml sdks/pypi-cli/edgecrab_cli/_version.py \
        sdks/pypi-cli/edgecrab_cli/_binary.py sdks/python/pyproject.toml
git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"
```

---

## After the release

### Update the Homebrew formula

Once binaries are live on the GitHub Release, update the tap:

```bash
# Download both macOS archives and compute SHA256
ARM_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-aarch64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')
X86_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-x86_64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')

echo "ARM SHA256:   $ARM_SHA"
echo "x86_64 SHA256: $X86_SHA"
```

Then edit `Formula/edgecrab.rb` in `homebrew-tap` with the new version + SHA256 values, commit, and push.

### Verify all install methods

```bash
# Docker (should pull the arm64 image on Apple Silicon)
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run --rm ghcr.io/raphaelmansuy/edgecrab:latest --version

# npm (fresh install, no cache)
npm install -g edgecrab-cli
edgecrab --version

# pip
pip install --force-reinstall edgecrab-cli
edgecrab --version

# Homebrew
brew upgrade edgecrab
edgecrab --version
```

---

## Required secrets / environments

| Secret | Where | Used by |
|---|---|---|
| `NPM_TOKEN` | `npm` environment | `release-npm-cli.yml` |
| `CARGO_REGISTRY_TOKEN` | repository secrets | `release-rust.yml` |
| PyPI OIDC trusted publisher | `pypi` environment | `release-pypi-cli.yml` |
| `GITHUB_TOKEN` | auto-provisioned | all workflows |

---

## Versioning policy

EdgeCrab follows [Semantic Versioning](https://semver.org):

- **PATCH** (`0.1.x`) — bug fixes, dependency updates, documentation
- **MINOR** (`0.x.0`) — new features, backwards-compatible changes
- **MAJOR** (`x.0.0`) — breaking CLI / config / API changes
