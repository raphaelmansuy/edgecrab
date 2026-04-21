# 19 — Dynamic Schema Cross-Reference Filtering (FP14)

> G13: Strip references to unavailable tools from schema descriptions.
> Cross-ref: Hermes `get_tool_definitions()` post-filter

---

## WHY (First Principle FP14)

```
"Schema must reflect reality"
```

When a tool's description says "prefer web_search for general queries" but
`web_search` is disabled (missing API key, toolset excluded), the model
sees a reference to a tool that doesn't exist. It will:

1. Attempt to call `web_search` → fails → wastes a turn
2. Retry the same call → burns budget
3. Eventually give up or hallucinate a workaround

**The fix is simple**: After filtering tools by availability, scan remaining
schemas for references to tools NOT in the filtered set. Strip those refs.

---

## Hermes Agent Pattern (Cross-Reference)

**File:** `model_tools.py` lines 510-570

```python
# Post-filter: strip cross-references to unavailable tools
for tool_def in filtered_tools:
    if tool_def["name"] == "browser_navigate":
        # Remove "prefer web_search" if web_search not available
        if "web_search" not in available_names:
            tool_def["description"] = tool_def["description"].replace(
                "prefer web_search for general queries", ""
            )
    if tool_def["name"] == "execute_code":
        # Strip sandbox tool names that aren't available
        ...
```

---

## EdgeCrab Implementation

### Location: `crates/edgecrab-tools/src/registry.rs`

Add a post-filter pass in `get_definitions()`:

```rust
/// After filtering tools by toolset/availability, strip schema description
/// references to tools not in the final set.
fn strip_unavailable_cross_refs(
    definitions: &mut [ToolSchema],
    available_names: &HashSet<&str>,
);
```

### Cross-Reference Map (Static)

Define known cross-references as a compile-time map:

```rust
/// Tools that reference other tools in their descriptions.
/// Key = tool name, Value = list of (referenced_tool, pattern_to_strip)
const TOOL_CROSS_REFS: &[(&str, &str, &str)] = &[
    ("browser_navigate", "web_search", "prefer web_search for general queries"),
    ("browser_navigate", "web_extract", "use web_extract for content extraction"),
    ("execute_code", "terminal", "use terminal for system commands"),
    ("write_file", "patch", "use patch for partial edits"),
];
```

### Integration

| File | Change | Why |
|------|--------|-----|
| `registry.rs` | Add `strip_unavailable_cross_refs()` after `get_definitions()` filter | FP14 |
| `registry.rs` | Add `TOOL_CROSS_REFS` static | DRY cross-ref map |

### Edge Cases

| Case | Handling |
|------|----------|
| No cross-refs to strip | No-op, zero cost |
| Tool re-enabled mid-session | Schemas rebuild on next API call → refs restored |
| MCP tool references core tool | MCP schemas are pass-through, not filtered |
| Pattern appears in user content | Only strip from schema descriptions, not user messages |

### Tests

```
test_strip_cross_refs_removes_unavailable_tool_mentions
test_strip_cross_refs_preserves_available_tool_mentions
test_strip_cross_refs_handles_empty_definitions
test_strip_cross_refs_handles_no_cross_refs
```

---

## Estimated Impact

| Metric | Before | After |
|--------|--------|-------|
| Hallucinated tool calls per session | ~1-3 | ~0 |
| Wasted turns on unavailable tools | ~2-5 | ~0 |
