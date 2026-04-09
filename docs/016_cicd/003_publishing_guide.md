# EdgeCrab — Publication Guide

> **Single authoritative reference** for every release artifact: 10 Rust crates, 2 npm packages, 2 PyPI packages, 1 Docker image, pre-built native binaries for 5 platforms, and the docs site.  
> Read [001_secrets_setup.md](001_secrets_setup.md) before your very first release — every secret must be configured or CI will fail silently.

---

## Artifact map

| Artifact | Registry | CI workflow | Makefile target | Trigger |
|---|---|---|---|---|
| Native binaries (×5 platforms) | GitHub Releases | `release-binaries.yml` | *(CI only)* | tag `v*` |
| Rust crates (×10) | [crates.io](https://crates.io) | `release-rust.yml` | `make publish-rust` | tag `v*` |
| Python SDK (`edgecrab-sdk`) | [PyPI](https://pypi.org) | `release-python.yml` | `make publish-python` | tag `v*` |
| Node.js SDK (`edgecrab-sdk`) | [npm](https://npmjs.com) | `release-node.yml` | `make publish-node` | tag `v*` |
| npm CLI wrapper (`edgecrab-cli`) | [npm](https://npmjs.com) | `release-npm-cli.yml` | `make publish-npm-cli` | tag `v*` |
| PyPI CLI wrapper (`edgecrab-cli`) | [PyPI](https://pypi.org) | `release-pypi-cli.yml` | `make publish-pypi-cli` | tag `v*` |
| Docker image | [GHCR](https://ghcr.io) | `release-docker.yml` | *(CI only)* | tag `v*` |
| Docs site | GitHub Pages / www.edgecrab.com | `deploy-site.yml` | `make site-deploy` | push to `main` |

> **Why native binaries matter:** `edgecrab-cli` (both npm and PyPI wrappers) downloads the pre-built Rust binary from GitHub Releases at install time. The `release-binaries.yml` workflow must complete before end-users install those wrappers.

---

## Standard release workflow (step by step)

### Step 1 — Pre-flight checks

```bash
make ci              # fmt-check + clippy + all tests (must be green)
make publish-all-dry # dry-run every package (cargo --dry-run + npm pack + twine check)
```

### Step 2 — Bump versions

One command updates all manifests consistently:

```bash
make version-bump VERSION=0.2.0
git add -A && git commit -m "chore: bump version to 0.2.0" && git push
```

This updates the canonical workspace version in `Cargo.toml` and then syncs
the derived package versions in `sdks/node/package.json`,
`sdks/npm-cli/package.json`, `sdks/python/pyproject.toml`, and
`sdks/pypi-cli/edgecrab_cli/_version.py`.

First principle: every release channel should derive from the smallest possible
set of version-bearing files.

- canonical release authority: `Cargo.toml` `[workspace.package].version`
- sync command: `./scripts/release-version.sh set <version>`
- CI guardrail: `./scripts/release-version.sh check`

- Node SDK package version derives from `sdks/node/package.json`
- npm CLI wrapper binary tag derives from `sdks/npm-cli/package.json`
- PyPI CLI wrapper package version and binary tag derive from `sdks/pypi-cli/edgecrab_cli/_version.py`
- generated build output does not participate in release bookkeeping

### Step 3 — Tag → CI publishes everything

```bash
make tag-release VERSION=0.2.0
# equivalent: git tag -a v0.2.0 -m "Release v0.2.0" && git push origin v0.2.0
```

One annotated tag triggers **seven** workflows in parallel:

```
git push origin v0.2.0
    │
    ├── release-binaries.yml   → cross-compile + upload native bins to GH Release
    ├── release-rust.yml       → publish 10 crates to crates.io
    ├── release-python.yml     → publish edgecrab-sdk to PyPI (multi-platform wheels)
    ├── release-node.yml       → publish edgecrab-sdk to npm
    ├── release-npm-cli.yml    → publish edgecrab-cli to npm
    ├── release-pypi-cli.yml   → publish edgecrab-cli to PyPI
    └── release-docker.yml     → push multi-arch Docker image to GHCR
```

### Step 4 — Monitor

```bash
GH_PAGER='' gh run list --limit 10
```

### Step 5 — Verify

```bash
cargo search edgecrab-cli           # must show 0.2.0
npm view edgecrab-sdk version       # must show 0.2.0
npm view edgecrab-cli version       # must show 0.2.0
pip index versions edgecrab-sdk     # must include 0.2.0
pip index versions edgecrab-cli     # must include 0.2.0
docker pull ghcr.io/raphaelmansuy/edgecrab:0.2.0
gh release download v0.2.0 --pattern edgecrab-checksums.txt --repo raphaelmansuy/edgecrab
curl -I https://www.edgecrab.com    # HTTP 200
```

---

## Manual publish from workstation

Use when CI is unavailable, for first-time setup, or to republish a specific package.

### Prerequisites

```bash
# Rust — log in once; token stored in ~/.cargo/credentials.toml
cargo login        # paste CARGO_REGISTRY_TOKEN from crates.io

# npm — log in once; token stored in ~/.npmrc
npm login          # or: echo "//registry.npmjs.org/:_authToken=<NPM_TOKEN>" >> ~/.npmrc

# PyPI — add token to ~/.pypirc
# pypi.org → Account Settings → API tokens → New token (scope: entire account for first publish)
cat >> ~/.pypirc << 'EOF'
[distutils]
index-servers = pypi
[pypi]
username = __token__
password = pypi-AgAAA...your-token-here
EOF
chmod 600 ~/.pypirc

# Build tools
pip install --upgrade build twine
```

### Publish Rust crates (dependency order)

```bash
make publish-rust
```

Equivalent manual steps:

```bash
cargo publish -p edgecrab-types && sleep 30
cargo publish -p edgecrab-security --no-verify && sleep 30
cargo publish -p edgecrab-state    --no-verify && sleep 30
cargo publish -p edgecrab-cron     --no-verify && sleep 30
cargo publish -p edgecrab-tools    --no-verify && sleep 30
cargo publish -p edgecrab-core     --no-verify && sleep 30
cargo publish -p edgecrab-gateway  --no-verify && sleep 30
cargo publish -p edgecrab-acp      --no-verify && sleep 30
cargo publish -p edgecrab-migrate  --no-verify && sleep 30
cargo publish -p edgecrab-cli      --no-verify
```

> `--no-verify` is required for crates with workspace path dependencies.
> The repository CI is the correctness gate.

### Publish Python SDK

```bash
make publish-python
# Manual: cd sdks/python && python -m build && twine upload dist/*
```

### Publish Node.js SDK

```bash
make publish-node
# Manual: cd sdks/node && npm ci && npm run build && npm publish --access public
```

### Publish npm CLI wrapper

```bash
make publish-npm-cli
# Manual: cd sdks/npm-cli && npm publish --access public
```

### Publish PyPI CLI wrapper

```bash
make publish-pypi-cli
# Manual: cd sdks/pypi-cli && python -m build && twine upload dist/*
```

### Publish all at once

```bash
make publish-all   # Rust + Python + Node + npm-cli + pypi-cli
```

---

## Local publish (no registry — development only)

Use these targets to build and install the Python wheels and npm packages **directly on your workstation** without pushing to any registry. Ideal for testing SDK changes end-to-end before tagging a release.

### All local packages at once

```bash
make publish-local
# Runs: publish-python-local + publish-node-local + publish-npm-cli-local + publish-pypi-cli-local
```

### Individual local targets

| Target | What it does |
|---|---|
| `make publish-python-local` | Builds `edgecrab-sdk` wheel → `pip install --force-reinstall` |
| `make publish-node-local` | Builds `edgecrab-sdk` TypeScript → `npm link --force` |
| `make publish-npm-cli-local` | `npm link` for the `edgecrab-cli` npm wrapper |
| `make publish-pypi-cli-local` | Builds `edgecrab-cli` wheel → `pip install --force-reinstall` |

After running `publish-local`, verify:

```bash
# Python
pip show edgecrab-sdk edgecrab-cli

# npm (link makes packages available globally)
npm list -g --depth=0 | grep edgecrab
```

> **Note:** `npm link` installs packages into your global node prefix (managed by your Node version manager, e.g. fnm/nvm). Both `edgecrab-sdk` and `edgecrab-cli` expose a binary named `edgecrab`, so `publish-node-local` uses `--force` to overwrite the bin symlink if it already exists from `publish-npm-cli-local`.

---

## CI/CD architecture

### Workflow inventory

| File | Trigger | Purpose |
|---|---|---|
| `ci.yml` | push/PR to `main` | Build, test, clippy, fmt, audit, site build |
| `release-binaries.yml` | tag `v*` | Cross-compile 5 platform binaries → GH Release |
| `release-rust.yml` | tag `v*` | Publish 10 crates in dependency order → GH Release |
| `release-node.yml` | tag `v*` | Publish `edgecrab-sdk` to npm |
| `release-python.yml` | tag `v*` | Build multi-platform wheels + publish `edgecrab-sdk` to PyPI |
| `release-npm-cli.yml` | tag `v*` | Publish `edgecrab-cli` to npm |
| `release-pypi-cli.yml` | tag `v*` | Publish `edgecrab-cli` to PyPI |
| `release-docker.yml` | tag `v*` | Build multi-arch Docker image → GHCR |
| `deploy-site.yml` | push to `main` touching `site/` | Astro build → GitHub Pages |

### Required secrets and environments

```
CARGO_REGISTRY_TOKEN   repo secret   — crates.io token (publish-new + publish-update scopes)
NPM_TOKEN              env secret    — npm automation token (environment: npm)
GITHUB_TOKEN           built-in      — GH Release upload, GHCR push, Pages deploy
PyPI OIDC              trusted pub   — no token stored; OIDC trusted publishing (environment: pypi)
```

GitHub environments to configure at **Repo → Settings → Environments**:

| Environment | Recommended protection | Secrets |
|---|---|---|
| `npm` | 1 required reviewer | `NPM_TOKEN` |
| `pypi` | 1 required reviewer | *(none — OIDC)* |
| `github-pages` | Optional | *(none — GITHUB_TOKEN)* |

### Native binary platform matrix

| Runner | Target | Archive |
|---|---|---|
| `ubuntu-latest` | `x86_64-unknown-linux-gnu` | `edgecrab-x86_64-unknown-linux-gnu.tar.gz` |
| `ubuntu-latest` + `cross` | `aarch64-unknown-linux-gnu` | `edgecrab-aarch64-unknown-linux-gnu.tar.gz` |
| `macos-13` (Intel) | `x86_64-apple-darwin` | `edgecrab-x86_64-apple-darwin.tar.gz` |
| `macos-14` (M1) | `aarch64-apple-darwin` | `edgecrab-aarch64-apple-darwin.tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `edgecrab-x86_64-pc-windows-msvc.zip` |

These archive names are hardcoded in `sdks/npm-cli/scripts/install.js` and
`sdks/pypi-cli/edgecrab_cli/_binary.py`. Do not rename them without updating both.

### Rust crate publish order

```
edgecrab-types → edgecrab-security → edgecrab-state → edgecrab-cron
      → edgecrab-tools → edgecrab-core → edgecrab-gateway
      → edgecrab-acp → edgecrab-migrate → edgecrab-cli (last)
```

`release-rust.yml` enforces that the tag version equals the `edgecrab-core` version in
`Cargo.toml`. A mismatch aborts before publishing anything.

---

## Versioning policy

All 10 Rust crates, both SDKs, and both CLI wrappers share the **same version number** always.
Use `make version-bump VERSION=x.y.z` or `./scripts/release-version.sh set x.y.z` to keep them in sync.

| Change | Bump | Example |
|---|---|---|
| Bug fix | patch | `0.1.0 → 0.1.1` |
| New feature | minor | `0.1.0 → 0.2.0` |
| Breaking API change | major | `0.1.0 → 1.0.0` |
| Pre-release | suffix | `0.2.0-beta.1` |

---

## CHANGELOG generation

```bash
cargo install git-cliff
git cliff --unreleased           # preview next release notes
git cliff --output CHANGELOG.md  # regenerate full changelog
```

`release-rust.yml` auto-generates `RELEASE_NOTES.md` per tag and attaches it to the GitHub
Release. Conventional commit prefixes: `feat:` `fix:` `perf:` `docs:` `ci:` `chore:` `BREAKING CHANGE:`.

---

## Troubleshooting

**`cargo publish` fails with "already exists on crates.io"** — non-fatal; the workflow and
Makefile both skip gracefully. crates.io versions are immutable.

**npm publish fails with "cannot publish over the previously published version"** — same skip
behaviour; safe to ignore.

**PyPI `skip-existing: true`** — duplicate uploads are silently skipped by the workflow.

**Binary missing from GitHub Release after npm/PyPI CLI install fails** — re-run
`release-binaries.yml` via `workflow_dispatch` or upload manually:

```bash
cargo build --release --target aarch64-apple-darwin -p edgecrab-cli
tar -czf edgecrab-aarch64-apple-darwin.tar.gz -C target/aarch64-apple-darwin/release edgecrab
gh release upload v0.1.0 edgecrab-aarch64-apple-darwin.tar.gz --clobber
```

**`release-rust.yml` fails with "tag version ≠ Cargo.toml version"** — delete the tag, run
`make version-bump`, commit, re-tag:

```bash
git tag -d v0.2.0 && git push origin :refs/tags/v0.2.0
make version-bump VERSION=0.2.0
git add -A && git commit -m "chore: bump version to 0.2.0" && git push
make tag-release VERSION=0.2.0
```

**PyPI OIDC failing** — the trusted publisher on pypi.org must exactly match: owner
`raphaelmansuy`, repo `edgecrab`, workflow filename `release-pypi-cli.yml`, environment `pypi`.

---

## Quick reference cheatsheet

```bash
# Full release
make ci && make publish-all-dry
make version-bump VERSION=0.2.0
git add -A && git commit -m "chore: bump version to 0.2.0" && git push
make tag-release VERSION=0.2.0
GH_PAGER='' gh run list --limit 10

# Verify
cargo search edgecrab-cli && npm view edgecrab-cli version && pip index versions edgecrab-cli

# Local install (no registry — dev/testing)
make publish-local                  # all Python + npm packages
make publish-python-local           # Python SDK only
make publish-npm-cli-local          # npm CLI wrapper only

# Manual workstation publish (no tag/CI)
make publish-rust && make publish-python && make publish-node
make publish-npm-cli && make publish-pypi-cli
```

---

## Cross-references

- Secrets setup → [001_secrets_setup.md](001_secrets_setup.md)
- GitHub Pages DNS → [002_github_pages_dns.md](002_github_pages_dns.md)
- Crate dependency graph → [../002_architecture/002_crate_dependency_graph.md](../002_architecture/002_crate_dependency_graph.md)
- All workflow files → [../../.github/workflows/](../../.github/workflows/)
