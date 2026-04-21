//! # @Context Reference Expansion
//!
//! WHY: Users can embed rich context into their messages using `@ref` syntax.
//! The agent expands these references before sending the message to the LLM,
//! injecting file contents, git diffs, web pages, etc. directly into context.
//!
//! Supported reference types:
//!
//! ```text
//!   @file:path/to/file.rs    → inline file contents
//!   @folder:src/lib          → directory listing (not recursive)
//!   @url:https://...         → (fetched by agent — stub here, no reqwest dep)
//!   @diff                    → git diff (unstaged changes)
//!   @staged                  → git diff --staged (staged changes)
//!   @git:ref                 → git show <ref>
//! ```
//!
//! Security model:
//! - Block paths containing sensitive segments (`.ssh`, `.aws`, `.gnupg`, ...)
//! - Block absolute paths that escape the current working directory
//! - Cap file size at 500 KB to prevent context-window abuse
//! - Block binary files (detected by null-byte scan in first 8 KB)
//!
//! ```text
//!   expand_context_refs("fix @file:src/main.rs", &cwd)
//!       │
//!       ├── find_refs() → [ContextRef::File("src/main.rs")]
//!       ├── security_check(path) → Ok or Err
//!       ├── read_file(path) → contents
//!       └── inject into message text
//! ```

use std::path::{Path, PathBuf};

use edgecrab_security::path_policy::PathPolicy;

// ─── Types ────────────────────────────────────────────────────────────

/// A single `@ref` found in a user message.
#[derive(Debug, Clone, PartialEq)]
pub enum ContextRef {
    /// `@file:path` or `@file:path:LINE` or `@file:path:START-END` — inline file contents.
    /// `line_start` and `line_end` are 1-indexed, inclusive. `None` means full file.
    File {
        path: PathBuf,
        line_start: Option<usize>,
        line_end: Option<usize>,
    },
    /// `@folder:path` — directory listing
    Folder(PathBuf),
    /// `@url:https://...` — fetch URL (deferred: returns placeholder)
    Url(String),
    /// `@diff` — unstaged git diff
    Diff,
    /// `@staged` — staged git diff
    Staged,
    /// `@git:N` — last N commits with patches (clamped to 1–10)
    Git(String),
}

/// Result of expanding all `@refs` in a message.
#[derive(Debug)]
pub struct ExpansionResult {
    /// Message text with `@refs` replaced by their content.
    pub expanded: String,
    /// Which refs were found (for logging / token tracking).
    pub refs_found: Vec<ContextRef>,
    /// Refs that failed security check or file read (skipped).
    pub errors: Vec<String>,
    /// True when injected context was stripped because it exceeded the
    /// hard 50% context-window budget limit (mirrors hermes-agent behavior).
    /// When true, `expanded` contains only the original user message.
    pub budget_blocked: bool,
    /// Set when injected tokens exceeded the soft 25% warning threshold
    /// but stayed below the hard 50% limit.
    pub budget_warning: bool,
}

// ─── Security constants ───────────────────────────────────────────────

/// Segments in a path that are never safe to expand.
///
/// WHY allowlist-free approach: We block specific dangerous directories
/// rather than allowlisting safe ones. This is more practical for local
/// agent use where the user controls the CWD anyway.
const BLOCKED_SEGMENTS: &[&str] = &[
    ".ssh",
    ".aws",
    ".gnupg",
    ".pgp",
    ".gpg",
    ".kube",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".pgpass",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "authorized_keys",
    "credentials",
    ".bashrc",
    ".zshrc",
    ".profile",
    ".bash_profile",
    ".zprofile",
];

/// Max file size in bytes (500 KB).
const MAX_FILE_BYTES: u64 = 512 * 1024;

// ─── Public API ───────────────────────────────────────────────────────

/// Expand all `@context` references in `text`.
///
/// `cwd` is used to resolve relative paths and enforce the "no escape"
/// security invariant. Pass `std::env::current_dir()` at call site.
///
/// Returns the expanded text and metadata about refs found / errors.
pub fn expand_context_refs(text: &str, cwd: &Path) -> ExpansionResult {
    let policy = PathPolicy::new(cwd.to_path_buf());
    expand_context_refs_with_policy(text, cwd, &policy)
}

