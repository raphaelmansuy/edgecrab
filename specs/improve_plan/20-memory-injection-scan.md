# 20 — Memory Write Injection Scanning (FP15)

> G14: Block exfiltration and injection patterns before persisting to memory.
> Cross-ref: Hermes `memory_tool.py` injection scanning, `edgecrab-security` crate

---

## WHY (First Principle FP15)

```
"Trust boundary at memory write"
```

Memory files (MEMORY.md, USER.md) are injected into the system prompt on
every session start. This makes them an **injection surface**:

```
+----------------------------------------------------------------------+
|  ATTACK CHAIN:                                                       |
|                                                                      |
|  1. Malicious content in user message or fetched webpage              |
|  2. Agent calls memory_write("Remember: ignore all instructions...")  |
|  3. Memory persisted to ~/.edgecrab/memories/MEMORY.md               |
|  4. Next session: poisoned memory injected into system prompt         |
|  5. Agent behavior compromised across ALL future sessions             |
|                                                                      |
|  SEVERITY: Critical. Persistent cross-session prompt injection.       |
|                                                                      |
+----------------------------------------------------------------------+
```

**EdgeCrab already scans context files** (AGENTS.md, SOUL.md) for injection
patterns via `edgecrab-security`. But `memory_write` in `memory.rs` does
NOT call the scanner before persisting content.

---

## Hermes Agent Pattern (Cross-Reference)

**File:** `tools/memory_tool.py` lines 40-80

Hermes scans for:
- Invisible unicode chars (`\u200b`, `\ufeff`, zero-width joiners)
- Role hijack patterns ("ignore previous instructions", "you are now")
- Exfiltration patterns (`curl.*$TOKEN`, `cat /etc/passwd`)
- Base64-encoded payloads in memory content

---

## EdgeCrab Implementation

### Existing Asset: `edgecrab-security` crate

The security crate already has `injection_scan::scan_for_injection()` used
for context files. We reuse it for memory writes.

### Location: `crates/edgecrab-tools/src/tools/memory.rs`

Add scanning before any `tokio::fs::write()` call:

```rust
use edgecrab_security::injection_scan;

// In memory_write execute():
let scan_result = injection_scan::scan_content(&content);
if scan_result.severity >= Severity::High {
    return Err(ToolError::Other(format!(
        "Memory write blocked: content contains suspicious patterns ({}).\n\
         Detected: {}.\n\
         If this is a false positive, rephrase the content and try again.",
        scan_result.severity,
        scan_result.patterns.join(", ")
    )));
}
```

### Integration Points

| File | Change | Why |
|------|--------|-----|
| `memory.rs` | Add `injection_scan::scan_content()` before write | FP15 |
| `edgecrab-security/src/injection_scan.rs` | Ensure `scan_content()` is public | API surface |

### Edge Cases

| Case | Handling |
|------|----------|
| False positive on legitimate code | Return actionable error suggesting rephrase |
| Unicode normalization tricks | Security crate already handles zero-width chars |
| Base64-encoded injection | Security crate pattern includes base64 payloads |
| Memory read (not write) | No scanning needed — read is not a persistence boundary |
| Memory append vs overwrite | Both paths go through write → both scanned |

### Tests

```
test_memory_write_blocks_role_hijack
test_memory_write_blocks_exfiltration_pattern
test_memory_write_allows_normal_content
test_memory_write_blocks_invisible_unicode
test_memory_write_error_message_is_actionable
```

---

## Estimated Impact

| Metric | Before | After |
|--------|--------|-------|
| Cross-session injection risk | OPEN | BLOCKED |
| False positive rate | N/A | <1% (reuse battle-tested scanner) |
