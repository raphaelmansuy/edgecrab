# 016.001 — CI/CD Secrets Setup

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 008.001 Environments](../008_environments/001_environments.md)
> **Applies to**: GitHub Actions workflows in `.github/workflows/`
> **Last updated**: 2026-04-05

This guide covers every secret and environment variable required to make all CI/CD pipelines
green — crates.io, npm, PyPI, and Docker (GHCR). Follow each section in order before pushing
a release tag.

---

## 1. Quick Reference

| Secret / Setting | Where to add | Used by workflow |
|---|---|---|
| `CARGO_REGISTRY_TOKEN` | Repository → Secrets | `release-rust.yml` |
| `NPM_TOKEN` | Environment `npm` → Secrets | `release-node.yml` |
| PyPI Trusted Publisher | PyPI project settings (no secret) | `release-python.yml` |
| `GITHUB_TOKEN` | Built-in (no setup) | `release-docker.yml`, all workflows |

Docker/GHCR uses the built-in `GITHUB_TOKEN` — no secret to create.

---

## 2. crates.io — `CARGO_REGISTRY_TOKEN`

### 2.1 Generate the token

1. Go to <https://crates.io> and sign in with your GitHub account.
2. Click your avatar → **Account Settings**.
3. Scroll to **API Tokens** → **New Token**.
4. Name it `edgecrab-ci` (or similar), select scope **Publish new crates** and
   **Publish updates**, then click **Create Token**.
5. Copy the token immediately — it is shown **once only**.

### 2.2 Add to GitHub

1. Open your repository on GitHub.
2. **Settings** → **Secrets and variables** → **Actions** → **New repository secret**.
3. Name: `CARGO_REGISTRY_TOKEN`
4. Value: paste the crates.io token.
5. Click **Add secret**.

### 2.3 Verify

The `release-rust.yml` workflow publishes crates in dependency order:

```
edgecrab-types → edgecrab-security → edgecrab-state → edgecrab-tools
→ edgecrab-cron → edgecrab-core → edgecrab-cli → edgecrab-gateway
→ edgecrab-acp → edgecrab-migrate
```

A 30-second `sleep` is inserted between each publish step to let the crates.io index update.
If a version was already published, the step skips automatically (idempotent).

### 2.4 Crate ownership (first publish only)

For a brand-new crate name, the account that first publishes becomes the **owner**.
To add a co-owner via CLI:

```bash
cargo owner --add github:raphaelmansuy:maintainers edgecrab-core
```

---

## 3. npm — `NPM_TOKEN`

### 3.1 Generate the token

1. Go to <https://www.npmjs.com> and sign in.
2. Click your avatar → **Access Tokens** → **Generate New Token** → **Classic Token**.
3. Select type **Automation** (bypasses 2FA for CI).
4. Copy the token — shown once only.

### 3.2 Create the `npm` GitHub Environment

The `release-node.yml` workflow uses `environment: npm`, so the token must live there:

1. **Settings** → **Environments** → **New environment**.
2. Name it exactly `npm`.
3. (Optional) Add **Required reviewers** or branch protection as needed.
4. Click **Configure environment** → **Environment secrets** → **Add secret**.
5. Name: `NPM_TOKEN`
6. Value: paste the npm Automation token.
7. Save.

### 3.3 Package scope

The workflow publishes with `--access public`. If the package name in `sdks/node/package.json`
uses a **scoped name** (e.g. `@raphaelmansuy/edgecrab-sdk`), ensure the npm account owns that
scope, or create it at <https://www.npmjs.com/org/create>.

### 3.4 Verify

Trigger the workflow manually via **Actions** → **Release — Node.js (npm)** →
**Run workflow**, supplying a tag name like `v0.1.0`. Check the **Publish to npm** step output.

---

## 4. PyPI — Trusted Publisher (OIDC, no secret)

The Python release workflow uses `pypa/gh-action-pypi-publish` with **OIDC Trusted
Publisher** authentication, which means **no token is stored in GitHub**. The workflow requests
a short-lived credential at publish time.

### 4.1 Create the PyPI project (first publish only)

If the package does not yet exist on PyPI, publish once manually from your machine:

```bash
cd sdks/python
pip install build twine
python -m build
twine upload dist/*
```

Enter your PyPI username and password when prompted. This creates the project and makes your
account the owner.

### 4.2 Configure Trusted Publisher on PyPI

