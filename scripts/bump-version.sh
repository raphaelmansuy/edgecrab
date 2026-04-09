#!/usr/bin/env bash
# scripts/bump-version.sh — Local release helper
#
# Usage:
#   ./scripts/bump-version.sh 0.2.0          # bump + commit + tag + push
#   ./scripts/bump-version.sh 0.2.0 --dry-run # preview changes only
#
# This is the local equivalent of the "Release — Coordinator" GitHub Actions
# workflow (.github/workflows/release.yml).  Both do identical file edits.

set -euo pipefail

# ── Args ──────────────────────────────────────────────────────────────────────
VERSION="${1:-}"
DRY_RUN=false
[[ "${2:-}" == "--dry-run" ]] && DRY_RUN=true

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <MAJOR.MINOR.PATCH> [--dry-run]"
  exit 1
fi

if ! echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "ERROR: version must be MAJOR.MINOR.PATCH (e.g. 0.2.0), got: $VERSION"
  exit 1
fi

# ── Must run from repo root ────────────────────────────────────────────────────
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

# ── Guard: tag must not already exist ─────────────────────────────────────────
if git rev-parse "v$VERSION" >/dev/null 2>&1; then
  echo "ERROR: tag v$VERSION already exists"
  exit 1
fi

# ── Guard: clean working tree ─────────────────────────────────────────────────
if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: working tree is dirty — commit or stash changes first"
  git status --short
  exit 1
fi

echo "==> Bumping all versions to $VERSION"

./scripts/release-version.sh set "$VERSION"

# ── Summary ────────────────────────────────────────────────────────────────────
echo ""
echo "=== Changed files ==="
git diff --stat

if [[ "$DRY_RUN" == "true" ]]; then
  echo ""
  echo "==> DRY RUN — reverting (nothing committed/pushed)"
  git checkout -- .
  exit 0
fi

# ── Commit + tag + push ────────────────────────────────────────────────────────
echo ""
echo "==> Creating commit and tag v$VERSION"

git add \
  Cargo.toml \
  sdks/node/package.json \
  sdks/npm-cli/package.json \
  sdks/pypi-cli/edgecrab_cli/_version.py \
  sdks/python/pyproject.toml \
  sdks/python/edgecrab/_version.py

git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"

echo ""
echo "==> Done! Tag v$VERSION pushed."
echo "    GitHub Actions will now build and publish:"
echo "      • Native binaries (5 targets)"
echo "      • Docker image (linux/amd64 + linux/arm64, no QEMU)"
echo "      • npm package (edgecrab-cli@$VERSION)"
echo "      • PyPI package (edgecrab-cli==$VERSION)"
echo "      • Rust crate (crates.io)"
echo ""
echo "    Monitor at: https://github.com/raphaelmansuy/edgecrab/actions"
