# 🦀 GitHub Pages and DNS

> **WHY**: Documentation that lives in the repo deploys automatically on every merge to `main` — no manual uploads, no stale hosted copy, no divergence between code and docs.

**Source**: `.github/workflows/deploy-site.yml`, `site/public/CNAME`, `site/astro.config.mjs`

---

## Deploy Flow

```
push to main
(touches site/)
      │
      ▼
┌─────────────────────┐
│  deploy-site.yml    │  GitHub Actions workflow
│  triggered          │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  pnpm install       │  install Astro deps
│  pnpm build         │  output → site/dist/
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  actions/upload-    │  package site/dist/ as
│  pages-artifact     │  Pages artifact
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  actions/deploy-    │  deploy to github-pages
│  pages              │  environment
└──────────┬──────────┘
           │
           ▼
  custom domain serves site
  (CNAME → GitHub Pages CDN)
```

---

## Workflow Permissions

```yaml
# deploy-site.yml
permissions:
  contents: read
  pages: write        # required to upload Pages artifact
  id-token: write     # required for OIDC-based Pages deployment
```

The `github-pages` environment must exist in the repo settings before the first deploy. GitHub creates it automatically on the first successful Pages deployment via Actions if the repo has Pages enabled.

---

## Files That Must Stay in Sync

| File | Purpose | What breaks if wrong |
|---|---|---|
| `site/public/CNAME` | Tells GitHub Pages the custom domain | Pages reverts to `<org>.github.io/<repo>` URL |
| `site/astro.config.mjs` → `site` field | Astro uses this for path generation | Internal links break if hostname doesn't match CNAME |
| DNS → CNAME record | Points custom domain to GitHub CDN | Site unreachable on custom domain |

---

## DNS Setup

GitHub Pages requires one of:

```
# Apex domain (example.com)
@ → 185.199.108.153
@ → 185.199.109.153
@ → 185.199.110.153
@ → 185.199.111.153

# Subdomain (docs.example.com)
docs → CNAME → <org>.github.io
```

After updating DNS:
1. Repo → Settings → Pages → verify the custom domain
2. Enable "Enforce HTTPS" (available after DNS propagates)

> **Tip**: DNS propagation can take up to 48 hours. The `deploy-site.yml` workflow will succeed even if DNS is still propagating — the CNAME in `site/public/` is what matters for GitHub's side.

---

## Astro Configuration Checklist

```js
// site/astro.config.mjs  — minimum required fields
export default defineConfig({
  site: 'https://your-custom-domain.com',  // must match CNAME
  output: 'static',
});
```

If `site` is wrong, Astro generates incorrect canonical URLs and the sitemap points to the wrong domain.

---

## Operational Checklist

| Task | Owner |
|---|---|
| `site/public/CNAME` matches actual custom domain | Repo maintainer |
| DNS CNAME/A records point to GitHub Pages IPs | DNS admin |
| `github-pages` environment exists in repo settings | Repo admin |
| Workflow has `pages: write` + `id-token: write` permissions | Checked in YAML |
| `site/astro.config.mjs` `site` field matches CNAME | Developer |

---

## FAQ

**Q: The workflow succeeds but the site shows old content.**
A: GitHub Pages CDN has a short cache. Wait 2–3 minutes and hard-refresh. If still stale, check that the artifact upload step uploaded the correct `dist/` directory.

**Q: I get a "Page build failed" error.**
A: This usually means the `github-pages` environment doesn't exist or Pages is not enabled for the repo. Go to Settings → Pages → enable "GitHub Actions" as the source.

**Q: Can I preview the site locally before pushing?**
A: `cd site && pnpm dev` — Astro starts a local dev server. Does not require the custom domain to be configured.

**Q: How do I add a new docs page to the site?**
A: Add a `.md` or `.astro` file to `site/src/content/` (or `site/src/pages/`). The next push to `main` touching `site/` triggers a redeploy automatically.

---

## Cross-References

- CI/CD secrets for `github-pages` environment → [`016_cicd/001_secrets_setup.md`](001_secrets_setup.md)
- Documentation index → [`INDEX.md`](../INDEX.md)
