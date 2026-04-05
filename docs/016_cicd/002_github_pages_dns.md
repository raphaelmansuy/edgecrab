# 016.002 — GitHub Pages & DNS for www.edgecrab.com

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 016.001 CI/CD Secrets](./001_secrets_setup.md)
> **Workflow**: `.github/workflows/deploy-site.yml`
> **Domain**: `www.edgecrab.com`
> **Last updated**: 2026-04-05

Covers the complete setup path: DNS records, GitHub Pages configuration, CNAME file, Astro
`site` URL, and the automated deploy pipeline. All steps are idempotent — re-running them is safe.

---

## 1. How it all fits together

```
git push → main
       │
       └─ deploy-site.yml ─▶ pnpm build ─▶ upload artifact
                                                │
                                         actions/deploy-pages
                                                │
                               github.io/raphaelmansuy/edgecrab
                                                │  (301)
                               ┌────────────────┘
                               │  CNAME: www.edgecrab.com
                               │  DNS CNAME: raphaelmansuy.github.io
                               ▼
                        www.edgecrab.com  (HTTPS, auto-cert)
```

The `CNAME` file in `site/public/` is copied verbatim into `site/dist/` by Astro at build time,
so GitHub Pages always knows the custom domain — even after a fresh deploy wipes the repo's
Pages settings.

---

## 2. DNS records (one-time, at your registrar)

### 2.1 `www` subdomain (required)

Add one **CNAME** record:

| Type | Host / Name | Value | TTL |
|------|-------------|-------|-----|
| `CNAME` | `www` | `raphaelmansuy.github.io` | 3600 (or lowest) |

> GitHub Pages resolves `raphaelmansuy.github.io` → the deployed site automatically.  
> Do **not** point `www` directly to a GitHub IP — use the `github.io` CNAME, not an A record.

### 2.2 Apex domain redirect (recommended)

So that bare `edgecrab.com` redirects to `www.edgecrab.com`, add four **A records** pointing
to GitHub Pages' anycast IPs:

| Type | Host / Name | Value |
|------|-------------|-------|
| `A` | `@` | `185.199.108.153` |
| `A` | `@` | `185.199.109.153` |
| `A` | `@` | `185.199.110.153` |
| `A` | `@` | `185.199.111.153` |

And an **AAAA record** for IPv6:

| Type | Host / Name | Value |
|------|-------------|-------|
| `AAAA` | `@` | `2606:50c0:8000::153` |
| `AAAA` | `@` | `2606:50c0:8001::153` |
| `AAAA` | `@` | `2606:50c0:8002::153` |
| `AAAA` | `@` | `2606:50c0:8003::153` |

> GitHub Pages automatically redirects `edgecrab.com` → `www.edgecrab.com` once both the
> `www` CNAME and the apex A/AAAA records resolve.

### 2.3 Verify DNS propagation

```bash
# www CNAME
dig www.edgecrab.com CNAME +short
# expected: raphaelmansuy.github.io.

# apex A records
dig edgecrab.com A +short
# expected: 185.199.108.153  185.199.109.153  185.199.110.153  185.199.111.153
```

DNS changes can take up to 48 hours but usually propagate within minutes.

---

## 3. GitHub Pages — one-time repository settings

1. **Settings** → **Pages** (left sidebar).
2. Under **Build and deployment**:
   - **Source**: `GitHub Actions` ← must be this, not "Deploy from a branch".
3. Under **Custom domain**:
   - Enter `www.edgecrab.com` and click **Save**.
   - GitHub will run a DNS check and show a green tick when DNS is correct.
4. Tick **Enforce HTTPS** (available once the TLS certificate is issued, usually < 10 min).

> The `CNAME` file at `site/public/CNAME` (content: `www.edgecrab.com`) ensures this setting
> persists across every deploy. You do **not** need to re-enter it after each push.

---

## 4. CNAME file — already committed

`site/public/CNAME` already contains:

```
www.edgecrab.com
```

Astro copies everything in `public/` to `dist/` verbatim. The deploy workflow uploads
`site/dist` as the Pages artifact, so the `CNAME` file is always included.

**Do not delete this file.** Without it, GitHub Pages resets the custom domain to
`raphaelmansuy.github.io` after every deploy.

---

## 5. Astro config — already correct

`site/astro.config.mjs` declares:

```js
const siteUrl = 'https://www.edgecrab.com';

export default defineConfig({
  site: siteUrl,
  // ...
});
```

This sets the canonical base URL used by the sitemap, OG tags, and `<link rel="canonical">`.
No changes needed.

---

## 6. Deploy workflow — `deploy-site.yml`

The workflow at `.github/workflows/deploy-site.yml` runs on every push to `main` that touches
`site/**` or the workflow file itself, plus on manual dispatch.

### Required permissions (already set in the file)

```yaml
permissions:
  contents: read
  pages: write       # write to GitHub Pages
  id-token: write    # OIDC token for actions/deploy-pages
```

### Required GitHub environment

The `deploy` job uses:

```yaml
environment:
  name: github-pages
  url: ${{ steps.deployment.outputs.page_url }}
```

GitHub creates the `github-pages` environment automatically the first time Pages is enabled.
No manual setup is required.

### Concurrency guard

```yaml
concurrency:
  group: pages
  cancel-in-progress: true
```

Only one deploy runs at a time. A second push while a deploy is in progress cancels the
in-flight run and starts a fresh one — preventing stale deploys.

---

## 7. End-to-end verification checklist

Run through this after any DNS or Pages change:

- [ ] `dig www.edgecrab.com CNAME +short` → `raphaelmansuy.github.io.`
- [ ] `dig edgecrab.com A +short` → four GitHub Pages IPs
- [ ] GitHub **Settings → Pages → Custom domain** shows `www.edgecrab.com` with a green DNS check
- [ ] **Enforce HTTPS** is enabled
- [ ] Push any change under `site/` to `main`; the **Deploy Site to GitHub Pages** workflow runs green
- [ ] `https://www.edgecrab.com` loads the site with a valid TLS certificate
- [ ] `https://edgecrab.com` redirects to `https://www.edgecrab.com` (HTTP 301)
- [ ] `curl -sI https://www.edgecrab.com | grep -i "content-type"` returns `text/html`

---

## 8. Troubleshooting

### "Domain's DNS record could not be retrieved"

DNS has not propagated yet, or the CNAME record is wrong. Re-check section 2.1 and wait up
to 48 hours. Use `dig` to confirm the record is live before saving in GitHub Pages settings.

### TLS certificate stuck on "Unavailable"

GitHub provisions certificates via Let's Encrypt. It requires both `www` CNAME and apex
A records to be correct. Verify with `dig`, then click **Save** again in Pages settings to
re-trigger the certificate request.

### Site returns 404 after deploy

Ensure **Source** in Pages settings is set to `GitHub Actions`, not "Deploy from a branch".
Check that the workflow completed successfully under **Actions → Deploy Site to GitHub Pages**.

### Custom domain resets to blank after each deploy

The `site/public/CNAME` file is missing or its content changed. Restore it to exactly:

```
www.edgecrab.com
```

with no trailing newline or spaces.

### `pnpm: command not found` in the workflow

The workflow uses `pnpm/action-setup@v4`. Ensure `site/pnpm-lock.yaml` is committed and the
`cache-dependency-path` points to it. Run `pnpm install` locally and commit the lockfile if
missing.
