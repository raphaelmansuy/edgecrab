# Homebrew Tap Automated Update Setup

EdgeCrab uses a GitHub Actions workflow to automatically update the
[raphaelmansuy/homebrew-tap](https://github.com/raphaelmansuy/homebrew-tap)
`Formula/edgecrab.rb` file every time a new release is published.

Three authentication methods are supported in preference order:

| Option | Secret(s) required | Properties |
|--------|-------------------|------------|
| **A — SSH deploy key** | `HOMEBREW_TAP_DEPLOY_KEY` | Narrowest scope, no expiry, already configured |
| **B — GitHub App** | `HOMEBREW_TAP_APP_ID` + `HOMEBREW_TAP_APP_PRIVATE_KEY` | Short-lived token, no long-lived secret |
| **C — Fine-grained PAT** | `HOMEBREW_TAP_PUSH_TOKEN` | Simplest, but is a long-lived credential |

**Option A (deploy key) is already configured** — see below for how it was set up.

---

## Option A — SSH Deploy Key (recommended, already active)

A GitHub App generates a short-lived installation token (~1 hour) for each
workflow run. No token is ever stored in a secret beyond the App's private key,
which is a standard operations credential rather than a user credential.

### How it was set up

A deploy key was created and configured using the `gh` CLI:

```bash
# 1. Generate an ed25519 keypair (no passphrase)
ssh-keygen -t ed25519 -C "edgecrab-ci-homebrewtap" -f /tmp/edgecrab_tap_deploy -N ""

# 2. Install the public key as a write-access deploy key on the tap repo
gh api -X POST /repos/raphaelmansuy/homebrew-tap/keys \
  -f title="edgecrab-ci-homebrewtap" \
  -f key="$(cat /tmp/edgecrab_tap_deploy.pub)" \
  -F read_only=false

# 3. Store the private key as a secret in edgecrab
gh secret set HOMEBREW_TAP_DEPLOY_KEY \
  --body "$(cat /tmp/edgecrab_tap_deploy)" \
  -R raphaelmansuy/edgecrab

# 4. Remove temporary key files (private key is now only in GitHub secrets)
rm -f /tmp/edgecrab_tap_deploy /tmp/edgecrab_tap_deploy.pub
```

The deploy key (id `148386829`) is visible at:
<https://github.com/raphaelmansuy/homebrew-tap/settings/keys>

The secret is visible (name only) at:
<https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>

### To rotate the deploy key

Run the 4 commands above again, then delete the old key from the tap repo:

```bash
# List key IDs
gh api /repos/raphaelmansuy/homebrew-tap/keys --jq '.[] | [.id,.title] | @tsv'
# Delete old key
gh api -X DELETE /repos/raphaelmansuy/homebrew-tap/keys/<OLD_ID>
```

---

## Option B — GitHub App (no long-lived token)

1. Go to <https://github.com/settings/apps/new> (personal account) or
   Settings → Developer Settings → GitHub Apps for an organisation.
2. Set:
   - **App name**: `edgecrab-tap-bot` (or any name you prefer)
   - **Homepage URL**: `https://github.com/raphaelmansuy/edgecrab`
   - **Webhook**: uncheck **Active** (no webhook needed)
3. Under **Repository permissions**, set **Contents** → **Read and write**.
4. Under **Where can this GitHub App be installed?** choose **Only on this account**.
5. Click **Create GitHub App**.

### 2. Install the App on the tap repository

1. In the App settings page, click **Install App**.
2. Choose your account → select **Only select repositories** → choose
   `raphaelmansuy/homebrew-tap`.
3. Click **Install**.

### 3. Generate a private key

1. In the App settings page, scroll to **Private keys**.
2. Click **Generate a private key**. A `.pem` file is downloaded.
3. Keep this file safe — it is the only copy.

### 4. Add secrets to the `edgecrab` repository

In <https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>:

| Secret name | Value |
|-------------|-------|
| `HOMEBREW_TAP_APP_ID` | The numeric App ID shown on the App settings page (e.g. `123456`) |
| `HOMEBREW_TAP_APP_PRIVATE_KEY` | The full contents of the downloaded `.pem` file |

Paste the entire PEM file (including `-----BEGIN RSA PRIVATE KEY-----` header
and footer) as the secret value.

### 5. Verify

Trigger a manual run of the **Release — Homebrew Tap** workflow via
<https://github.com/raphaelmansuy/edgecrab/actions/workflows/release-homebrew-tap.yml>.
The **Generate GitHub App token** step should succeed and the commit should
appear in [raphaelmansuy/homebrew-tap](https://github.com/raphaelmansuy/homebrew-tap/commits/master).

---

## Option B — Fine-grained Personal Access Token (fallback)

Use this if you prefer not to create a GitHub App.

### 1. Create a fine-grained PAT

1. Go to <https://github.com/settings/tokens?type=beta>.
2. Click **Generate new token**.
3. Set:
   - **Token name**: `edgecrab-homebrewtap-push`
   - **Expiration**: Set a reminder — fine-grained PATs expire after at most 1 year.
   - **Resource owner**: your personal account
   - **Repository access**: **Only select repositories** → `homebrew-tap`
   - **Permissions**: **Contents** → **Read and write**
4. Click **Generate token** and copy the value.

### 2. Add the secret

In <https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>:

| Secret name | Value |
|-------------|-------|
| `HOMEBREW_TAP_PUSH_TOKEN` | The fine-grained PAT you just created |

---

## Precedence

The workflow checks for credentials in this order:

1. `HOMEBREW_TAP_DEPLOY_KEY` → SSH deploy key (preferred, already configured)
2. `HOMEBREW_TAP_APP_ID` + `HOMEBREW_TAP_APP_PRIVATE_KEY` → GitHub App token
3. `HOMEBREW_TAP_PUSH_TOKEN` → fine-grained PAT
4. None configured → job logs a notice and exits cleanly (release continues)

---

## Why the release does not fail when Homebrew is not configured

Homebrew is a convenience distribution channel; it is not a gate for the
release. If no push credential is configured the job emits a GitHub Actions
notice annotation and exits with code 0 so the release pipeline is green. You
can configure the credential at any time and then manually re-trigger the
workflow.

---

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `Generate GitHub App token` step fails with `Not found` | App is not installed on `homebrew-tap` | Re-install the App on the tap repo (step 2 above) |
| `Generate GitHub App token` step fails with `Bad credentials` | Wrong private key or mismatched App ID | Regenerate the private key and re-add both secrets |
| `Commit and push` fails with 403 | PAT does not have write access or is expired | Check the token expiry and repository permission scope |
| Formula already up to date | No-op; the correct version is already in the tap | Nothing to fix |
