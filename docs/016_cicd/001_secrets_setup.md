# CI/CD Secrets

Verified against `.github/workflows/`.

The repo currently ships six workflows:

- `ci.yml`
- `release-rust.yml`
- `release-node.yml`
- `release-python.yml`
- `release-docker.yml`
- `deploy-site.yml`

## Secrets and environments actually used

- `CARGO_REGISTRY_TOKEN` for `release-rust.yml`
- `NPM_TOKEN` in the `npm` environment for `release-node.yml`
- `pypi` environment for `release-python.yml`
- built-in `GITHUB_TOKEN` for Docker and Pages workflows
- `github-pages` environment for the site deploy job

## Rust release order in the current workflow

```text
+------------------+
| edgecrab-types   |
+------------------+
         |
         v
+------------------+
| edgecrab-security|
+------------------+
         |
         v
+------------------+
| edgecrab-state   |
+------------------+
         |
         v
+------------------+
| edgecrab-tools   |
+------------------+
         |
         v
+------------------+
| edgecrab-cron    |
+------------------+
         |
         v
+------------------+
| edgecrab-core    |
+------------------+
         |
         v
+------------------+
| edgecrab-gateway |
+------------------+
         |
         v
+------------------+
| edgecrab-acp     |
+------------------+
         |
         v
+------------------+
| edgecrab-migrate |
+------------------+
         |
         v
+------------------+
| edgecrab-cli     |
+------------------+
```

The workflow inserts waits between publishes so crates.io has time to index dependencies.

## Site deploy requirements

`deploy-site.yml` uses:

- `pages: write`
- `id-token: write`
- `github-pages` environment

## Practical rule

If you change a release workflow, update this doc only after verifying the exact secret names and job order in the YAML.
