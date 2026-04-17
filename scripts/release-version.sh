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
  perl -0pi -e 's/^(edgecrab-(?:command-catalog|types|security|state|plugins|cron|lsp|tools|core|gateway|acp|migrate|sdk-core|sdk-macros|sdk) = \{ path = "(?:crates|sdks)\/[^"]+", version = )"[^"]+"/${1}"'"$version"'"/mg' \
    Cargo.toml
}

sync_versions() {
  local version="$1"

  perl -0pi -e 's/"version": "[^"]+"/"version": "'"$version"'"/' \
    sdks/nodejs-native/package.json

  if [[ -f sdks/nodejs-native/package-lock.json ]]; then
    perl -0pi -e 's/^(\s*"version":\s*)"[^"]+"/${1}"'"$version"'"/m; s/("packages":\s*\{\s*"":\s*\{\s*"name":\s*"edgecrab",\s*"version":\s*)"[^"]+"/${1}"'"$version"'"/s' \
      sdks/nodejs-native/package-lock.json
  fi

  if [[ -f sdks/nodejs-native/index.js ]]; then
    perl -0pi -e "s/(bindingPackageVersion !== )'[^']+'/\${1}'$version'/g; s/(expected )[0-9]+\.[0-9]+\.[0-9]+( but got \\$\{bindingPackageVersion\})/\${1}$version\${2}/g" \
      sdks/nodejs-native/index.js
  fi

  perl -0pi -e 's/"version": "[^"]+"/"version": "'"$version"'"/' \
    sdks/npm-cli/package.json

  if [[ -f sdks/wasm/package.json ]]; then
    perl -0pi -e 's/"version": "[^"]+"/"version": "'"$version"'"/' \
      sdks/wasm/package.json
  fi

  printf '__version__ = "%s"\n' "$version" > sdks/pypi-cli/edgecrab_cli/_version.py

  perl -0pi -e 's/^(version = )"[^"]+"/${1}"'"$version"'"/m' sdks/python/pyproject.toml
}

read_npm_version() {
  sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/npm-cli/package.json
}

read_node_sdk_version() {
  sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/nodejs-native/package.json
}

read_node_sdk_lock_version() {
  sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/nodejs-native/package-lock.json | head -n 1
}

read_pypi_cli_version() {
  sed -n 's/^__version__ = "\([^"]*\)"$/\1/p' sdks/pypi-cli/edgecrab_cli/_version.py
}

read_python_sdk_version() {
  sed -n 's/^version = "\([^"]*\)"$/\1/p' sdks/python/pyproject.toml | head -n 1
}

read_wasm_sdk_version() {
  if [[ -f sdks/wasm/package.json ]]; then
    sed -n 's/^[[:space:]]*"version": "\([^"]*\)",$/\1/p' sdks/wasm/package.json
  fi
}

read_node_sdk_runtime_version() {
  if [[ -f sdks/nodejs-native/index.js ]]; then
    sed -n 's/.*expected \([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\) but got.*/\1/p' sdks/nodejs-native/index.js | head -n 1
  fi
}

check_synced() {
  local version="$1"
  local failed=0

  local dependency_versions
  dependency_versions="$(
    awk '
      /^\[workspace\.dependencies\]/ { in_section=1; next }
      /^\[/ { in_section=0 }
      in_section && /^edgecrab-(command-catalog|types|security|state|plugins|cron|lsp|tools|core|gateway|acp|migrate|sdk-core|sdk-macros|sdk) = \{ path = "(crates|sdks)\// {
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
    echo "Version drift: sdks/nodejs-native/package.json is $node_sdk_version, expected $version" >&2
    failed=1
  fi

  if [[ -f sdks/nodejs-native/package-lock.json ]]; then
    local node_sdk_lock_version
    node_sdk_lock_version="$(read_node_sdk_lock_version)"
    if [[ "$node_sdk_lock_version" != "$version" ]]; then
      echo "Version drift: sdks/nodejs-native/package-lock.json is $node_sdk_lock_version, expected $version" >&2
      failed=1
    fi
  fi

  if [[ -f sdks/nodejs-native/index.js ]]; then
    local node_sdk_runtime_version
    node_sdk_runtime_version="$(read_node_sdk_runtime_version)"
    if [[ "$node_sdk_runtime_version" != "$version" ]]; then
      echo "Version drift: sdks/nodejs-native/index.js expects $node_sdk_runtime_version, expected $version" >&2
      failed=1
    fi
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
    echo "Version drift: sdks/python/pyproject.toml is $python_sdk_version, expected $version" >&2
    failed=1
  fi

  if [[ -f sdks/wasm/package.json ]]; then
    local wasm_sdk_version
    wasm_sdk_version="$(read_wasm_sdk_version)"
    if [[ "$wasm_sdk_version" != "$version" ]]; then
      echo "Version drift: sdks/wasm/package.json is $wasm_sdk_version, expected $version" >&2
      failed=1
    fi
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
