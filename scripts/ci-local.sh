#!/usr/bin/env bash
# ci-local.sh — Run the same offline CI gates used by .github/workflows/ci.yml
#
# Usage:
#   ./scripts/ci-local.sh              # quality + rust tests + offline gates
#   ./scripts/ci-local.sh quality      # fmt + clippy + version check
#   ./scripts/ci-local.sh test         # workspace tests (excl. lsp then lsp)
#   ./scripts/ci-local.sh gates        # harness + MCP mock e2e gates
#   ./scripts/ci-local.sh audit        # cargo audit
#   ./scripts/ci-local.sh site         # docs site build
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "${GREEN}✅ $1${NC}"; }
fail() { echo -e "${RED}❌ $1${NC}"; exit 1; }
step() { echo -e "\n${BOLD}${YELLOW}── $1 ──${NC}"; }

run_quality() {
  step "Version consistency"
  ./scripts/release-version.sh check && pass "release-version check OK"

  step "Rustfmt"
  cargo fmt --all --check && pass "Formatting OK" || fail "Formatting failed — run: cargo fmt --all"

  step "Clippy"
  cargo clippy --workspace --all-targets -- -D warnings && pass "Clippy OK"
}

run_test() {
  step "edgecrab-lsp tests"
  cargo test -p edgecrab-lsp --all-targets && pass "lsp OK"

  step "Workspace tests (exclude lsp, single-threaded for EDGECRAB_HOME isolation)"
  # Match release verification: parallel threads race on EDGECRAB_HOME / skills hub state.
  cargo test --workspace --exclude edgecrab-lsp -- --test-threads=1 && pass "Workspace tests OK"
}

run_gates() {
  step "Offline harness + MCP mock gates"
  bash scripts/check-no-flaky-heuristics.sh
  cargo test -p edgecrab-core --test harness_nonflaky_e2e
  cargo test -p edgecrab-cli --test mcp_agent_visibility_e2e
  cargo test -p edgecrab-cli --test mcp_oauth_discover_e2e
  cargo test -p edgecrab-cli --test mcp_register_e2e
  pass "Offline gates OK"
}

run_audit() {
  step "Security audit"
  if ! command -v cargo-audit >/dev/null 2>&1; then
    cargo install cargo-audit --locked
  fi
  cargo audit && pass "Audit OK"
}

run_site() {
  step "Site build"
  if ! command -v pnpm >/dev/null 2>&1; then
    fail "pnpm not found — install pnpm to run the site job locally"
  fi
  (cd site && pnpm install --frozen-lockfile && pnpm run build) && pass "Site OK"
}

MODE="${1:-all}"

case "$MODE" in
  quality) run_quality ;;
  test)    run_test ;;
  gates)   run_gates ;;
  audit)   run_audit ;;
  site)    run_site ;;
  all)
    run_quality
    run_test
    run_gates
    echo -e "\n${GREEN}${BOLD}✅ Local CI gates passed (quality + tests + offline gates)${NC}"
    ;;
  *)
    echo "Unknown target: $MODE"
    echo "Usage: $0 [all|quality|test|gates|audit|site]"
    exit 1
    ;;
esac
