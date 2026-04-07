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

# ── Rust workspace ─────────────────────────────────────────────────────────────
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" Cargo.toml
rm -f Cargo.toml.bak

# ── npm CLI wrapper ────────────────────────────────────────────────────────────
sed -i.bak "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" \
  sdks/npm-cli/package.json
rm -f sdks/npm-cli/package.json.bak

sed -i.bak "s/const BINARY_VERSION = '[^']*'/const BINARY_VERSION = '$VERSION'/" \
  sdks/npm-cli/scripts/install.js
rm -f sdks/npm-cli/scripts/install.js.bak

# ── PyPI CLI wrapper ───────────────────────────────────────────────────────────
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" \
  sdks/pypi-cli/pyproject.toml
rm -f sdks/pypi-cli/pyproject.toml.bak

printf '__version__ = "%s"\n' "$VERSION" > \
  sdks/pypi-cli/edgecrab_cli/_version.py

sed -i.bak "s/^BINARY_VERSION = \".*\"/BINARY_VERSION = \"$VERSION\"/" \
  sdks/pypi-cli/edgecrab_cli/_binary.py
rm -f sdks/pypi-cli/edgecrab_cli/_binary.py.bak

# ── Python SDK ─────────────────────────────────────────────────────────────────
sed -i.bak "s/^version = \".*\"/version = \"$VERSION\"/" \
  sdks/python/pyproject.toml
rm -f sdks/python/pyproject.toml.bak

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
  sdks/npm-cli/package.json \
  sdks/npm-cli/scripts/install.js \
  sdks/pypi-cli/pyproject.toml \
  sdks/pypi-cli/edgecrab_cli/_version.py \
  sdks/pypi-cli/edgecrab_cli/_binary.py \
  sdks/python/pyproject.toml

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