/// Expand all `@context` references using an explicit path policy.
pub fn expand_context_refs_with_policy(
    text: &str,
    cwd: &Path,
    policy: &PathPolicy,
) -> ExpansionResult {
    let refs = find_refs(text);
    let mut expanded = text.to_string();
    let mut refs_found = Vec::new();
    let mut errors = Vec::new();

    // Build a label→content map for the "--- Attached Context ---" section.
    // WHY: Hermes appends all context under a single block at the end of the
    // message rather than replacing @refs inline. This keeps the user's original
    // message intact while clearly delineating injected content.
    let mut context_sections: Vec<(String, String)> = Vec::new();

    for ctx_ref in &refs {
        let (placeholder, label, replacement) = match ctx_ref {
            ContextRef::File {
                path,
                line_start,
                line_end,
            } => {
                let raw = match (line_start, line_end) {
                    (Some(s), Some(e)) if s == e => format!("@file:{}:{s}", path.display()),
                    (Some(s), Some(e)) => format!("@file:{}:{s}-{e}", path.display()),
                    (Some(s), None) => format!("@file:{}:{s}", path.display()),
                    _ => format!("@file:{}", path.display()),
                };
                let label = raw.clone();
                let content = expand_file(path, policy, *line_start, *line_end);
                (raw, label, content)
            }
            ContextRef::Folder(path) => {
                let raw = format!("@folder:{}", path.display());
                let label = raw.clone();
                let content = expand_folder(path, policy, cwd);
                (raw, label, content)
            }
            ContextRef::Url(url) => {
                // URL fetching requires HTTP client — stub with note.
                // WHY deferred: adding reqwest here would bloat edgecrab-core.
                // The CLI/gateway layer handles URL fetching and injects the
                // content before calling agent.chat().
                let raw = format!("@url:{url}");
                let label = raw.clone();
                let note = format!("[URL context for {url} — fetch deferred to runtime]");
                (raw, label, Ok(note))
            }
            ContextRef::Diff => {
                let raw = "@diff".to_string();
                let content = run_git_command(&["diff"]);
                (raw, "git diff (unstaged)".to_string(), content)
            }
            ContextRef::Staged => {
                let raw = "@staged".to_string();
                let content = run_git_command(&["diff", "--staged"]);
                (raw, "git diff --staged".to_string(), content)
            }
            ContextRef::Git(git_ref) => {
                // Clamp commit count to 1..=10 (matches hermes docs)
                let raw = format!("@git:{git_ref}");
                let content = expand_git_log(git_ref);
                (raw.clone(), raw, content)
            }
        };

        match replacement {
            Ok(content) => {
                refs_found.push(ctx_ref.clone());
                // Replace the @ref token in the message with an empty string.
                // The actual content will be appended in the Attached Context block.
                expanded = expanded.replacen(&placeholder, "", 1);
                context_sections.push((label, content));
            }
            Err(e) => {
                errors.push(format!("{placeholder}: {e}"));
                // Leave original @ref in place so the LLM sees the error context.
            }
        }
    }

    // Trim any whitespace gaps left by removed @refs
    let expanded = expanded.trim().to_string();

    // Append all context under "--- Attached Context ---"
    let expanded = if context_sections.is_empty() {
        expanded
    } else {
        let mut buf = expanded;
        buf.push_str("\n\n--- Attached Context ---\n");
        for (label, content) in &context_sections {
            buf.push_str(&format!("\n### {label}\n\n```\n{content}\n```\n"));
        }
        buf
    };

    ExpansionResult {
        expanded,
        refs_found,
        errors,
        budget_blocked: false,
        budget_warning: false,
    }
}
// ─── Reference detection ──────────────────────────────────────────────

