# ==============================================================================
#  EdgeCrab — Makefile
#
#  Quick start:
#    make build          # release binary
#    make test           # run all Rust tests
#    make ci             # full CI gate (fmt + lint + test + SDKs)
#    make test-python    # Python SDK tests
#    make test-node      # Node.js SDK tests
#    make test-sdks      # all SDK tests
#    make site-dev       # start Astro docs site in dev mode
#    make site-build     # build Astro docs site for production
#    make site-preview   # preview the production build locally
#    make site-deploy    # trigger live GitHub Pages deployment
# ==============================================================================

.DEFAULT_GOAL := help
.PHONY: help \
        build build-debug check fmt fmt-check lint test test-lsp ci \
        install uninstall \
        test-python test-node test-sdks \
        publish-rust publish-rust-dry \
        publish-python publish-python-dry \
        publish-node publish-node-dry \
        publish-npm-cli publish-npm-cli-dry \
        publish-pypi-cli publish-pypi-cli-dry \
        publish-all publish-all-dry \
        publish-python-local publish-node-local publish-npm-cli-local publish-pypi-cli-local \
        publish-local \
        version-bump tag-release \
        site-dev site-build site-preview site-install site-deploy site-deploy-status \
        clean clean-all

# ── Colours ────────────────────────────────────────────────────────────────────
BOLD   := \033[1m
GREEN  := \033[0;32m
CYAN   := \033[0;36m
YELLOW := \033[0;33m
RED    := \033[0;31m
DIM    := \033[2m
RESET  := \033[0m

# ── Paths ──────────────────────────────────────────────────────────────────────
BINARY := target/release/edgecrab

# ── Version (read from the canonical release-version helper) ──────────────────
VERSION := $(shell ./scripts/release-version.sh print)

# ── Helper macros ──────────────────────────────────────────────────────────────
define log
  @printf "$(BOLD)$(GREEN) ▶$(RESET) $(GREEN)$(1)$(RESET)\n"
endef
define ok
  @printf "$(BOLD)$(GREEN) ✓$(RESET) $(1)\n"
endef
define warn
  @printf "$(BOLD)$(YELLOW) ⚠$(RESET) $(YELLOW)$(1)$(RESET)\n"
endef
define err
  @printf "$(BOLD)$(RED) ✖$(RESET) $(RED)$(1)$(RESET)\n"
endef

# ══════════════════════════════════════════════════════════════════════════════
#  HELP
# ══════════════════════════════════════════════════════════════════════════════
help: ## Show this help screen
	@printf "\n$(BOLD)EdgeCrab$(RESET) — Rust-native autonomous coding agent\n"
	@printf "$(DIM)https://github.com/raphaelmansuy/edgecrab$(RESET)\n\n"
	@printf "$(BOLD)Usage$(RESET)\n"
	@printf "  make $(CYAN)<target>$(RESET)\n\n"
	@printf "$(BOLD)Targets$(RESET)\n"
	@awk 'BEGIN {FS = ":.*##"; section=""} \
	     /^## / { printf "\n  $(BOLD)%s$(RESET)\n", substr($$0, 4); next } \
	     /^[a-zA-Z_-]+:.*##/ { printf "    $(CYAN)%-26s$(RESET) %s\n", $$1, $$2 }' \
	     $(MAKEFILE_LIST)
	@printf "\n"

# ══════════════════════════════════════════════════════════════════════════════
## Build
# ══════════════════════════════════════════════════════════════════════════════

build: ## Build optimised release binary
	$(call log,cargo build --release)
	@cargo build --release
	$(call ok,Binary ready: $(BINARY))

build-debug: ## Build debug binary
	$(call log,cargo build)
	@cargo build

check: ## Fast compile-check (no binary produced)
	$(call log,cargo check)
	@cargo check

# ══════════════════════════════════════════════════════════════════════════════
## Code quality
# ══════════════════════════════════════════════════════════════════════════════

fmt: ## Auto-format all crates
	$(call log,cargo fmt --all)
	@cargo fmt --all

fmt-check: ## Verify formatting without changes (CI gate)
	$(call log,cargo fmt --all -- --check)
	@cargo fmt --all -- --check

lint: ## Run Clippy — warnings promoted to errors
	$(call log,cargo clippy -- -D warnings)
	@cargo clippy -- -D warnings

test: ## Run all Rust unit and integration tests across the workspace
	$(call log,cargo test --workspace)
	@cargo test --workspace

test-lsp: ## Run the dedicated LSP crate tests and integration coverage
	$(call log,cargo test -p edgecrab-lsp)
	@cargo test -p edgecrab-lsp

ci: fmt-check lint test-lsp test test-sdks ## Full CI gate: fmt → lint → LSP → workspace test → SDK tests
	$(call ok,All CI checks passed)

# ══════════════════════════════════════════════════════════════════════════════
## Install
# ══════════════════════════════════════════════════════════════════════════════

install: build ## Install edgecrab to ~/.cargo/bin
	$(call log,cargo install --path crates/edgecrab-cli)
	@cargo install --path crates/edgecrab-cli
	$(call ok,Installed: $$(which edgecrab))

uninstall: ## Remove edgecrab from ~/.cargo/bin
	$(call warn,Removing edgecrab from ~/.cargo/bin ...)
	@cargo uninstall edgecrab-cli || true

# ══════════════════════════════════════════════════════════════════════════════
## SDK Tests
# ══════════════════════════════════════════════════════════════════════════════

test-python: ## Run Python SDK tests
	$(call log,Python SDK tests)
	@cd sdks/python && pip install -e ".[dev]" -q && pytest tests/ -v

test-node: ## Run Node.js SDK tests
	$(call log,Node.js SDK tests)
	@cd sdks/node && npm ci --silent && npm run build && npm test

test-sdks: test-python test-node ## Run all SDK test suites
	$(call ok,All SDK tests passed)

# ══════════════════════════════════════════════════════════════════════════════
## Publish
# ══════════════════════════════════════════════════════════════════════════════

# ── Rust / crates.io ──────────────────────────────────────────────────────────

# Dry-run every crate in publish order.  Uses --no-verify + --allow-dirty so
# workspace path deps don't cause false failures locally.
publish-rust-dry: ## Dry-run: verify all 11 crates package cleanly
	$(call log,Dry-run: all Rust crates ...)
	@for crate in edgecrab-types edgecrab-security edgecrab-state edgecrab-cron edgecrab-tools edgecrab-lsp edgecrab-core edgecrab-gateway edgecrab-acp edgecrab-migrate edgecrab-cli; do \
	  printf " $(CYAN)→$(RESET) cargo publish -p $$crate --dry-run\n"; \
	  cargo publish -p $$crate --dry-run --allow-dirty --no-verify 2>&1 | grep -v 'Uploading' || true; \
	done
	$(call ok,Rust dry-run passed)

# Publishes all crates in strict topological order.  Between each publish
# we sleep 30 s so crates.io has time to index the crate before the next
# dependent crate is submitted.  Errors due to "already published" are
# treated as non-fatal; all other errors abort immediately.
publish-rust: ## Publish all 11 crates to crates.io (dependency order)
	$(call log,Publishing edgecrab-types ...)
	@OUTPUT=$$(cargo publish -p edgecrab-types 2>&1); STATUS=$$?; \
	 echo "$$OUTPUT"; \
	 if [ $$STATUS -ne 0 ]; then \
	   echo "$$OUTPUT" | grep -q 'already exists' && echo '  (already published — skipping)' || exit 1; \
	 fi
	$(call log,Waiting 30 s for index propagation ...)
	@sleep 30
	$(call log,Publishing remaining crates ...)
	@for crate in edgecrab-security edgecrab-state edgecrab-cron edgecrab-tools edgecrab-lsp edgecrab-core edgecrab-gateway edgecrab-acp edgecrab-migrate edgecrab-cli; do \
	  printf " $(CYAN)→$(RESET) cargo publish -p $$crate --no-verify\n"; \
	  OUTPUT=$$(cargo publish -p $$crate --no-verify 2>&1); STATUS=$$?; \
	  echo "$$OUTPUT"; \
	  if [ $$STATUS -ne 0 ]; then \
	    echo "$$OUTPUT" | grep -q 'already exists' && echo '  (already published — skipping)' || exit 1; \
	  fi; \
	  sleep 30; \
	done
	$(call ok,Rust crates published)

# ── Python / PyPI ─────────────────────────────────────────────────────────────
publish-python-dry: ## Dry-run: build Python sdist and check with twine
	$(call log,Building Python sdist [dry-run])
	@cd sdks/python && python -m build --sdist
	@twine check sdks/python/dist/*
	$(call ok,Python dry-run passed)

publish-python: ## Build and upload Python SDK to PyPI
	$(call log,Building Python wheels ...)
	@cd sdks/python && python -m build
	$(call log,Uploading to PyPI ...)
	@twine upload sdks/python/dist/*
	$(call ok,Python SDK published to PyPI)

publish-python-local: ## Build Python SDK wheel and install it locally with pip
	$(call log,Building Python SDK wheel ...)
	@cd sdks/python && python -m build
	$(call log,Installing Python SDK locally ...)
	@pip install --force-reinstall sdks/python/dist/edgecrab_sdk-*.whl
	$(call ok,Python SDK installed locally)

# ── Node.js / npm ────────────────────────────────────────────────────────────
publish-node-dry: ## Dry-run: build and pack Node.js SDK
	$(call log,Building Node.js SDK [dry-run])
	@cd sdks/node && npm ci && npm run build
	@cd sdks/node && npm pack --dry-run
	$(call ok,Node.js dry-run passed)

publish-node: ## Build and publish Node.js SDK to npm
	$(call log,Building Node.js SDK ...)
	@cd sdks/node && npm ci && npm run build
	$(call log,Publishing to npm ...)
	@cd sdks/node && npm publish --access public
	$(call ok,Node.js SDK published to npm)

publish-node-local: ## Build Node.js SDK, pack it, and install it locally via npm link
	$(call log,Building Node.js SDK ...)
	@cd sdks/node && npm ci && npm run build
	$(call log,Linking Node.js SDK locally ...)
	@cd sdks/node && npm link --force
	$(call ok,Node.js SDK linked locally — use 'npm link edgecrab-sdk' in your project)

# ── npm CLI (edgecrab-cli wrapper) ───────────────────────────────────────────
publish-npm-cli-dry: ## Dry-run: pack npm CLI wrapper package
	$(call log,npm CLI dry-run pack)
	@cd sdks/npm-cli && npm pack --dry-run
	$(call ok,npm CLI dry-run passed)

publish-npm-cli: ## Publish npm CLI wrapper to npm registry
	$(call log,Publishing npm CLI wrapper ...)
	@cd sdks/npm-cli && npm publish --access public
	$(call ok,npm CLI published to npm)

publish-npm-cli-local: ## Pack npm CLI wrapper and install it globally via npm link
	$(call log,Linking npm CLI wrapper locally ...)
	@cd sdks/npm-cli && npm link
	$(call ok,npm CLI linked globally — 'edgecrab' command now available)

# ── PyPI CLI (edgecrab-cli wrapper) ──────────────────────────────────────────
publish-pypi-cli-dry: ## Dry-run: build PyPI CLI wrapper and check with twine
	$(call log,Building PyPI CLI wrapper [dry-run])
	@cd sdks/pypi-cli && python -m build --sdist
	@twine check sdks/pypi-cli/dist/*
	$(call ok,PyPI CLI dry-run passed)

publish-pypi-cli: ## Build and upload PyPI CLI wrapper to PyPI
	$(call log,Building PyPI CLI wrapper ...)
	@cd sdks/pypi-cli && python -m build
	$(call log,Uploading to PyPI ...)
	@twine upload sdks/pypi-cli/dist/*
	$(call ok,PyPI CLI published)

publish-pypi-cli-local: ## Build PyPI CLI wrapper wheel and install it locally with pip
	$(call log,Building PyPI CLI wrapper wheel ...)
	@cd sdks/pypi-cli && python -m build
	$(call log,Installing PyPI CLI wrapper locally ...)
	@pip install --force-reinstall sdks/pypi-cli/dist/edgecrab_cli-*.whl
	$(call ok,PyPI CLI installed locally)

# ── Publish all ───────────────────────────────────────────────────────────────

# Dry-run preflight for every package — run before tagging a release.
publish-all-dry: publish-rust-dry publish-python-dry publish-node-dry publish-npm-cli-dry publish-pypi-cli-dry ## Dry-run all packages (preflight before tagging)
	$(call ok,All dry-runs passed — safe to tag and release)

publish-all: publish-rust publish-python publish-node publish-npm-cli publish-pypi-cli ## Publish all packages (crates.io + PyPI + npm)
	$(call ok,All packages published)

publish-local: publish-python-local publish-node-local publish-npm-cli-local publish-pypi-cli-local ## Build and install/link all Python + npm packages locally (no registry)
	$(call ok,All packages published locally)

# ── Version bump ──────────────────────────────────────────────────────────────

# Bump the canonical release version and sync every derived manifest.
# Usage: make version-bump VERSION=0.2.0
version-bump: ## Bump version in all manifests. Usage: make version-bump VERSION=0.2.0
	@[ -n "$(VERSION)" ] || (printf "$(RED)ERROR: VERSION is required. Example: make version-bump VERSION=0.2.0$(RESET)\n"; exit 1)
	$(call log,Setting canonical release version to $(VERSION) ...)
	@./scripts/release-version.sh set "$(VERSION)"
	@./scripts/release-version.sh check
	$(call ok,Version bumped to $(VERSION))
	@printf "$(DIM)  Next: git add -A && git commit -m 'chore: bump version to $(VERSION)' && make tag-release VERSION=$(VERSION)$(RESET)\n"

# ── Tag release ───────────────────────────────────────────────────────────────

# Create and push an annotated release tag.  This triggers all release-*.yml
# workflows on GitHub Actions (crates.io + npm + PyPI + Docker + binaries).
# Run `make publish-all-dry` first as a preflight check.
# Usage: make tag-release VERSION=0.2.0
tag-release: ## Create and push release tag. Usage: make tag-release VERSION=0.2.0
	@[ -n "$(VERSION)" ] || (printf "$(RED)ERROR: VERSION is required. Example: make tag-release VERSION=0.2.0$(RESET)\n"; exit 1)
	$(call log,Creating annotated tag v$(VERSION) ...)
	@git tag -a "v$(VERSION)" -m "Release v$(VERSION)"
	$(call log,Pushing tag v$(VERSION) to origin ...)
	@git push origin "v$(VERSION)"
	$(call ok,Tag v$(VERSION) pushed — CI will publish all packages)
	@printf "$(DIM)  Monitor: gh run list --limit 10$(RESET)\n"

# ══════════════════════════════════════════════════════════════════════════════
## Documentation Site (Astro)
# ══════════════════════════════════════════════════════════════════════════════

SITE_DIR := site

site-install: ## Install Astro site dependencies
	$(call log,Installing site dependencies ...)
	@cd $(SITE_DIR) && pnpm install
	$(call ok,Site dependencies installed)

site-dev: ## Start Astro docs site in dev mode (hot-reload)
	$(call log,Starting Astro dev server → http://localhost:4321)
	@cd $(SITE_DIR) && pnpm dev

site-build: ## Build Astro docs site for production (output: site/dist/)
	$(call log,Building Astro site for production ...)
	@cd $(SITE_DIR) && pnpm build
	$(call ok,Site built → $(SITE_DIR)/dist/)

site-preview: site-build ## Build then preview the production site locally
	$(call log,Previewing production build → http://localhost:4321)
	@cd $(SITE_DIR) && pnpm preview

site-deploy: ## Trigger GitHub Pages deployment via workflow_dispatch (pushes live to www.edgecrab.com)
	$(call log,Triggering GitHub Pages deployment ...)
	@GH_PAGER='' gh workflow run deploy-site.yml --ref main
	$(call ok,Deployment triggered — monitor at: https://github.com/raphaelmansuy/edgecrab/actions/workflows/deploy-site.yml)

site-deploy-status: ## Check the latest GitHub Pages deployment status
	$(call log,Checking latest deployment status ...)
	@GH_PAGER='' gh run list --workflow=deploy-site.yml --limit 3

# ══════════════════════════════════════════════════════════════════════════════
## Clean
# ══════════════════════════════════════════════════════════════════════════════

clean: ## Remove Rust build artifacts
	$(call log,cargo clean)
	@cargo clean

clean-all: clean ## Remove all build artifacts (Rust + SDKs + site)
	@rm -rf sdks/node/dist sdks/node/node_modules
	@rm -rf sdks/python/dist sdks/python/*.egg-info
	@rm -rf sdks/npm-cli/bin/edgecrab sdks/npm-cli/bin/edgecrab.exe
	@rm -rf sdks/pypi-cli/dist sdks/pypi-cli/*.egg-info sdks/pypi-cli/edgecrab_cli/_bin
	@rm -rf $(SITE_DIR)/dist $(SITE_DIR)/.astro
	$(call ok,All build artifacts removed)
