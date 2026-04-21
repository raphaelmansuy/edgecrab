# 07 — P3: Terminal Anti-Pattern Guard

**Priority**: P3
**Impact**: Prevents LLM from using terminal as escape hatch for file ops
**Risk**: Low — returns warning + suggestion, does not block execution
**Cross-ref**: [01-diagnosis.md](01-diagnosis.md) RC-6

## WHY

```
CURRENT BEHAVIOR:
    Schema description: "Do not use cat/head/tail"  (prose, ignored)
    LLM calls: terminal(command="python3 -c 'open(f).read()'")
    Result: 10-minute python3 process reading files

    The prose instruction fails because:
    1. LLM has 173 tools to remember — schema desc gets lost
    2. "python3 -c open()" is technically not cat/head/tail
    3. No runtime enforcement — description is advisory only

TARGET BEHAVIOR:
    LLM calls: terminal(command="cat main.rs")
    Result: WARNING prepended to output:
      "Note: For reading files, the `read_file` tool is faster and
       more reliable. Use `terminal` for commands that transform or
       process data, not for simple file I/O."
    File content still returned (not blocked — soft guardrail).
```

## Implementation

### File: `crates/edgecrab-tools/src/tools/terminal.rs`

Add `detect_file_io_antipattern()` function:

```rust
use regex::Regex;
use once_cell::sync::Lazy;

static FILE_READ_PATTERNS: Lazy<Vec<(Regex, &'static str)>> = Lazy::new(|| vec![
    (Regex::new(r"^cat\s+").unwrap(),
     "Use `read_file` instead of `cat` — it handles encoding and large files safely."),
    (Regex::new(r"^head\s+").unwrap(),
     "Use `read_file` with line range instead of `head`."),
    (Regex::new(r"^tail\s+").unwrap(),
     "Use `read_file` with line range instead of `tail`."),
    (Regex::new(r"python3?\s+-c\s+.*open\(").unwrap(),
     "Use `read_file` instead of python file I/O — it's faster and path-safe."),
    (Regex::new(r"^less\s+|^more\s+").unwrap(),
     "Use `read_file` instead of `less`/`more`."),
    (Regex::new(r#"^echo\s+.*>\s*\S+"#).unwrap(),
     "Use `write_file` instead of `echo >` — it creates parent dirs and validates paths."),
    (Regex::new(r"^sed\s+-i").unwrap(),
     "Use `patch` instead of `sed -i` — it has fuzzy matching and creates backups."),
]);

fn detect_file_io_antipattern(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    for (pattern, suggestion) in FILE_READ_PATTERNS.iter() {
        if pattern.is_match(trimmed) {
            return Some(suggestion);
        }
    }
    // Also check piped commands (first segment)
    if let Some(first_cmd) = trimmed.split('|').next() {
        let first_trimmed = first_cmd.trim();
        for (pattern, suggestion) in FILE_READ_PATTERNS.iter() {
            if pattern.is_match(first_trimmed) {
                return Some(suggestion);
            }
        }
    }
    None
}
```

### Integration with execute()

```rust
async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String, ToolError> {
    let args: TerminalArgs = serde_json::from_value(args)?;

    // Soft guardrail: detect file I/O anti-patterns
    let antipattern_warning = detect_file_io_antipattern(&args.command);

    // ... existing security scanning, execution logic ...

    let mut result = execute_command(&args, ctx).await?;

    // Prepend warning if anti-pattern detected
    if let Some(warning) = antipattern_warning {
        result = format!("[NOTE: {}]\n\n{}", warning, result);
    }

    Ok(result)
}
```

## Edge Cases

1. **Piped commands**: `cat file.rs | wc -l` — first segment matches cat.
   Warning is shown but command still runs. The pipeline context makes
   cat legitimate, but the warning is still useful.

2. **Complex python**: `python3 -c "import json; print(json.dumps(...))"` —
   does NOT match because there's no `open(` in the command.

3. **sed for non-file**: `sed 's/foo/bar/'` (no -i flag) — does NOT match.
   Only `sed -i` (in-place file edit) triggers the warning.

4. **echo for env vars**: `echo $PATH` — does NOT match because the regex
   requires `>` redirect.

5. **Legitimate terminal use**: `grep -r "pattern" . | sort | head -5` —
   `head` in a pipeline is legitimate for truncating output, but the
   warning is still acceptable as a soft nudge.

## Feature Preservation

- Command execution: UNCHANGED — all commands still execute
- Security scanning: UNCHANGED — Aho-Corasick patterns still enforced
- Approval system: UNCHANGED — dangerous commands still require approval
- Background processes: UNCHANGED
- Timeout handling: UNCHANGED