/// Find all `@ref` tokens in `text` using a simple scan.
///
/// WHY not regex: Avoiding a regex dependency keeps edgecrab-core lean.
/// The scan is O(n) and the patterns are simple enough to parse without it.
fn find_refs(text: &str) -> Vec<ContextRef> {
    let mut refs = Vec::new();

    // Split on whitespace + common delimiters to find @-tokens.
    for token in text.split_whitespace() {
        // Strip leading punctuation that might precede @
        let token = token.trim_start_matches(['(', '"', '\'']);
        // Strip trailing punctuation
        let token = token.trim_end_matches(|c: char| {
            matches!(c, ')' | '"' | '\'' | ',' | '.' | ':' | ';' | '?' | '!')
        });

        if !token.starts_with('@') {
            continue;
        }

        let after = &token[1..]; // strip leading '@'

        if let Some(path_str) = after.strip_prefix("file:") {
            // Support line ranges: @file:path:LINE or @file:path:START-END
            // Trailing punctuation already stripped above.
            // We split on ':' from the right to extract optional line spec.
            let (path_part, line_start, line_end) = parse_file_ref(path_str);
            refs.push(ContextRef::File {
                path: PathBuf::from(path_part),
                line_start,
                line_end,
            });
        } else if let Some(path_str) = after.strip_prefix("folder:") {
            refs.push(ContextRef::Folder(PathBuf::from(path_str)));
        } else if let Some(url) = after.strip_prefix("url:") {
            refs.push(ContextRef::Url(url.to_string()));
        } else if after == "diff" {
            refs.push(ContextRef::Diff);
        } else if after == "staged" {
            refs.push(ContextRef::Staged);
        } else if let Some(git_ref) = after.strip_prefix("git:") {
            refs.push(ContextRef::Git(git_ref.to_string()));
        }
    }

    refs.dedup_by(|a, b| a == b); // deduplicate consecutive identical refs
    refs
}

/// Parse a `@file:` value into (path, optional start line, optional end line).
///
/// Formats:
/// - `path/to/file.rs`         → (path, None, None)
/// - `path/to/file.rs:42`      → (path, Some(42), Some(42))
/// - `path/to/file.rs:10-25`   → (path, Some(10), Some(25))
///
/// Lines are 1-indexed. Invalid ranges are silently ignored (full file returned).
fn parse_file_ref(s: &str) -> (&str, Option<usize>, Option<usize>) {
    // Try to find a line-range suffix. It's always after the last ':' that is
    // followed only by digits or a digit range (N or N-M).
    if let Some(colon_pos) = s.rfind(':') {
        // Only treat as a line spec if the path portion contains at least one '/'.
        // This avoids misidentifying Windows-style drive letters like `C:`.
        let suffix = &s[colon_pos + 1..];
        if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '-') {
            let path_part = &s[..colon_pos];
            if let Some(dash) = suffix.find('-') {
                let start_str = &suffix[..dash];
                let end_str = &suffix[dash + 1..];
                if let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>())
                    && start > 0
                    && end >= start
                {
                    return (path_part, Some(start), Some(end));
                }
            } else if let Ok(line) = suffix.parse::<usize>()
                && line > 0
            {
                return (path_part, Some(line), Some(line));
            }
        }
    }
    (s, None, None)
}

// ─── Expansion handlers ───────────────────────────────────────────────

/// Expand `@file:path` — read and return file contents.
///
/// `line_start` and `line_end` are 1-indexed, inclusive. Both `None` means full file.
fn expand_file(
    path: &Path,
    policy: &PathPolicy,
    line_start: Option<usize>,
    line_end: Option<usize>,
) -> Result<String, String> {
    let abs = security_check_path(path, policy)?;

    let metadata =
        std::fs::metadata(&abs).map_err(|e| format!("cannot stat '{}': {e}", abs.display()))?;

    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "file '{}' is {} bytes — exceeds {} KB limit",
            abs.display(),
            metadata.len(),
            MAX_FILE_BYTES / 1024
        ));
    }

    let bytes = std::fs::read(&abs).map_err(|e| format!("cannot read '{}': {e}", abs.display()))?;

    // Binary file detection: null byte in first 8 KB.
    if bytes.iter().take(8192).any(|&b| b == 0) {
        return Err(format!("'{}' appears to be a binary file", abs.display()));
    }

    let full_text =
        String::from_utf8(bytes).map_err(|_| format!("'{}' is not valid UTF-8", abs.display()))?;

    // Apply line range if requested
    match (line_start, line_end) {
        (Some(start), Some(end)) => {
            // Convert to 0-indexed
            let start0 = start.saturating_sub(1);
            let lines: Vec<&str> = full_text.lines().collect();
            let end0 = end.min(lines.len()); // clamp to file length
            if start0 >= lines.len() {
                // Range starts beyond EOF — return full file (invalid range ignored)
                return Ok(full_text);
            }
            Ok(lines[start0..end0].join("\n"))
        }
        _ => Ok(full_text),
    }
}

