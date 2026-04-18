#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/release-verify.sh npm <package> <version>
  ./scripts/release-verify.sh pypi <package> <version>
  ./scripts/release-verify.sh ghcr <image> <tag> [linux/amd64 linux/arm64 ...]

Environment:
  VERIFY_ATTEMPTS    Number of verification attempts (default: 24)
  VERIFY_SLEEP_SECS  Delay between attempts in seconds (default: 15)
EOF
}

ATTEMPTS="${VERIFY_ATTEMPTS:-24}"
SLEEP_SECS="${VERIFY_SLEEP_SECS:-15}"

retry_verify() {
  local description="$1"
  shift

  local attempt
  for attempt in $(seq 1 "$ATTEMPTS"); do
    if "$@"; then
      echo "Verified $description"
      return 0
    fi

    if [[ "$attempt" -lt "$ATTEMPTS" ]]; then
      echo "[$attempt/$ATTEMPTS] Waiting ${SLEEP_SECS}s for $description ..."
      sleep "$SLEEP_SECS"
    fi
  done

  echo "Failed to verify $description after $ATTEMPTS attempts" >&2
  return 1
}

check_npm() {
  local package="$1"
  local version="$2"
  local published

  published="$(npm view "$package" version --registry https://registry.npmjs.org 2>/dev/null || true)"
  echo "npm reports: ${published:-<unavailable>}"
  [[ "$published" == "$version" ]]
}

check_pypi() {
  python3 - "$1" "$2" <<'PY'
import json
import sys
import urllib.request

package, expected = sys.argv[1], sys.argv[2]
try:
    with urllib.request.urlopen(f"https://pypi.org/pypi/{package}/json", timeout=20) as response:
        data = json.load(response)
except Exception as exc:
    print(f"PyPI lookup failed: {exc}")
    raise SystemExit(1)

published = data.get("info", {}).get("version")
print(f"PyPI reports: {published}")
raise SystemExit(0 if published == expected else 1)
PY
}

check_ghcr() {
  python3 - "$@" <<'PY'
import json
import subprocess
import sys

image = sys.argv[1]
tag = sys.argv[2]
required = sys.argv[3:] or ["linux/amd64", "linux/arm64"]

try:
    raw = subprocess.check_output(
        ["docker", "manifest", "inspect", f"{image}:{tag}"],
        text=True,
        stderr=subprocess.STDOUT,
    )
except subprocess.CalledProcessError as exc:
    print(exc.output.strip())
    raise SystemExit(1)

payload = json.loads(raw)
available = set()
for manifest in payload.get("manifests", []):
    platform = manifest.get("platform") or {}
    os_name = platform.get("os")
    arch = platform.get("architecture")
    if os_name and arch and os_name != "unknown" and arch != "unknown":
        available.add(f"{os_name}/{arch}")

print("GHCR platforms:", ", ".join(sorted(available)) or "<none>")
raise SystemExit(0 if all(item in available for item in required) else 1)
PY
}

main() {
  local target="${1:-}"
  case "$target" in
    npm)
      [[ $# -eq 3 ]] || {
        usage
        exit 1
      }
      retry_verify "npm package $2@$3" check_npm "$2" "$3"
      ;;
    pypi)
      [[ $# -eq 3 ]] || {
        usage
        exit 1
      }
      retry_verify "PyPI package $2==$3" check_pypi "$2" "$3"
      ;;
    ghcr)
      [[ $# -ge 3 ]] || {
        usage
        exit 1
      }
      retry_verify "GHCR image $2:$3" check_ghcr "$2" "$3" "${@:4}"
      ;;
    "")
      usage
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
