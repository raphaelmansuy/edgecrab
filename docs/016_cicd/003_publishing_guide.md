# EdgeCrab — Complete Publication Guide

> **Purpose**: Single authoritative reference for releasing EdgeCrab. Covers manual workstation releases, tag-based CI/CD automation, and the documentation site. Read [001_secrets_setup.md](001_secrets_setup.md) before your first release.

---

## Quick Reference: What Goes Where

| Artifact | Registry | Trigger | Makefile target |
|---|---|---|---|
| Rust crates (×10) | [crates.io](https://crates.io) | tag `v*` → `release-rust.yml` | `make publish-rust` |
| Python SDK | [PyPI](https://pypi.org) | tag `v*` → `release-python.yml` | `make publish-python` |
| Node.js SDK | [npm](https://npmjs.com) | tag `v*` → `release-node.yml` | `make publish-node` |
| npm CLI wrapper | [npm](https://npmjs.com) | tag `v*` → `release-node.yml` | `make publish-npm-cli` |
| PyPI CLI wrapper | [PyPI](https://pypi.org) | tag `v*` → `release-python.yml` | `make publish-pypi-cli` |
| Docker image | [GHCR](https://ghcr.io) | tag `v*` → `release-docker.yml` | *(CI only)* |
| Docs site | GitHub Pages / www.edgecrab.com | push to `main` touching `site/` | `make site-deploy` |

---

## Recommended Release Process

### 1 — Pre-release checks

```bash
# Must all pass clean before tagging
make ci                 # fmt-check + lint + all tests
make publish-rust-dry   # dry-run cargo publish for types crate
make publish-python-dry # dry-run Python build + twine check
make publish-node-dry   # dry-run npm pack
make site-build         # confirm the docs site builds cleanly
```

### 2 — Bump versions consistently

All version numbers must match. The release-rust workflow enforces this automatically but bump manually first:

```bash
# 1. Edit Cargo.toml workspace version
#    Change:  version = "0.1.0"    (root workspace Cargo.toml)
#    To:      version = "0.2.0"

# 2. Sync Node SDK version
cd sdks/node   && npm version 0.2.0 --no-git-tag-version
cd sdks/python && sed -i 's/version = ".*"/version = "0.2.0"/' pyproject.toml

# 3. Sync npm-cli and pypi-cli wrappers
cd sdks/npm-cli  && npm version 0.2.0 --no-git-tag-version
cd sdks/pypi-cli && sed -i 's/version = ".*"/version = "0.2.0"/' pyproject.toml

# 4. Commit everything
git add -A
git commit -m "chore: bump version to 0.2.0"
git push
```

### 3 — Tag → all CI/CD publish workflows fire

```bash
# This single tag triggers release-rust, release-node, release-python, release-docker
git tag v0.2.0
git push origin v0.2.0
```

Monitor the live runs:

```bash
GH_PAGER='' gh run list --limit 10
```

Open https://github.com/raphaelmansuy/edgecrab/actions for the full Actions tab.

### 4 — Post-release verification

| Check | Command / URL |
|---|---|
| Crates on crates.io | `cargo search edgecrab-cli` |
| npm SDK | `npm view edgecrab-sdk version` |
| PyPI SDK | `pip index versions edgecrab-sdk` |
| npm CLI | `npm view edgecrab-cli version` |
| PyPI CLI | `pip index versions edgecrab-cli` |
| Docker | `docker pull ghcr.io/raphaelmansuy/edgecrab:0.2.0` |
| Docs site | `curl -I https://www.edgecrab.com` → 200 |

---

## Manual Publish from Workstation

Use these when you need to publish without waiting for a CI tag run (hotfix, first-time setup, crates.io token rotation test).

### Required tools

```bash
# Rust publish (already in PATH if you have Cargo)
cargo --version

# Python publish
pip install --upgrade build twine
twine --version

# Node publish
node --version   # 18+
npm --version
```

### Required credentials

```bash
# Rust — log in once; token stored in ~/.cargo/credentials.toml
cargo login                       # paste CARGO_REGISTRY_TOKEN

# npm — log in once; token stored in ~/.npmrc
npm login                         # paste NPM_TOKEN or use --auth-type=legacy

# PyPI — tokens stored in ~/.pypirc or use twine --username/__token__
# If using OIDC on CI, you still need a manual token for workstation publish:
# pypi.org → Account Settings → API tokens → Add token
```

### Publish Rust crates (dependency order)

```bash
make publish-rust
# Equivalent manual steps:
cargo publish -p edgecrab-types
sleep 30
cargo publish -p edgecrab-security
sleep 30
cargo publish -p edgecrab-state
sleep 30
cargo publish -p edgecrab-tools --no-verify
sleep 30
cargo publish -p edgecrab-cron   --no-verify
sleep 30
cargo publish -p edgecrab-core   --no-verify
sleep 30
cargo publish -p edgecrab-gateway --no-verify
sleep 30
cargo publish -p edgecrab-acp    --no-verify
sleep 30
cargo publish -p edgecrab-migrate --no-verify
sleep 30
cargo publish -p edgecrab-cli    --no-verify
```

> **Why `--no-verify` for crates after `types`?** The workspace has path dependencies. `cargo publish --no-verify` skips the isolated build check; CI is the correctness gate.

### Publish Python SDK

```bash
make publish-python
# Manual:
cd sdks/python
python -m build       # creates dist/edgecrab_sdk-*.whl + .tar.gz
twine upload dist/*   # prompts for credentials or uses ~/.pypirc
```

### Publish Node.js SDK

```bash
make publish-node
# Manual:
cd sdks/node
npm ci
npm run build
npm publish --access public
```

### Publish npm CLI wrapper

```bash
make publish-npm-cli
# Manual:
cd sdks/npm-cli
npm publish --access public
```

### Publish PyPI CLI wrapper

```bash
make publish-pypi-cli
# Manual:
cd sdks/pypi-cli
python -m build
twine upload dist/*
```

### Publish all at once

```bash
make publish-all   # publish-rust + publish-python + publish-node + publish-npm-cli + publish-pypi-cli
```

### Deploy docs site manually

```bash
make site-deploy              # triggers GitHub Actions workflow_dispatch
make site-deploy-status       # check the latest deploy run status
```

Alternatively, push any change under `site/` to `main` — the workflow fires automatically.

---

## GitHub Actions: Tag-Based Release Architecture

```
git push origin v0.2.0
         │
         ├─── release-rust.yml    (tag: v[0-9]+.[0-9]+.[0-9]+)
         │    ├── Verify tag == Cargo.toml version
         │    ├── Generate RELEASE_NOTES.md via git-cliff
         │    ├── cargo publish -p edgecrab-types
         │    ├── sleep 30 ...
         │    ├── cargo publish -p edgecrab-cli
         │    └── gh release create
         │
         ├─── release-node.yml   (tag: v[0-9]+.[0-9]+.[0-9]+)
         │    ├── npm version $TAG
         │    ├── npm ci && npm run build
         │    └── npm publish --access public
         │
         ├─── release-python.yml  (tag: v[0-9]+.[0-9]+.[0-9]+)
         │    ├── build sdist (ubuntu)
         │    ├── build wheels (ubuntu + macos + windows) × 4 Python versions
         │    └── pypa/gh-action-pypi-publish (OIDC — no token stored)
         │
         └─── release-docker.yml  (tag: v[0-9]+.[0-9]+.[0-9]+)
              ├── docker buildx build --platform linux/amd64,linux/arm64
              └── docker push ghcr.io/raphaelmansuy/edgecrab:$TAG
```

Tag format: `v<major>.<minor>.<patch>` — no suffix for stable, suffix for pre-release (e.g. `v0.2.0-beta.1`).

The `release-rust.yml` workflow enforces that the tag version equals the `edgecrab-core` version in `Cargo.toml`. If they differ, the workflow exits with an error before publishing anything.

---

## Documentation Site (GitHub Pages)

The Astro docs site at https://www.edgecrab.com is deployed via GitHub Actions to GitHub Pages.

### Automatic deployment

Any push to `main` that touches files under `site/` or `.github/workflows/deploy-site.yml` fires the `deploy-site.yml` workflow.

### Manual deployment

```bash
make site-deploy               # fires workflow_dispatch on main
make site-deploy-status        # tail recent deploy runs
```

### Key files

| File | Role |
|---|---|
| `site/public/CNAME` | Custom domain (`www.edgecrab.com`) — must match repo Pages setting |
| `site/astro.config.mjs` → `site:` | Astro's canonical URL — must match CNAME |
| `.github/workflows/deploy-site.yml` | Build (pnpm build) + upload artifact + deploy-pages |

The HTTPS certificate for `www.edgecrab.com` is managed by GitHub and auto-renewed. Current expiry: **2026-07-04**.

### GitHub Pages configuration (current state)

```
Build type:     workflow (GitHub Actions)
Custom domain:  www.edgecrab.com
HTTPS enforced: true
Certificate:    approved (covers www.edgecrab.com + edgecrab.com)
```

To inspect live:
```bash
GH_PAGER='' gh api repos/raphaelmansuy/edgecrab/pages
```

---

## Environments and Protection Rules

GitHub environments provide deployment gates. Recommended config:

| Environment | Required reviewers | Secrets |
|---|---|---|
| `github-pages` | *(optional)* | none — uses GITHUB_TOKEN |
| `npm` | 1 reviewer | `NPM_TOKEN` |
| `pypi` | 1 reviewer | *(none — OIDC trusted publishing)* |

Set up environments at: **Repo → Settings → Environments**

---

## Versioning Strategy

EdgeCrab follows [Semantic Versioning](https://semver.org/):

| Change | Version bump | Example |
|---|---|---|
| Bug fix, backward compatible | patch | `0.1.0 → 0.1.1` |
| New feature, backward compatible | minor | `0.1.0 → 0.2.0` |
| Breaking API change | major | `0.1.0 → 1.0.0` |
| Pre-release | suffix | `0.2.0-beta.1` |

All 10 crates, both SDKs, and both CLI wrappers share the **same version number** at all times. The `release-rust.yml` workflow enforces this by comparing the git tag to the `Cargo.toml` workspace version.

---

## CHANGELOG

EdgeCrab uses [git-cliff](https://git-cliff.org/) to generate changelogs from [Conventional Commits](https://www.conventionalcommits.org/).

```bash
# Install
cargo install git-cliff

# Preview next release notes
git cliff --unreleased

# Generate full CHANGELOG.md
git cliff --output CHANGELOG.md
```

The `release-rust.yml` workflow automatically generates `RELEASE_NOTES.md` for each tag and attaches it to the GitHub Release.

Commit message prefixes used in this repo:

| Prefix | Changelog section |
|---|---|
| `feat:` | Features |
| `fix:` | Bug Fixes |
| `perf:` | Performance |
| `refactor:` | Refactoring |
| `docs:` | Documentation |
| `chore:` | Miscellaneous |
| `ci:` | CI/CD |
| `test:` | Tests |
| `BREAKING CHANGE:` | Breaking Changes |

---

## Troubleshooting

### `cargo publish` fails with "already exists on crates.io"

Not an error — the workflow skips already-published versions. Safe to ignore.

### npm publish fails with "cannot publish over the previously published version"

Same as above — skip is handled in the workflow.

### PyPI OIDC fails with "invalid trusted publisher"

The OIDC publisher on PyPI must match the exact workflow filename and GitHub environment name. Check: pypi.org → Project → Publishing.

### Deploy-site workflow fails at "Creating Pages deployment failed"

GitHub Pages must be enabled in the repo settings. Run:
```bash
GH_PAGER='' gh api --method POST repos/raphaelmansuy/edgecrab/pages --field build_type=workflow
```

### HTTPS not enforced after domain change

Wait for DNS propagation and certificate issuance, then:
```bash
GH_PAGER='' gh api --method PUT repos/raphaelmansuy/edgecrab/pages --field cname=www.edgecrab.com --field https_enforced=true
```

---

## Cross-References

- Secrets setup → [001_secrets_setup.md](001_secrets_setup.md)
- GitHub Pages DNS → [002_github_pages_dns.md](002_github_pages_dns.md)
- Crate dependency graph → [../002_architecture/002_crate_dependency_graph.md](../002_architecture/002_crate_dependency_graph.md)
- All workflow files → [../../.github/workflows/](../../.github/workflows/)
