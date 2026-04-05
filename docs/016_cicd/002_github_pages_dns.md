# GitHub Pages and DNS

Verified against:
- `.github/workflows/deploy-site.yml`
- `site/public/CNAME`
- `site/astro.config.mjs`

The site deploy flow is straightforward:

```text
+-----------------------------+
| push to main touching site/ |
+-----------------------------+
               |
               v
+-----------------------------+
| build Astro site            |
+-----------------------------+
               |
               v
+-----------------------------+
| upload Pages artifact       |
+-----------------------------+
               |
               v
+-----------------------------+
| actions/deploy-pages        |
+-----------------------------+
               |
               v
+-----------------------------+
| custom domain serves site   |
+-----------------------------+
```

## Code-backed facts

- the workflow file is `deploy-site.yml`
- Pages deploys use the `github-pages` environment
- `site/public/CNAME` exists
- the Astro config sets the site URL

## Operational checklist

- keep the `CNAME` file in `site/public/`
- keep Pages permissions in the workflow
- keep the custom domain consistent with the Astro `site` URL
- verify DNS outside the repo before blaming the workflow

This page is intentionally short because most of the real truth lives in the workflow YAML and the site config files.