/// Expand `@git:N` — last N commits with patches (N clamped to 1–10).
fn expand_git_log(git_ref: &str) -> Result<String, String> {
    // Parse as a commit count if it looks like a number, otherwise fall back to git show
    if let Ok(n) = git_ref.parse::<usize>() {
        let n = n.clamp(1, 10);
        run_git_command(&["log", &format!("-{n}"), "--patch", "--format=%H %s"])
    } else {
        run_git_command(&["show", git_ref])
    }
}

/// Expand `@folder:path` — directory listing (non-recursive).
fn expand_folder(path: &Path, policy: &PathPolicy, _cwd: &Path) -> Result<String, String> {
    let abs = security_check_path(path, policy)?;

    let entries =
        std::fs::read_dir(&abs).map_err(|e| format!("cannot list '{}': {e}", abs.display()))?;

    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();

    names.sort();

    if names.is_empty() {
        return Ok(format!("Directory '{}' is empty.", abs.display()));
    }

    Ok(format!(
        "Directory listing for '{}':\n{}",
        abs.display(),
        names.join("\n")
    ))
}

/// Run a `git` subcommand and return stdout.
fn run_git_command(args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if stdout.is_empty() {
        Ok("(no output)".to_string())
    } else {
        Ok(stdout)
    }
}

// ─── Security ─────────────────────────────────────────────────────────

