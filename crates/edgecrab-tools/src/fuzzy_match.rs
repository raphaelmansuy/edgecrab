//! # fuzzy_match — 8-strategy fuzzy find-and-replace for the `patch` tool
//!
//! Implements the same chain used by hermes-agent's `tools/fuzzy_match.py`,
//! enabling the `patch` tool to handle common LLM-generated code variations:
//! trailing spaces, indentation drift, smart quotes, escaped newlines, etc.
//!
//! ## Strategy chain (tried in order, first match wins)
//! 1. **Exact**              — direct `str::find`
//! 2. **Line-trimmed**       — strip leading+trailing whitespace per line
//! 3. **Whitespace-norm**    — collapse `[ \t]+` to single space per line
//! 4. **Indent-flexible**    — strip leading whitespace per line
//! 5. **Escape-norm**        — convert `\\n`/`\\t`/`\\r` literals to chars
//! 6. **Trimmed-boundary**   — trim only first and last line
//! 7. **Block-anchor**       — match first+last line; 10/30% middle similarity
//! 8. **Context-aware**      — ≥50% of lines at ≥80% similarity (LCS ratio)
//!
//! ## Unicode normalization
//! Smart quotes, em-dashes, non-breaking spaces are replaced with ASCII
//! equivalents before strategies 7–8 (matches hermes behaviour).

/// Byte-range match `[start, end)` in the original content string.
type Match = (usize, usize);

// ─── Unicode normalization ────────────────────────────────────────────────

/// Replace common Unicode typographic characters with ASCII equivalents.
/// Mirrors hermes `_unicode_normalize()` in fuzzy_match.py.
fn unicode_normalize(s: &str) -> String {
    s.replace(['\u{201C}', '\u{201D}'], "\"") // double quotation marks
        .replace(['\u{2018}', '\u{2019}'], "'") // single quotation marks
        .replace('\u{2014}', "--") // em dash
        .replace('\u{2013}', "-") // en dash
        .replace('\u{2026}', "...") // horizontal ellipsis
        .replace('\u{00A0}', " ") // non-breaking space
}

// ─── Sequence similarity (LCS ratio) ────────────────────────────────────

/// Compute the *character-level* similarity ratio between two strings.
///
/// `ratio = 2 * lcs_len(a,b) / (|a| + |b|)`, matching difflib's SequenceMatcher.
/// Used only for strategies 7 and 8 on typical code lines (≤200 chars), so
/// O(|a|·|b|) DP is acceptable.
fn similarity_ratio(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let total = a.chars().count() + b.chars().count();
    if total == 0 {
        return 1.0;
    }
    let lcs = lcs_char_len(a, b);
    2.0 * lcs as f64 / total as f64
}

fn lcs_char_len(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut prev = vec![0usize; b.len() + 1];
    for &ca in &a {
        let mut curr = vec![0usize; b.len() + 1];
        for (j, &cb) in b.iter().enumerate() {
            curr[j + 1] = if ca == cb {
                prev[j] + 1
            } else {
                curr[j].max(prev[j + 1])
            };
        }
        prev = curr;
    }
    prev[b.len()]
}

// ─── Positional helpers ──────────────────────────────────────────────────

/// Convert a range of `[start_line, end_line)` (0-based, exclusive) to byte
/// offsets `(start, end)` in the original `content` string.
///
/// The start offset is the first byte of `content_lines[start_line]`.
/// The end offset is the byte *after* the last character of
/// `content_lines[end_line - 1]` (i.e. just before the `\n` separator or at
/// end-of-string), making the range suitable for `replace_range(start..end)`.
///
/// Invariant: splitting `content` by `\n` must yield `content_lines`.
fn line_range_to_bytes(content_lines: &[&str], start_line: usize, end_line: usize) -> Match {
    // Walk to start_line, accumulating byte lengths (+1 for the '\n').
    let start_pos: usize = content_lines[..start_line]
        .iter()
        .map(|l| l.len() + 1)
        .sum();

    // Walk to end_line and subtract 1 to exclude the trailing '\n'.
    let end_pos: usize = content_lines[..end_line]
        .iter()
        .map(|l| l.len() + 1)
        .sum::<usize>()
        .saturating_sub(1);

    (start_pos, end_pos)
}

