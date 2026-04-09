# 🦀 CI/CD Secrets Setup

> **WHY**: A Rust workspace publishing 11 crates to crates.io, two SDK packages (npm + PyPI), one Docker image, and a documentation site needs disciplined secret hygiene — the wrong token in the wrong workflow is a supply-chain incident.

**Source**: `.github/workflows/`

---

## Workflow Inventory

| File | Trigger | Purpose |
|---|---|---|
| `ci.yml` | Push / PR | Build, test, clippy, fmt check |
| `release-binaries.yml` | Tag push (`v*`) | Build native binaries, upload checksums, publish GitHub Release |
| `release-rust.yml` | Tag push (`v*`) | Publish all 11 crates to crates.io in dependency order |
| `release-node.yml` | Tag push (`v*`) | Publish npm package (JS/TS SDK) |
| `release-python.yml` | Tag push (`v*`) | Publish Python SDK to PyPI |
| `release-npm-cli.yml` | `release-binaries.yml` completed successfully | Publish `edgecrab-cli` npm wrapper after binaries are public |
| `release-pypi-cli.yml` | `release-binaries.yml` completed successfully | Publish `edgecrab-cli` PyPI wrapper after binaries are public |
| `release-docker.yml` | Tag push (`v*`) | Build and push Docker image to GHCR |
| `deploy-site.yml` | Push to `main` touching `site/` | Build Astro docs site → GitHub Pages |

---

## Secrets and Environments Per Workflow

```
ci.yml
  └── (no secrets — uses GITHUB_TOKEN read-only implicitly)

release-rust.yml
  └── CARGO_REGISTRY_TOKEN     (repo secret)

release-binaries.yml
  └── GITHUB_TOKEN             (built-in — contents:write for release upload/publish)

release-node.yml
  └── environment: npm
      └── NPM_TOKEN             (environment secret — npm environment)

release-python.yml
  └── environment: pypi
      └── (OIDC trusted publishing — no long-lived token needed)

release-npm-cli.yml
  └── environment: npm
      └── NPM_TOKEN             (environment secret — npm environment)

release-pypi-cli.yml
  └── environment: pypi
      └── (OIDC trusted publishing — no long-lived token needed)

release-docker.yml
  └── GITHUB_TOKEN              (built-in — write:packages permission)

deploy-site.yml
  └── environment: github-pages
      └── GITHUB_TOKEN          (built-in — pages:write + id-token:write)
```

> **Tip**: Use GitHub **environment** secrets (not repo secrets) for `npm` and `pypi`. Environment protection rules add a required-reviewer gate so no workflow can publish without approval.

---

## Rust Crate Publish Order

`release-rust.yml` publishes crates in strict dependency order with waits between publishes so crates.io has time to index each crate before the next one depends on it:

```
edgecrab-types
      │
      ▼
edgecrab-security
      │
      ▼
edgecrab-state
      │
      ▼
edgecrab-cron
      │
      ▼
edgecrab-tools
      │
      ▼
edgecrab-lsp
      │
      ▼
edgecrab-core
      │
      ▼
edgecrab-gateway
      │
      ▼
edgecrab-acp
      │
      ▼
edgecrab-migrate
      │
      ▼
edgecrab-cli          ← published last; depends on everything
```

This order matches the DAG in [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md). If you add a new crate, insert it at the correct position in this chain.

---

## Setting Up Secrets (New Repo)

### `CARGO_REGISTRY_TOKEN`

1. Log in to [crates.io](https://crates.io)
2. Account Settings → API Tokens → New Token (scope: `publish-new` + `publish-update`)
3. GitHub repo → Settings → Secrets and variables → Actions → New repository secret
4. Name: `CARGO_REGISTRY_TOKEN`, value: paste token

### `NPM_TOKEN`

1. Log in to [npmjs.com](https://npmjs.com)
2. Access Tokens → Generate New Token → Automation (for CI)
3. GitHub repo → Settings → Environments → Create `npm` environment
4. Add environment secret: `NPM_TOKEN`
5. (Recommended) Add required reviewer to the `npm` environment

### PyPI OIDC Trusted Publishing (no token needed)

1. Log in to [pypi.org](https://pypi.org)
2. Project → Publishing → Add a new publisher → GitHub Actions
3. Fill in: repository owner, repository name, workflow filename (`release-python.yml`), environment name (`pypi`)
4. GitHub repo → Settings → Environments → Create `pypi` environment
5. No secret needed — PyPI mints a short-lived token via OIDC

### Docker / GHCR

Uses the built-in `GITHUB_TOKEN` with `packages: write` permission. No setup required beyond the permission declaration in the workflow YAML:

```yaml
permissions:
  contents: read
  packages: write
```

---

## `ci.yml` — Key Checks

```
push / PR
     │
     ├── cargo fmt --check
     ├── cargo clippy -- -D warnings
     ├── cargo test --workspace
     └── cargo build --workspace --release
```

All four gates must pass before a PR can merge. The `release-*` workflows only trigger on version tags, so a broken build never reaches the publish step.

---

## Tips

- **Never put `CARGO_REGISTRY_TOKEN` in an environment** — it gives publish rights to every crate. Keep it as a repo-level secret and restrict it to the `release-rust.yml` workflow with `if: github.ref_type == 'tag'`.
- **The inter-publish waits are load-bearing** — crates.io has eventual consistency. If you remove the `sleep` steps, downstream crates will fail to resolve the freshly published dependency.
- **Tag format matters** — the release workflows use `v*` glob matching. A tag named `release-1.0` will not trigger them.

---

## FAQ

**Q: How do I do a dry-run publish?**
A: `cargo publish --dry-run -p edgecrab-types` locally. The CI workflow does not support dry-run mode.

**Q: What if a crate publish fails mid-chain?**
A: The workflow is not transactional. Fix the failure and re-run the workflow from the failed step. `cargo publish` is idempotent for the same version — it will skip already-published crates with a warning.

**Q: Can I publish a single crate without the full chain?**
A: Locally, yes. In CI, the `release-rust.yml` workflow always runs the full chain to keep versions in sync across all crates.

---

## Cross-References

- Crate dependency order (why this publish order) → [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md)
- GitHub Pages deploy → [`016_cicd/002_github_pages_dns.md`](002_github_pages_dns.md)
