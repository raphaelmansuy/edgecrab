#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/release-version.sh print
  ./scripts/release-version.sh sync
  ./scripts/release-version.sh set <MAJOR.MINOR.PATCH>
  ./scripts/release-version.sh check

Commands:
  print   Print the canonical workspace release version.
  sync    Sync derived package versions from Cargo.toml.
  set     Update Cargo.toml, then sync all derived package versions.
  check   Fail if any derived package version has drifted.
EOF
}

die() {
  echo "ERROR: $*" >&2
  exit 1
}

require_repo_root() {
  REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
  cd "$REPO_ROOT"
}

validate_version() {
  local version="$1"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || die "version must be MAJOR.MINOR.PATCH, got: $version"
}

workspace_version() {
  awk '
    /^\[workspace\.package\]/ { in_section=1; next }
    /^\[/ { in_section=0 }
    in_section && /^[[:space:]]*version = "/ {
      gsub(/^[^"]*"/, "", $0)
      gsub(/".*$/, "", $0)
      print
      exit
    }
  ' Cargo.toml
}

set_workspace_version() {
  local version="$1"
  perl -0pi -e 's/(\[workspace\.package\]\n(?:[^\[]*\n)*?version = )"[^"]+"/${1}"'"$version"'"/m' Cargo.toml
}

set_workspace_dependency_versions() {
  local version="$1"
  perl -0pi -e 's/^(edgecrab-(?:types|security|state|cron|lsp|tools|core|gateway|acp|migrate) = \{ path = "crates\/[^"]+", version = )"[^"]+"/${1}"'"$version"'"/mg' \
    Cargo.toml
}

sync_versions() {
  local version="$1"

  perl -0pi -e 's/"version": "[^"]+"/"version": "'"$version"'"/' \
    sdks/node/package.json

  perl -0pi -e 's/"version": "[^"]+"/"version": "'"$version"'"/' \
    sdks/npm-cli/package.json

  printf '__version__ = "%s"\n' "$version" > sdks/pypi-cli/edgecrab_cli/_version.py

  printf '__version__ = "%s"\n' "$version" > sdks/python/edgecrab/_version.py
}

read_npm_version() {
  sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/npm-cli/package.json
}

read_node_sdk_version() {
  sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/node/package.json
}

read_pypi_cli_version() {
  sed -n 's/^__version__ = "\([^"]*\)"$/\1/p' sdks/pypi-cli/edgecrab_cli/_version.py
}

read_python_sdk_version() {
  sed -n 's/^__version__ = "\([^"]*\)"$/\1/p' sdks/python/edgecrab/_version.py
}

check_synced() {
  local version="$1"
  local failed=0

  local dependency_versions
  dependency_versions="$(
    awk '
      /^\[workspace\.dependencies\]/ { in_section=1; next }
      /^\[/ { in_section=0 }
      in_section && /^edgecrab-(types|security|state|cron|lsp|tools|core|gateway|acp|migrate) = \{ path = "crates\// {
        line=$0
        sub(/^.*version = "/, "", line)
        sub(/".*$/, "", line)
        print line
      }
    ' Cargo.toml | sort -u
  )"
  if [[ "$dependency_versions" != "$version" ]]; then
    echo "Version drift: workspace internal dependency versions do not all match $version" >&2
    printf '%s\n' "$dependency_versions" >&2
    failed=1
  fi

  local node_sdk_version
  node_sdk_version="$(read_node_sdk_version)"
  if [[ "$node_sdk_version" != "$version" ]]; then
    echo "Version drift: sdks/node/package.json is $node_sdk_version, expected $version" >&2
    failed=1
  fi

  local npm_version
  npm_version="$(read_npm_version)"
  if [[ "$npm_version" != "$version" ]]; then
    echo "Version drift: sdks/npm-cli/package.json is $npm_version, expected $version" >&2
    failed=1
  fi

  local pypi_cli_version
  pypi_cli_version="$(read_pypi_cli_version)"
  if [[ "$pypi_cli_version" != "$version" ]]; then
    echo "Version drift: sdks/pypi-cli/edgecrab_cli/_version.py is $pypi_cli_version, expected $version" >&2
    failed=1
  fi

  local python_sdk_version
  python_sdk_version="$(read_python_sdk_version)"
  if [[ "$python_sdk_version" != "$version" ]]; then
    echo "Version drift: sdks/python/edgecrab/_version.py is $python_sdk_version, expected $version" >&2
    failed=1
  fi

  if [[ "$failed" -ne 0 ]]; then
    echo "Run ./scripts/release-version.sh sync" >&2
    exit 1
  fi
}

main() {
  require_repo_root

  local command="${1:-}"
  case "$command" in
    print)
      workspace_version
      ;;
    sync)
      local version
      version="$(workspace_version)"
      [[ -n "$version" ]] || die "failed to read workspace version from Cargo.toml"
      set_workspace_dependency_versions "$version"
      sync_versions "$version"
      echo "Synced derived package versions to $version"
      ;;
    set)
      local version="${2:-}"
      [[ -n "$version" ]] || die "missing version"
      validate_version "$version"
      set_workspace_version "$version"
      set_workspace_dependency_versions "$version"
      sync_versions "$version"
      echo "Set workspace and derived package versions to $version"
      ;;
    check)
      local version
      version="$(workspace_version)"
      [[ -n "$version" ]] || die "failed to read workspace version from Cargo.toml"
      check_synced "$version"
      echo "All derived package versions match $version"
      ;;
    *)
      usage
      [[ -n "$command" ]] && exit 1
      exit 0
      ;;
  esac
}

main "$@"
