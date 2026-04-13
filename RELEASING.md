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

# 3. Commit, tag, push — let release-version.sh sync handle all derived files
git add Cargo.toml \
        sdks/npm-cli/package.json \
        sdks/node/package.json sdks/node/package-lock.json \
        sdks/pypi-cli/edgecrab_cli/_version.py \
        sdks/python/pyproject.toml sdks/python/edgecrab/_version.py
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
formula change using a **GitHub App** (recommended) or `HOMEBREW_TAP_PUSH_TOKEN` (legacy PAT).

**🔒 Recommended: Use GitHub App (see [Homebrew Tap Authentication](#-homebrew-tap-authentication-security-best-practice) section)**

The GitHub App approach is more secure because:
- Tokens are short-lived and scoped to minimal permissions
- Automatic token generation per-workflow run
- No persistent secrets stored in repository
- Better audit trail
- Revoke instantly if needed

**Setup GitHub App:**
1. Create a GitHub App with `contents: write` permission on the tap repository
2. Generate and store the private key and app ID as secrets
3. Update the workflow to use `actions/create-github-app-token@v1`

See the **[Homebrew Tap Authentication](#-homebrew-tap-authentication-security-best-practice)** section below for detailed setup instructions.

**If using legacy PAT approach (not recommended):**

If you cannot use a GitHub App, you can configure `HOMEBREW_TAP_PUSH_TOKEN` in
repository secrets, but this should only be a temporary solution. Follow the
security practices documented in the [Fallback](#fallback-if-using-github-pat) section

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

# pip (Python SDK)
pip install --force-reinstall edgecrab
python -c "import edgecrab; print('edgecrab SDK ok')"

# pip (CLI wrapper)
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

| Secret | Where | Used by | Type |
|---|---|---|---|
| `NPM_TOKEN` | `npm` environment | `release-npm-cli.yml` | npm Bearer token |
| `CARGO_REGISTRY_TOKEN` | repository secrets | `release-rust.yml` | Cargo API token |
| PyPI OIDC trusted publisher | `pypi` environment | `release-pypi-cli.yml` | OIDC federated credential |
| `PYPI_API_TOKEN` | `pypi` environment + repository secrets | `release-python.yml` | PyPI API token — required when OIDC trusted publisher is not yet registered for a new project name (e.g. first publish of `edgecrab`); once the project exists on PyPI, OIDC takes over for `release-pypi-cli.yml` |
| `HOMEBREW_TAP_DEPLOY_KEY` | repository secrets | `release-homebrew-tap.yml` | ed25519 SSH deploy key with write access to `raphaelmansuy/homebrew-tap` (key id 148386829); **primary auth method since v0.4.1** |
| `HOMEBREW_TAP_PUSH_TOKEN` | repository secrets | `release-homebrew-tap.yml` | **Deprecated** — legacy GitHub PAT; superseded by `HOMEBREW_TAP_DEPLOY_KEY` |
| `GITHUB_TOKEN` | auto-provisioned | all workflows | GitHub Actions auto-token |

### Homebrew Tap Authentication — current setup (v0.4.1+)

The tap is updated automatically via an **SSH deploy key** stored as `HOMEBREW_TAP_DEPLOY_KEY`.

**How it works:**
1. `release-homebrew-tap.yml` decodes the key at runtime (`base64 -d`) and adds it to `ssh-agent`
2. Clones `raphaelmansuy/homebrew-tap` over SSH, updates the formula, and pushes
3. Three-tier fallback in the workflow: deploy key → GitHub App tokens (if configured) → PAT → skip (non-fatal)

**Key details:**
- Key id: `148386829`, type: `ed25519`
- Grant scope: write access to `raphaelmansuy/homebrew-tap` only
- Stored as: base64-encoded PEM in repository secret `HOMEBREW_TAP_DEPLOY_KEY`

**To rotate the deploy key:**
```bash
# 1. Generate a new ed25519 key pair
ssh-keygen -t ed25519 -C "edgecrab-homebrew-deploy" -f homebrew_deploy_key -N ""

# 2. Add the public key to raphaelmansuy/homebrew-tap → Settings → Deploy keys
#    Title: edgecrab-homebrew-deploy, Allow write access: ✅

# 3. Store the base64-encoded private key as the repository secret
gh secret set HOMEBREW_TAP_DEPLOY_KEY -R raphaelmansuy/edgecrab < <(base64 < homebrew_deploy_key)

# 4. Delete the local key files
rm homebrew_deploy_key homebrew_deploy_key.pub

# 5. Remove the old deploy key from raphaelmansuy/homebrew-tap → Settings → Deploy keys
```

#### Fallback: GitHub App (optional upgrade)

If you want short-lived app tokens instead of a static deploy key, a GitHub App with `contents: write` on the tap repo can be configured. Store `HOMEBREW_TAP_APP_ID` and `HOMEBREW_TAP_APP_PRIVATE_KEY` as repository secrets, then update the workflow to use `actions/create-github-app-token@v1`. The deploy key approach is simpler and currently sufficient.

#### Fallback: GitHub PAT (deprecated)

`HOMEBREW_TAP_PUSH_TOKEN` (fine-grained PAT, `contents: write` on `raphaelmansuy/homebrew-tap`, 90-day expiry) is still checked as a last resort but its use is discouraged. Remove it once the deploy key is confirmed working.

---

## Lessons learned

### PyPI package naming: first-time publish requires a token

**Background (v0.4.1):** The Python SDK was originally published to PyPI as `edgecrab-sdk`. This meant
`pip install edgecrab` always failed — the project `edgecrab` did not exist on PyPI.

**Fix:** Rename the package in `sdks/python/pyproject.toml` from `edgecrab-sdk` to `edgecrab`.

**Gotcha — OIDC cannot create a brand-new project:** PyPI OIDC trusted publishers are registered
per project name. When `edgecrab` didn't exist on PyPI yet, the workflow's OIDC credential (which
was registered for `edgecrab-sdk`) could not create it. Solution:

1. Build locally: `cd sdks/python && python3 -m build --outdir dist/`
2. Upload with a PyPI API token (stored in `~/.pypirc`): `python3 -m twine upload dist/*`
3. Once the project exists, future CI publishes via `PYPI_API_TOKEN` secret work fine.
4. Eventually register an OIDC trusted publisher on pypi.org for `edgecrab` to allow passwordless CI.

**Note:** `edgecrab-sdk` still exists on PyPI (0.4.1) and cannot be deleted, but all future versions
will be published under `edgecrab` only. The Node.js SDK remains `edgecrab-sdk` on npm (intentional).

---

### Version sync: always use `release-version.sh sync`

**Background (v0.4.1):** A hotfix commit manually bumped most version files but missed
`sdks/pypi-cli/edgecrab_cli/_version.py`, causing a CI failure on the PyPI CLI workflow.

**Rule:** Never manually edit version strings in individual files. Always run:

```bash
./scripts/release-version.sh sync
```

This is the **only** authoritative way to update all derived versions from `Cargo.toml`. If you have
already committed without syncing, run sync and add it as an amend or fixup commit before the tag.

---

### Homebrew tap: deploy key is the current primary auth

**Background:** The tap was initially configured with a GitHub PAT (`HOMEBREW_TAP_PUSH_TOKEN`), which
expired and caused v0.3.4 tap updates to silently skip. The `RELEASING.md` at the time documented
a GitHub App as the "recommended" path but neither was actually configured.

**Current setup (v0.4.1+):** An ed25519 SSH deploy key (`HOMEBREW_TAP_DEPLOY_KEY`) with write access
to `raphaelmansuy/homebrew-tap` is stored as a repository secret. This is what actually runs.

The `release-homebrew-tap.yml` workflow tries in order: deploy key → GitHub App tokens → PAT → skip.

---



EdgeCrab follows [Semantic Versioning](https://semver.org):

- **PATCH** (`0.1.x`) — bug fixes, dependency updates, documentation
- **MINOR** (`0.x.0`) — new features, backwards-compatible changes
- **MAJOR** (`x.0.0`) — breaking CLI / config / API changes

---

## Release Checklist Summary

After pushing a release tag, all workflows should complete within 10-15 minutes. Here's the expected status:

| Workflow | Status | Notes |
|---|---|---|
| **Release — Native Binaries** | ✅ Success | 5 native archives (macOS arm64/x86_64, Linux, Windows) |
| **Release — Docker (GHCR)** | ✅ Success | Published to `ghcr.io/raphaelmansuy/edgecrab:vX.Y.Z` |
| **Release — Node.js (npm)** | ✅ Success | Published to npm registry |
| **Release — Python (PyPI)** | ✅ Success | Published to PyPI |
| **Release — Rust (crates.io)** | ✅ Success | Published workspace crates in dependency order |
| **Release — npm CLI (edgecrab-cli)** | ✅ Success | npm wrapper for binaries |
| **Release — PyPI CLI (edgecrab-cli)** | ✅ Success | PyPI wrapper for binaries |
| **Release — Homebrew Tap** | ⚠️ Manual if token missing | See [Configuring the secret](#configuring-the-secret) above |

### What to do if workflows fail

1. **Check GitHub Actions logs** — Click the red/yellow workflow in Actions tab
2. **Common failures:**
   - `HOMEBREW_TAP_PUSH_TOKEN` missing or invalid → Migrate to GitHub App (recommended)
   - `Insufficient permissions` on Homebrew tap workflow → Verify GitHub App has `contents: write` on tap repo
   - crates.io timeouts → Usually transient; manual retry often succeeds
   - Binary build failures → Check logs for compilation errors; fix and re-tag
3. **Re-run failed workflows** — Use `workflow_dispatch` to replay with the same tag
4. **Manual publish steps** — Each workflow has a documented fallback procedure in this file

### Example: v0.3.4 Release (2026-04-12)

All critical release workflows succeeded:
- ✅ Native binaries built and published to GitHub Release
- ✅ Docker image built and published to GHCR
- ✅ npm packages published
- ✅ PyPI packages published
- ✅ Rust crates.io publish completed
- ⚠️ Homebrew Tap failed due to missing authentication

**Why Homebrew Tap failed:**
The workflow was configured to use `HOMEBREW_TAP_PUSH_TOKEN` (legacy GitHub PAT), which was not
configured in repository secrets. The workflow exited cleanly with:
```
HOMEBREW_TAP_PUSH_TOKEN is not configured; automatic tap push cannot proceed.
```

**Resolution:**
To fix this for future releases, migrate to the **GitHub App** approach (recommended):
1. Follow the setup steps in [Homebrew Tap Authentication](#-homebrew-tap-authentication-security-best-practice)
2. Update the `release-homebrew-tap.yml` workflow to use `actions/create-github-app-token@v1`
3. Test with a manual workflow dispatch

**After GitHub App migration:**
Future releases will automatically update the Homebrew formula without manual intervention.

**For now (if needed manually):**
The Homebrew formula in `raphaelmansuy/homebrew-tap` can be manually updated using the
fallback procedure documented above. All other distribution channels are live and available.
