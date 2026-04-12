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

| Secret | Where | Used by | Type |
|---|---|---|---|
| `NPM_TOKEN` | `npm` environment | `release-npm-cli.yml` | npm Bearer token |
| `CARGO_REGISTRY_TOKEN` | repository secrets | `release-rust.yml` | Cargo API token |
| PyPI OIDC trusted publisher | `pypi` environment | `release-pypi-cli.yml` | OIDC federated credential |
| `HOMEBREW_TAP_PUSH_TOKEN` | repository secrets | `release-homebrew-tap.yml` | **Deprecated: Use GitHub App instead** |
| `GITHUB_TOKEN` | auto-provisioned | all workflows | GitHub Actions auto-token |

### ⚠️ Homebrew Tap Authentication (Security Best Practice)

**Current approach (GitHub PAT):** Uses `HOMEBREW_TAP_PUSH_TOKEN` secret
- ❌ Persistent token stored in repository secrets
- ❌ Requires manual rotation
- ❌ High-privilege token (all user permissions)
- ❌ No automatic expiration

**Recommended approach (GitHub App):** Use a private GitHub App
- ✅ Scoped permissions (only `contents: write`)
- ✅ Installation-specific tokens
- ✅ Automatic short-lived token generation
- ✅ Better audit trail
- ✅ Revokable per installation
- ✅ No user account dependency

#### Setting up a GitHub App for Homebrew Tap Updates

**Step 1: Create a GitHub App**

1. Go to https://github.com/settings/apps/new
2. Fill in the form:
   - **App name:** `edgecrab-homebrew-updater` (or similar)
   - **Homepage URL:** `https://github.com/raphaelmansuy/edgecrab`
   - **Callback URL:** Leave blank (not needed for server-to-server)
   - **Setup URL:** Leave blank
   - **Request user authorization:** Uncheck
   - **Expire user authorization tokens:** N/A (skip)
   - **Webhook:** Uncheck (not needed)

3. Under **Permissions**, set:
   - **Repository permissions:**
     - `Contents: Read & write` (for updating Formula/)
   - **Account permissions:** None

4. Under **Where can the app be installed?:**
   - ✅ Only on this account

5. Click "Create GitHub App"

**Step 2: Generate and store the private key**

1. In the app settings, scroll to "Private keys"
2. Click "Generate a private key"
3. Store it securely as a repository secret named `HOMEBREW_TAP_APP_PRIVATE_KEY`

**Step 3: Note the App ID**

Find it in the "About" section. Store it as a repository secret named `HOMEBREW_TAP_APP_ID`.

**Step 4: Install the app to the Homebrew tap repository**

1. Go to the GitHub App settings → "Install App"
2. Click "Install" next to your account
3. Select the `raphaelmansuy/homebrew-tap` repository
4. Click "Install"

**Step 5: Update the workflow**

Replace the `release-homebrew-tap.yml` workflow to use the GitHub App:

```yaml
name: 'Release — Homebrew Tap'
on:
  workflow_run:
    workflows: ['Release — Native Binaries']
    types: [completed]

jobs:
  update-homebrew:
    runs-on: ubuntu-latest
    if: github.event.workflow_run.conclusion == 'success'
    steps:
      - uses: actions/checkout@v4
        with:
          repository: raphaelmansuy/homebrew-tap
          token: ${{ steps.app-token.outputs.token }}

      - name: Generate GitHub App Token
        id: app-token
        uses: actions/create-github-app-token@v1
        with:
          app-id: ${{ secrets.HOMEBREW_TAP_APP_ID }}
          private-key: ${{ secrets.HOMEBREW_TAP_APP_PRIVATE_KEY }}
          owner: raphaelmansuy

      - name: Download checksums from main edgecrab release
        run: |
          VERSION=$(jq -r '.tag_name' <<< '${{ github.event.workflow_run.name }}' | sed 's/v//')
          gh release download "v${VERSION}" \
            --repo raphaelmansuy/edgecrab \
            --pattern edgecrab-checksums.txt
          cat edgecrab-checksums.txt
        env:
          GITHUB_TOKEN: ${{ github.token }}

      - name: Update Homebrew formula
        run: |
          # Extract checksums and update Formula/edgecrab.rb
          VERSION=$(cat edgecrab-checksums.txt | head -1 | awk '{print $NF}')
          ARM_SHA=$(grep 'aarch64-apple-darwin' edgecrab-checksums.txt | awk '{print $1}')
          X86_SHA=$(grep 'x86_64-apple-darwin' edgecrab-checksums.txt | awk '{print $1}')
          
          # Update formula (simplified example)
          sed -i "s/version \".*\"/version \"${VERSION}\"/" Formula/edgecrab.rb
          sed -i "s/sha256 \".*\" => :arm64/sha256 \"${ARM_SHA}\" => :arm64/" Formula/edgecrab.rb
          sed -i "s/sha256 \".*\"/sha256 \"${X86_SHA}\"/" Formula/edgecrab.rb
          
          git config user.name "edgecrab-bot[bot]"
          git config user.email "edgecrab-bot[bot]@users.noreply.github.com"
          git add Formula/edgecrab.rb
          git commit -m "chore: update edgecrab to ${VERSION}"
          git push
```

**Step 6: Delete the old secret**

Once the App-based workflow is working, remove `HOMEBREW_TAP_PUSH_TOKEN` from repository secrets.

#### Why GitHub App is better

| Aspect | GitHub PAT | GitHub App |
|--------|----------|-----------|
| **Scope** | User's full permissions | Explicitly defined permissions |
| **Lifetime** | Indefinite (until manual revocation) | Automatic (generated per workflow) |
| **Revocation** | Manual, per-token | Per-installation, immediate |
| **Audit** | Tied to user account | Tied to app, cleaner audit trail |
| **Rate limit** | Shared with user | Separate, isolated quota |
| **Key rotation** | Manual | Automatic per-workflow |
| **Compromise impact** | User account exposed | Only that app's permissions exposed |

#### Fallback: If using GitHub PAT

If you must use PAT temporarily, follow these security practices:

```bash
# Create a minimal-scope PAT (via GitHub UI):
# 1. Go to Settings → Developer settings → Personal access tokens → Fine-grained tokens
# 2. Create new token with:
#    - Expiration: 90 days (automatic rotation policy)
#    - Permissions:
#      - Repository: Select only raphaelmansuy/homebrew-tap
#      - Contents: Read & write
#    - No Organization permissions
# 3. Document the token's purpose and rotation date
# 4. Set calendar reminder for rotation

# Store in repository secrets as HOMEBREW_TAP_PUSH_TOKEN
```

---

## Versioning policy

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