/// Scan `content_norm_lines` for windows matching `pattern_norm_lines` and
/// return byte offsets into the *original* content (via `content_lines`).
fn find_line_matches(
    content_lines: &[&str],
    content_norm_lines: &[String],
    pattern_norm_lines: &[String],
) -> Vec<Match> {
    let pc = pattern_norm_lines.len();
    if pc == 0 || pc > content_norm_lines.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let norm_pat: Vec<&str> = pattern_norm_lines.iter().map(|s| s.as_str()).collect();
    for i in 0..=(content_norm_lines.len() - pc) {
        let window: Vec<&str> = content_norm_lines[i..i + pc]
            .iter()
            .map(|s| s.as_str())
            .collect();
        if window == norm_pat {
            matches.push(line_range_to_bytes(content_lines, i, i + pc));
        }
    }
    matches
}

// ─── Apply replacements ──────────────────────────────────────────────────

/// Replace all `matches` (byte ranges) with `new_string` in `content`.
///
/// Matches are applied from end to start so that earlier byte offsets
/// remain valid after each substitution (same approach as hermes).
fn apply_replacements(content: &str, matches: &[Match], new_string: &str) -> String {
    let mut sorted: Vec<Match> = matches.to_vec();
    sorted.sort_by_key(|item| std::cmp::Reverse(item.0));
    let mut result = content.to_string();
    for (start, end) in sorted {
        let end = end.min(result.len());
        result.replace_range(start..end, new_string);
    }
    result
}

// ─── 8 strategies ────────────────────────────────────────────────────────

/// Strategy 1: Exact byte-level match.
fn strategy_exact(content: &str, old: &str) -> Vec<Match> {
    let mut matches = Vec::new();
    let mut start = 0;
    while let Some(rel) = content[start..].find(old) {
        let pos = start + rel;
        matches.push((pos, pos + old.len()));
        start = pos + 1;
    }
    matches
}

/// Strategy 2: Strip leading+trailing whitespace from *every* line.
fn strategy_line_trimmed(content: &str, old: &str) -> Vec<Match> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let norm_content: Vec<String> = content_lines.iter().map(|l| l.trim().to_string()).collect();
    let norm_pattern: Vec<String> = old.split('\n').map(|l| l.trim().to_string()).collect();
    find_line_matches(&content_lines, &norm_content, &norm_pattern)
}

/// Collapse consecutive spaces/tabs on a single line to one space.
fn collapse_spaces(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut prev_space = false;
    for c in line.chars() {
        if c == ' ' || c == '\t' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result
}

/// Strategy 3: Collapse multiple spaces/tabs to a single space per line.
fn strategy_whitespace_normalized(content: &str, old: &str) -> Vec<Match> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let norm_content: Vec<String> = content_lines.iter().map(|l| collapse_spaces(l)).collect();
    let norm_pattern: Vec<String> = old.split('\n').map(collapse_spaces).collect();
    find_line_matches(&content_lines, &norm_content, &norm_pattern)
}

/// Strategy 4: Strip *all* leading whitespace (indentation) from every line.
fn strategy_indentation_flexible(content: &str, old: &str) -> Vec<Match> {
    let content_lines: Vec<&str> = content.split('\n').collect();
    let norm_content: Vec<String> = content_lines
        .iter()
        .map(|l| l.trim_start().to_string())
        .collect();
    let norm_pattern: Vec<String> = old
        .split('\n')
        .map(|l| l.trim_start().to_string())
        .collect();
    find_line_matches(&content_lines, &norm_content, &norm_pattern)
}