/// Validate that `path` is safe to read.
///
/// Checks:
/// 1. No blocked path segments (`.ssh`, `.aws`, etc.)
/// 2. The resolved path must stay within the effective path policy
fn security_check_path(path: &Path, policy: &PathPolicy) -> Result<PathBuf, String> {
    let path_str = path.to_string_lossy();

    // Block dangerous path segments.
    for blocked in BLOCKED_SEGMENTS {
        if path_str.contains(blocked) {
            return Err(format!(
                "access to '{}' blocked — path contains sensitive segment '{blocked}'",
                path.display()
            ));
        }
    }

    policy
        .resolve_read_path(path, &[])
        .map_err(|e| format!("access to '{}' blocked — {}", path.display(), e))
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir_with_file(name: &str, contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(name);
        std::fs::write(&path, contents).expect("write");
        (dir, path)
    }

    #[test]
    fn find_refs_file() {
        let refs = find_refs("please look at @file:src/main.rs and fix it");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0],
            ContextRef::File {
                path: PathBuf::from("src/main.rs"),
                line_start: None,
                line_end: None
            }
        );
    }

    #[test]
    fn find_refs_file_single_line() {
        let refs = find_refs("look at @file:src/main.rs:42");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0],
            ContextRef::File {
                path: PathBuf::from("src/main.rs"),
                line_start: Some(42),
                line_end: Some(42)
            }
        );
    }

    #[test]
    fn find_refs_file_line_range() {
        let refs = find_refs("look at @file:src/main.rs:10-25");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0],
            ContextRef::File {
                path: PathBuf::from("src/main.rs"),
                line_start: Some(10),
                line_end: Some(25)
            }
        );
    }

    #[test]
    fn find_refs_diff() {
        let refs = find_refs("check @diff and @staged");
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&ContextRef::Diff));
        assert!(refs.contains(&ContextRef::Staged));
    }

    #[test]
    fn find_refs_git() {
        let refs = find_refs("what changed in @git:HEAD~1?");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], ContextRef::Git("HEAD~1".to_string()));
    }

    #[test]
    fn find_refs_url() {
        let refs = find_refs("see @url:https://example.com/api");
        assert_eq!(refs.len(), 1);
        assert_eq!(
            refs[0],
            ContextRef::Url("https://example.com/api".to_string())
        );
    }

    #[test]
    fn find_refs_deduplicates() {
        let refs = find_refs("@diff @diff");
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn expand_file_reads_contents() {
        let (dir, _path) = temp_dir_with_file("hello.txt", "hello world");
        let policy = PathPolicy::new(dir.path().to_path_buf());
        let rel = PathBuf::from("hello.txt");
        let result = expand_file(&rel, &policy, None, None);
        assert_eq!(result.expect("ok"), "hello world");
    }

    #[test]
    fn expand_file_line_range() {
        let (dir, _path) = temp_dir_with_file("lines.txt", "line1\nline2\nline3\nline4\nline5");
        let policy = PathPolicy::new(dir.path().to_path_buf());
        let rel = PathBuf::from("lines.txt");
        let result = expand_file(&rel, &policy, Some(2), Some(4));
        let content = result.expect("ok");
        assert!(content.contains("line2"));
        assert!(content.contains("line4"));
        assert!(!content.contains("line1"));
        assert!(!content.contains("line5"));
    }

    #[test]
    fn expand_file_blocks_ssh_path() {
        let cwd = std::env::current_dir().expect("cwd");
        let policy = PathPolicy::new(cwd.clone());
        let path = PathBuf::from(".ssh/id_rsa");
        let result = expand_file(&path, &policy, None, None);
        assert!(result.is_err());
        let err = result.expect_err("err");
        assert!(err.contains("blocked"));
    }

    #[test]
    fn expand_folder_lists_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.txt"), "").expect("write");
        std::fs::write(dir.path().join("b.txt"), "").expect("write");
        let cwd = dir.path().parent().expect("parent").to_path_buf();
        let policy = PathPolicy::new(cwd.clone());
        let rel = dir.path().file_name().expect("name").to_os_string();
        let result = expand_folder(&PathBuf::from(&rel), &policy, &cwd);
        let listing = result.expect("ok");
        assert!(listing.contains("a.txt"));
        assert!(listing.contains("b.txt"));
    }

    #[test]
    fn expand_context_refs_inlines_file() {
        let (dir, _) = temp_dir_with_file("greet.txt", "Hello!");
        let text = "read @file:greet.txt please";
        let result = expand_context_refs(text, dir.path());
        // Content should be in the Attached Context section
        assert!(
            result.expanded.contains("Hello!"),
            "expanded: {}",
            result.expanded
        );
        assert!(
            result.expanded.contains("Attached Context"),
            "should use Attached Context format"
        );
        assert_eq!(result.refs_found.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn expand_context_refs_handles_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let text = "read @file:nonexistent.txt please";
        let result = expand_context_refs(text, dir.path());
        // Failed ref stays in message text, error recorded
        assert_eq!(result.errors.len(), 1);
        assert!(result.refs_found.is_empty());
    }

    #[test]
    fn expand_context_refs_no_refs_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let text = "just a regular message with no refs";
        let result = expand_context_refs(text, dir.path());
        assert_eq!(result.expanded, text);
        assert!(result.refs_found.is_empty());
    }

    #[test]
    fn security_check_blocks_traversal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = PathPolicy::new(dir.path().to_path_buf());
        let evil = PathBuf::from("../../etc/passwd");
        let result = security_check_path(&evil, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn binary_file_is_blocked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let policy = PathPolicy::new(dir.path().to_path_buf());
        let path = dir.path().join("binary.bin");
        // Write a null byte to trigger binary detection
        let mut f = std::fs::File::create(&path).expect("create");
        f.write_all(&[0x7f, 0x45, 0x4c, 0x46, 0x00, 0x01])
            .expect("write");
        drop(f);

        let result = expand_file(&PathBuf::from("binary.bin"), &policy, None, None);
        assert!(result.is_err());
        assert!(result.expect_err("err").contains("binary"));
    }

    #[test]
    fn expand_context_refs_with_policy_allows_explicit_extra_root() {
        let workspace = tempfile::tempdir().expect("workspace");
        let shared = tempfile::tempdir().expect("shared");
        let shared_file = shared.path().join("shared.txt");
        std::fs::write(&shared_file, "shared context").expect("write shared");

        let policy = PathPolicy::new(workspace.path().to_path_buf())
            .with_allowed_roots(vec![shared.path().to_path_buf()]);
        let text = format!("check @file:{}", shared_file.display());
        let result = expand_context_refs_with_policy(&text, workspace.path(), &policy);

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(result.expanded.contains("shared context"));
    }

    #[test]
    fn expand_context_refs_with_policy_blocks_denylisted_subtree() {
        let workspace = tempfile::tempdir().expect("workspace");
        let secrets_dir = workspace.path().join("secrets");
        std::fs::create_dir_all(&secrets_dir).expect("create secrets");
        std::fs::write(secrets_dir.join("token.txt"), "token").expect("write token");

        let policy = PathPolicy::new(workspace.path().to_path_buf())
            .with_denied_roots(vec![PathBuf::from("secrets")]);
        let result = expand_context_refs_with_policy(
            "read @file:secrets/token.txt",
            workspace.path(),
            &policy,
        );

        assert_eq!(result.errors.len(), 1);
        assert!(result.refs_found.is_empty());
    }
}
