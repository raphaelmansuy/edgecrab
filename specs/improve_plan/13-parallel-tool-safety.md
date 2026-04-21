# Spec 13: Parallel Tool Path-Overlap Detection

**Priority:** P2 — Medium
**Crate:** `edgecrab-tools` (registry.rs)
**Cross-ref:** [09-assessment-round2.md](09-assessment-round2.md) Gap 4

## Problem

EdgeCrab uses a binary `is_parallel_safe()` flag per tool type. Tools like
`read_file` are parallel-safe (two reads never conflict). But `write_file`
is marked sequential — even when writing to different files.

Hermes Agent's `_should_parallelize_tool_batch()` uses path-scoped overlap
detection: two `write_file` calls targeting different paths run in parallel,
same path → sequential.

```
+---------------------------------------------------------------+
|                   CURRENT (TOO CONSERVATIVE)                  |
+---------------------------------------------------------------+
|                                                               |
|  write_file("a.txt") + write_file("b.txt")                   |
|       |                                                       |
|       v                                                       |
|  Both marked is_parallel_safe=false → sequential              |
|  Time: T(a) + T(b)  (unnecessarily slow)                     |
|                                                               |
+---------------------------------------------------------------+

+---------------------------------------------------------------+
|                   FIXED (PATH-AWARE)                          |
+---------------------------------------------------------------+
|                                                               |
|  write_file("a.txt") + write_file("b.txt")                   |
|       |                                                       |
|       v                                                       |
|  Different paths → parallel! Time: max(T(a), T(b))           |
|                                                               |
|  write_file("a.txt") + write_file("a.txt")                   |
|       |                                                       |
|       v                                                       |
|  Same path → sequential (safe)                                |
|                                                               |
+---------------------------------------------------------------+
```

## Design

Add a new trait method to `ToolHandler`:

```rust
fn path_arguments(&self) -> &[&str] {
    &[]  // default: no path args → use is_parallel_safe()
}
```

Tools that touch files override this to return the argument names containing
file paths (e.g., `&["file_path"]` for write_file, `&["path"]` for patch).

The `process_response` function extracts path values from args, and only
runs tools in parallel if no two tools share a common path.

## Implementation

**File:** `crates/edgecrab-tools/src/registry.rs`
**Change:** Add `fn path_arguments(&self) -> &[&str]` to `ToolHandler` trait.

**File:** `crates/edgecrab-core/src/conversation.rs`
**Change:** In `process_response`, replace the simple `is_parallel_safe()`
check with path-overlap detection:

```rust
fn can_parallelize_batch(
    tools: &[(String, String, String)],  // (id, name, args_json)
    registry: &ToolRegistry,
) -> Vec<bool> {
    let mut path_claims: HashMap<String, usize> = HashMap::new();
    let mut results = vec![true; tools.len()];
    
    for (i, (_, name, args_json)) in tools.iter().enumerate() {
        if !registry.is_parallel_safe(name) {
            // Check if this tool has path arguments
            let path_args = registry.path_arguments(name);
            if path_args.is_empty() {
                results[i] = false;  // no path args → must be sequential
                continue;
            }
            // Extract paths from args
            if let Ok(args) = serde_json::from_str::<Value>(args_json) {
                for pa in path_args {
                    if let Some(path) = args.get(pa).and_then(Value::as_str) {
                        let entry = path_claims.entry(path.to_string()).or_insert(0);
                        *entry += 1;
                        if *entry > 1 {
                            results[i] = false;  // path conflict
                        }
                    }
                }
            }
        }
    }
    results
}
```

## Tools That Need `path_arguments()`

| Tool | Path Args | Currently parallel_safe |
|------|-----------|------------------------|
| `write_file` | `["file_path"]` | false → stays false but path-aware |
| `patch` | `["file_path"]` | false → stays false but path-aware |
| `read_file` | `["file_path"]` | true → unchanged |

## Tests

1. Two `write_file` to different paths → both parallel
2. Two `write_file` to same path → sequential
3. `write_file` + `read_file` same path → write sequential, read parallel
4. Non-path tools → fall back to `is_parallel_safe()` flag

## SOLID

- **OCP:** New path-aware tools just implement `path_arguments()`.
- **LSP:** Default impl returns `&[]` — existing tools unchanged.