/// Strategy 5: Convert escaped sequences (`\\n`, `\\t`, `\\r`) in `old` to
/// their real character equivalents and then do an exact search.
fn strategy_escape_normalized(content: &str, old: &str) -> Vec<Match> {
    let unescaped = old
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\r", "\r");
    if unescaped == old {
        return Vec::new(); // no escape sequences — nothing to do
    }
    strategy_exact(content, &unescaped)
}

/// Strategy 6: Trim whitespace on the *first and last* lines only.
fn strategy_trimmed_boundary(content: &str, old: &str) -> Vec<Match> {
    let old_lines: Vec<&str> = old.split('\n').collect();
    if old_lines.is_empty() {
        return Vec::new();
    }
    let mut norm_pattern: Vec<String> = old_lines.iter().map(|l| l.to_string()).collect();
    norm_pattern[0] = norm_pattern[0].trim().to_string();
    let last = norm_pattern.len() - 1;
    if last > 0 {
        norm_pattern[last] = norm_pattern[last].trim().to_string();
    }

    let content_lines: Vec<&str> = content.split('\n').collect();
    let pc = norm_pattern.len();
    let mut matches = Vec::new();

    for i in 0..=content_lines.len().saturating_sub(pc) {
        let mut block: Vec<String> = content_lines[i..i + pc]
            .iter()
            .map(|l| l.to_string())
            .collect();
        block[0] = block[0].trim().to_string();
        let last_b = block.len() - 1;
        if last_b > 0 {
            block[last_b] = block[last_b].trim().to_string();
        }
        if block == norm_pattern {
            matches.push(line_range_to_bytes(&content_lines, i, i + pc));
        }
    }
    matches
}

/// Strategy 7: Anchor match on first+last line, use LCS similarity for middle.
///
/// Unicode normalization is applied before comparison.
/// Threshold: 0.10 (one candidate) or 0.30 (multiple candidates) — mirrors hermes.
fn strategy_block_anchor(content: &str, old: &str) -> Vec<Match> {
    let old_norm = unicode_normalize(old);
    let content_norm = unicode_normalize(content);

    let pattern_lines: Vec<&str> = old_norm.split('\n').collect();
    if pattern_lines.len() < 2 {
        return Vec::new();
    }
    let first = pattern_lines[0].trim();
    let last_line = pattern_lines[pattern_lines.len() - 1].trim();
    let pc = pattern_lines.len();

    let norm_content_lines: Vec<&str> = content_norm.split('\n').collect();
    let orig_content_lines: Vec<&str> = content.split('\n').collect();

    let potentials: Vec<usize> = (0..=norm_content_lines.len().saturating_sub(pc))
        .filter(|&i| {
            norm_content_lines[i].trim() == first
                && norm_content_lines[i + pc - 1].trim() == last_line
        })
        .collect();

    let threshold = if potentials.len() == 1 { 0.10 } else { 0.30 };
    let mut matches = Vec::new();

    for i in potentials {
        let similarity = if pc <= 2 {
            1.0
        } else {
            let content_middle = norm_content_lines[i + 1..i + pc - 1].join("\n");
            let pattern_middle = pattern_lines[1..pc - 1].join("\n");
            similarity_ratio(&content_middle, &pattern_middle)
        };
        if similarity >= threshold {
            matches.push(line_range_to_bytes(&orig_content_lines, i, i + pc));
        }
    }
    matches
}

/// Strategy 8: ≥50% of lines must have ≥80% LCS similarity.
fn strategy_context_aware(content: &str, old: &str) -> Vec<Match> {
    let pattern_lines: Vec<&str> = old.split('\n').collect();
    let content_lines: Vec<&str> = content.split('\n').collect();
    let pc = pattern_lines.len();
    if pc == 0 || pc > content_lines.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    for i in 0..=(content_lines.len() - pc) {
        let high_sim = content_lines[i..i + pc]
            .iter()
            .zip(pattern_lines.iter())
            .filter(|(cl, pl)| similarity_ratio(cl.trim(), pl.trim()) >= 0.80)
            .count();
        // ≥50% threshold
        if high_sim * 2 >= pc {
            matches.push(line_range_to_bytes(&content_lines, i, i + pc));
        }
    }
    matches
}

