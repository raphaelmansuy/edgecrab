# 06 — P2: Remove content:null Anti-Pattern

**Priority**: P2
**Impact**: Eliminates empty scaffold creation — the #1 empty-file bug
**Risk**: Low — aligns with Hermes Agent's proven pattern
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-2, [02-hermes-patterns.md](02-hermes-patterns.md) Pattern 2

## WHY

```
CURRENT SCHEMA (content nullable):

    "content": {"type": ["string", "null"], "description": "File content..."}

    LLM path of least resistance:
    write_file(path="main.rs", content=null) -> empty file created
    LLM thinks it made progress -> moves to next task
    File is empty -> broken project

HERMES SCHEMA (content required string):

    "content": {"type": "string", "description": "Complete content to write"}

    LLM must generate content:
    write_file(path="main.rs", content="fn main() {...}") -> real file
    No shortcut available -> forces actual code generation
```

## Implementation

### File: `crates/edgecrab-tools/src/tools/file_write.rs`

#### Change 1: Args struct

```rust
// BEFORE:
#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: Option<String>,  // null = empty scaffold
}

// AFTER:
#[derive(Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,  // required — no null
}
```

#### Change 2: Schema

```rust
// BEFORE:
"content": {
    "type": ["string", "null"],
    "description": "Content to write... Set to null to create an empty scaffold."
}

// AFTER:
"content": {
    "type": "string",
    "description": "Complete content to write to the file. Use empty string for an empty file."
}
```

#### Change 3: Execute function

```rust
// BEFORE:
let content = args.content.unwrap_or_default();

// AFTER:
let content = args.content;
// No unwrap_or_default needed — content is always a String
```

## Edge Cases

1. **Empty files needed**: LLM can still create empty files with `content: ""`
   (empty string). This is intentional — it forces explicit intent.

2. **__init__.py**: LLM writes `content: ""` — correct behavior.

3. **Backward compat**: If an LLM sends `content: null`, serde will return
   InvalidArgs error with our new enhanced error message (see 04-error-guidance.md).
   The error will say: `Required fields: ["path", "content"]`.

4. **Existing empty scaffold tests**: Update to use `content: ""` instead of
   `content: null`.

## Feature Preservation

- Read-before-write guard: PRESERVED (unchanged)
- Freshness guard: PRESERVED (unchanged)
- Mkdir -p behavior: PRESERVED (unchanged)
- Path safety validation: PRESERVED (unchanged)
