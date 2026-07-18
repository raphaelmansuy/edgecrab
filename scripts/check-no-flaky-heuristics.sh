#!/usr/bin/env bash
# Fail CI if banned flaky-heuristic patterns reappear in harness controllers (018 F1–F4).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

fail=0
ban() {
  local label="$1"
  local pattern="$2"
  shift 2
  if command -v rg >/dev/null 2>&1; then
    if rg -n --glob '!**/tests/**' -e "$pattern" "$@" 2>/dev/null; then
      echo "FAIL [$label]: banned pattern /$pattern/ in $*"
      fail=1
    fi
  else
    if grep -RInE --exclude-dir=target "$pattern" "$@" 2>/dev/null; then
      echo "FAIL [$label]: banned pattern /$pattern/ in $*"
      fail=1
    fi
  fi
}

ban "approval_prose_fn" 'has_approval_marker' \
  crates/edgecrab-core/src/completion_assessor.rs

ban "default_port_8000" 'unwrap_or\(8000\)' \
  crates/edgecrab-tools/src/tools/terminal.rs \
  crates/edgecrab-tools/src/dev_server.rs

ban "invent_port_8000" 'return Some\(8000\)' \
  crates/edgecrab-tools/src/dev_server.rs

ban "guardrail_error_substring" 'contains\("\\"error\\""\)' \
  crates/edgecrab-tools/src/tool_loop_guardrails.rs

ban "guardrail_failed_substring" 'contains\("\\"failed\\""\)' \
  crates/edgecrab-tools/src/tool_loop_guardrails.rs

ban "loader_prose_assess" 'fn loader_only|has_approval_marker' \
  crates/edgecrab-core/src/completion_assessor.rs

ban "success_false_storm" 'success":false' \
  crates/edgecrab-core/src/harness_advisory.rs

ban "serving_http_gate" 'contains\("serving http"\)' \
  crates/edgecrab-tools/src/dev_server.rs \
  crates/edgecrab-tools/src/tools/terminal.rs

if [[ "$fail" -ne 0 ]]; then
  echo "check-no-flaky-heuristics: FAILED"
  exit 1
fi
echo "check-no-flaky-heuristics: OK"
