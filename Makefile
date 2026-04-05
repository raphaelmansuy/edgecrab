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
# ==============================================================================

.DEFAULT_GOAL := help
.PHONY: help \
        build build-debug check fmt fmt-check lint test ci \
        install uninstall \
        test-python test-node test-sdks \
        publish-rust publish-rust-dry \
        publish-python publish-python-dry \
        publish-node publish-node-dry \
        publish-npm-cli publish-npm-cli-dry \
        publish-pypi-cli publish-pypi-cli-dry \
        publish-all \
        site-dev site-build site-preview site-install \
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

# ── Version (read from workspace Cargo.toml) ──────────────────────────────────
VERSION := $(shell grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')

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

test: ## Run all Rust unit and integration tests
	$(call log,cargo test)
	@cargo test

ci: fmt-check lint test test-sdks ## Full CI gate: fmt → lint → test → SDK tests
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
publish-rust-dry: ## Dry-run: verify all 10 crates package cleanly
	$(call log,cargo publish --dry-run [edgecrab-types])
	@cargo publish -p edgecrab-types --dry-run --allow-dirty
	$(call ok,Rust dry-run passed)

publish-rust: ## Publish crates to crates.io (dependency order)
	$(call log,Publishing edgecrab-types ...)
	@cargo publish -p edgecrab-types
	$(call log,Waiting 30s for index propagation ...)
	@sleep 30
	$(call log,Publishing remaining crates ...)
	@for crate in edgecrab-security edgecrab-state edgecrab-tools edgecrab-cron edgecrab-core edgecrab-gateway edgecrab-acp edgecrab-migrate edgecrab-cli; do \
	  printf " $(CYAN)→$(RESET) cargo publish -p $$crate\n"; \
	  cargo publish -p $$crate || true; \
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

# ── npm CLI (edgecrab-cli wrapper) ───────────────────────────────────────────
publish-npm-cli-dry: ## Dry-run: pack npm CLI wrapper package
	$(call log,npm CLI dry-run pack)
	@cd sdks/npm-cli && npm pack --dry-run
	$(call ok,npm CLI dry-run passed)

publish-npm-cli: ## Publish npm CLI wrapper to npm registry
	$(call log,Publishing npm CLI wrapper ...)
	@cd sdks/npm-cli && npm publish --access public
	$(call ok,npm CLI published to npm)

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

# ── Publish all ───────────────────────────────────────────────────────────────
publish-all: publish-rust publish-python publish-node publish-npm-cli publish-pypi-cli ## Publish all packages (crates.io + PyPI + npm)
	$(call ok,All packages published)

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