// ─── Public API ──────────────────────────────────────────────────────────

/// Find `old_string` in `content` using up to 8 strategies and replace it
/// with `new_string`.
///
/// Returns `(new_content, replacement_count)` on success, or an error
/// message string on failure.
///
/// When `replace_all` is `false` (default):
/// - Zero matches → error with a hint to re-read the file.
/// - Multiple matches → error asking for more context (or suggest `replace_all`).
///
/// When `replace_all` is `true`, all occurrences found by the first successful
/// strategy are replaced.
pub fn fuzzy_find_and_replace(
    content: &str,
    old_string: &str,
    new_string: &str,
    replace_all: bool,
) -> Result<(String, usize), String> {
    if old_string.is_empty() {
        return Err("old_string cannot be empty".into());
    }
    if old_string == new_string {
        return Err("old_string and new_string are identical — no change needed".into());
    }

    // Dispatch table of all 8 strategies (order matters — most conservative first)
    type MatchStrategy = fn(&str, &str) -> Vec<Match>;
    let strategies: &[(&str, MatchStrategy)] = &[
        ("exact", strategy_exact),
        ("line_trimmed", strategy_line_trimmed),
        ("whitespace_normalized", strategy_whitespace_normalized),
        ("indentation_flexible", strategy_indentation_flexible),
        ("escape_normalized", strategy_escape_normalized),
        ("trimmed_boundary", strategy_trimmed_boundary),
        ("block_anchor", strategy_block_anchor),
        ("context_aware", strategy_context_aware),
    ];

    for &(name, strategy) in strategies {
        let matches = strategy(content, old_string);
        if matches.is_empty() {
            continue;
        }
        if matches.len() > 1 && !replace_all {
            return Err(format!(
                "Found {} occurrences of old_string (strategy: {}). \
                 Include more surrounding context to make the match unique, \
                 or set replace_all=true to replace all occurrences.",
                matches.len(),
                name
            ));
        }
        let count = matches.len();
        let new_content = apply_replacements(content, &matches, new_string);
        return Ok((new_content, count));
    }

    Err(
        "Could not find old_string in the file after trying 8 matching strategies. \
         Use read_file to verify the current content before retrying."
            .into(),
    )
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── strategy_exact ────────────────────────────────────────────────────

    #[test]
    fn exact_single_match() {
        let (out, n) = fuzzy_find_and_replace("hello world", "world", "rust", false).expect("ok");
        assert_eq!(out, "hello rust");
        assert_eq!(n, 1);
    }

    #[test]
    fn exact_replace_all() {
        let (out, n) = fuzzy_find_and_replace("aa bb aa", "aa", "xx", true).expect("ok");
        assert_eq!(out, "xx bb xx");
        assert_eq!(n, 2);
    }

    #[test]
    fn exact_multiple_without_replace_all_is_error() {
        let err = fuzzy_find_and_replace("aa bb aa", "aa", "xx", false).unwrap_err();
        assert!(err.contains("2 occurrences"), "got: {err}");
    }

    #[test]
    fn identical_strings_error() {
        let err = fuzzy_find_and_replace("content", "same", "same", false).unwrap_err();
        assert!(err.contains("identical"));
    }

    // ── strategy_line_trimmed ─────────────────────────────────────────────

    #[test]
    fn line_trimmed_trailing_spaces() {
        let content = "fn foo() {  \n    let x = 1;\n}";
        let old = "fn foo() {\n    let x = 1;\n}";
        let (out, _) = fuzzy_find_and_replace(content, old, "fn foo() { }", false).expect("ok");
        assert!(out.contains("fn foo() { }"));
    }

    // ── strategy_whitespace_normalized ────────────────────────────────────

    #[test]
    fn whitespace_normalized_extra_spaces() {
        let content = "let  x  =  1;";
        let old = "let x = 1;";
        let (out, _) = fuzzy_find_and_replace(content, old, "let x = 2;", false).expect("ok");
        assert!(out.contains("let x = 2;"));
    }

    // ── strategy_indentation_flexible ─────────────────────────────────────

    #[test]
    fn indentation_flexible_different_indent() {
        // Pattern has 2-space indent; content has 4-space indent.
        let content = "def foo():\n    pass\n    return 1";
        let old = "def foo():\n  pass\n  return 1";
        let (out, _) =
            fuzzy_find_and_replace(content, old, "def foo():\n    return 0", false).expect("ok");
        assert!(out.contains("return 0"));
    }

    // ── strategy_escape_normalized ────────────────────────────────────────

    #[test]
    fn escape_normalized_converts_backslash_n() {
        let content = "line1\nline2";
        let old = "line1\\nline2"; // LLM sent \n literal
        let (out, _) = fuzzy_find_and_replace(content, old, "replaced", false).expect("ok");
        assert_eq!(out, "replaced");
    }

    // ── strategy_trimmed_boundary ─────────────────────────────────────────

    #[test]
    fn trimmed_boundary_first_last_line() {
        let content = "  fn foo() {\n    body\n  }  ";
        let old = "fn foo() {\n    body\n}";
        let (out, _) =
            fuzzy_find_and_replace(content, old, "fn foo() { /* replaced */ }", false).expect("ok");
        assert!(out.contains("replaced"), "got: {out}");
    }

    // ── strategy_block_anchor ─────────────────────────────────────────────

    #[test]
    fn block_anchor_unicode_smart_quotes() {
        let content = "fn foo() {\n    let x = \u{201C}hello\u{201D};\n}";
        let old = "fn foo() {\n    let x = \"hello\";\n}";
        let (out, _) =
            fuzzy_find_and_replace(content, old, "fn foo() { /* */ }", false).expect("ok");
        assert!(
            out.contains("replaced") || out.contains("/* */"),
            "got: {out}"
        );
    }

    // ── strategy_context_aware ─────────────────────────────────────────────

    #[test]
    fn context_aware_minor_typo() {
        // Pattern has a minor typo in the body; 2/3 lines still match at ≥80%
        let content = "def foo():\n    x = compute()\n    return x";
        let old = "def foo():\n    x = compute() \n    return x"; // trailing space
        let (out, _) =
            fuzzy_find_and_replace(content, old, "def foo():\n    return 0", false).expect("ok");
        assert!(out.contains("return 0"));
    }

    // ── line_range_to_bytes ───────────────────────────────────────────────

    #[test]
    fn line_range_to_bytes_basic() {
        let content = "abc\ndef\nghi";
        let lines: Vec<&str> = content.split('\n').collect();

        // Full file
        let (s, e) = line_range_to_bytes(&lines, 0, 3);
        assert_eq!(&content[s..e], "abc\ndef\nghi");

        // First line only
        let (s, e) = line_range_to_bytes(&lines, 0, 1);
        assert_eq!(&content[s..e], "abc");

        // Middle line
        let (s, e) = line_range_to_bytes(&lines, 1, 2);
        assert_eq!(&content[s..e], "def");
    }

    // ── lcs similarity ────────────────────────────────────────────────────

    #[test]
    fn similarity_identical() {
        assert_eq!(similarity_ratio("abc", "abc"), 1.0);
    }

    #[test]
    fn similarity_empty() {
        assert_eq!(similarity_ratio("", ""), 1.0);
    }

    #[test]
    fn similarity_partial() {
        let r = similarity_ratio("hello", "hxllx");
        assert!(r > 0.5 && r < 1.0, "ratio={r}");
    }
}
