#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/update-homebrew-formula.sh <formula-path> <version> <arm64-sha256> <x86_64-sha256>

Updates raphaelmansuy/homebrew-tap Formula/edgecrab.rb to point at the given
EdgeCrab release and macOS checksums.
EOF
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

FORMULA_PATH="${1:-}"
VERSION="${2:-}"
ARM_SHA="${3:-}"
X86_SHA="${4:-}"

[[ -n "$FORMULA_PATH" && -n "$VERSION" && -n "$ARM_SHA" && -n "$X86_SHA" ]] || {
  usage
  exit 1
}

[[ -f "$FORMULA_PATH" ]] || die "formula not found: $FORMULA_PATH"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "invalid version: $VERSION"
[[ "$ARM_SHA" =~ ^[0-9a-f]{64}$ ]] || die "invalid arm64 sha256: $ARM_SHA"
[[ "$X86_SHA" =~ ^[0-9a-f]{64}$ ]] || die "invalid x86_64 sha256: $X86_SHA"

perl -0pi -e 's/version "[^"]+"/version "'"$VERSION"'"/' "$FORMULA_PATH"
perl -0pi -e 's#(edgecrab/releases/download/v)[^/]+(/edgecrab-aarch64-apple-darwin\.tar\.gz")#${1}'"$VERSION"'${2}#g' "$FORMULA_PATH"
perl -0pi -e 's#(edgecrab/releases/download/v)[^/]+(/edgecrab-x86_64-apple-darwin\.tar\.gz")#${1}'"$VERSION"'${2}#g' "$FORMULA_PATH"
perl -0pi -e 's#(edgecrab-aarch64-apple-darwin\.tar\.gz"\n\s+sha256 ")[^"]+#${1}'"$ARM_SHA"'#' "$FORMULA_PATH"
perl -0pi -e 's#(edgecrab-x86_64-apple-darwin\.tar\.gz"\n\s+sha256 ")[^"]+#${1}'"$X86_SHA"'#' "$FORMULA_PATH"

echo "Updated $FORMULA_PATH to EdgeCrab $VERSION"