1. Sign in to <https://pypi.org>.
2. Go to **Your projects** → select `edgecrab-sdk` (or whatever `name` is in `pyproject.toml`).
3. **Manage** → **Publishing** → **Add a new publisher**.
4. Fill in:
   | Field | Value |
   |---|---|
   | Publisher | GitHub Actions |
   | Owner | `raphaelmansuy` |
   | Repository | `edgecrab` |
   | Workflow name | `release-python.yml` |
   | Environment name | `pypi` |
5. Click **Add**.

### 4.3 Create the `pypi` GitHub Environment

1. **Settings** → **Environments** → **New environment**.
2. Name it exactly `pypi`.
3. (Optional) Restrict to tag patterns like `v*` under **Deployment branches and tags**.
4. No secrets are needed — the Trusted Publisher handles authentication.

### 4.4 Required workflow permission

The `release-python.yml` already declares:

```yaml
permissions:
  id-token: write   # required for OIDC token exchange
  contents: read
```

This is mandatory for Trusted Publisher. Do not remove it.

### 4.5 Verify

Push a tag (`git tag v0.1.0 && git push --tags`). The `publish-pypi` job will appear under
**Actions** and request this environment's deployment. Approve if required reviewers are set,
then check PyPI for the new version at `https://pypi.org/project/edgecrab-sdk/`.

---

## 5. Docker — GHCR (no secrets needed)

The `release-docker.yml` workflow logs in with the built-in `GITHUB_TOKEN`:

```yaml
- uses: docker/login-action@v3
  with:
    registry: ghcr.io
    username: ${{ github.actor }}
    password: ${{ secrets.GITHUB_TOKEN }}
```

The workflow already declares `packages: write` permission, which `GITHUB_TOKEN` receives
automatically. **No manual secret is required.**

### 5.1 Make the package public (post first push)

After the first successful push, the package is private by default:

1. On GitHub, go to **Packages** (in the repository or org sidebar).
2. Select `edgecrab`.
3. **Package settings** → **Danger Zone** → **Change visibility** → **Public**.

---

## 6. Branch Protection & PR Approval

To let a single admin merge without a second reviewer:

1. **Settings** → **Branches** → **Add branch ruleset** (or edit existing rule for `main`).
2. Under **Require a pull request before merging**:
   - Enable **Require pull request reviews before merging**.
   - Set **Required approving reviews** to `1`.
3. Enable **Allow specified actors to bypass required pull requests**.
4. Add the admin user (or team) to the **bypass list**.
5. Save.

Alternatively, if you want the admin to self-merge without any approval:
set **Required approving reviews** to `0` — but be aware this disables the review gate
entirely for that branch. The bypass-list approach is recommended.

---

## 7. Release Checklist

Run through this list before pushing a release tag:

- [ ] `CARGO_REGISTRY_TOKEN` added to repository secrets
- [ ] `npm` environment created with `NPM_TOKEN` secret
- [ ] `pypi` environment created (no secret) + PyPI Trusted Publisher configured
- [ ] PyPI project created manually on first release
- [ ] GHCR package visibility set to **Public** after first Docker push
- [ ] `main` branch protection allows admin bypass for solo merges
- [ ] All CI checks green on the release commit:
  ```bash
  cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --all --check
  ```
- [ ] Version in `Cargo.toml` (edgecrab-core) matches the intended tag:
  ```bash
  # Must output the same version as your planned tag (without the 'v' prefix)
  cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name=="edgecrab-core") | .version'
  ```
- [ ] Python `pyproject.toml` and Node `package.json` versions will be overwritten
  automatically by the release workflows from the tag — no manual bump needed.

---

## 8. Troubleshooting

### `cargo publish` fails with "invalid token"

The token may have expired or been revoked. Regenerate at <https://crates.io/me> and update
the `CARGO_REGISTRY_TOKEN` repository secret.

### `npm publish` fails with "You must be logged in"

Verify that the `NPM_TOKEN` secret is set in the `npm` **environment** (not repository secrets)
and that `release-node.yml` references `environment: npm`.

### PyPI upload fails with "403 Forbidden"

1. Check that the **environment name** in the Trusted Publisher config on PyPI is `pypi`
   (exact match, case-sensitive).
2. Check that the **workflow filename** is `release-python.yml` (exact match).
3. Ensure `id-token: write` permission is present in the workflow.

### Docker push fails with "denied: permission_denied"

Ensure the repository has **Workflow permissions** set to **Read and write**:
**Settings** → **Actions** → **General** → **Workflow permissions** → **Read and write permissions**.

### GHCR image not visible publicly

Follow step 5.1 to change the package visibility to Public after the first push.
