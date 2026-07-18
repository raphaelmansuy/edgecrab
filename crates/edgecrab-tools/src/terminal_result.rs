//! Machine-readable terminal tool result header (structured evidence cliff).
//!
//! Foreground `terminal` results are prefixed with:
//! ```text
//! [terminal_result status=success|error backend=… cwd=… exit_code=N]
//! ```
//! Contract / assess paths must parse this — do not substring-match alone.

/// Parsed `[terminal_result …]` header + remaining body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTerminalResult<'a> {
    pub status: &'a str,
    pub backend: &'a str,
    pub cwd: &'a str,
    pub exit_code: i32,
    /// Content after the header line (stdout/stderr body).
    pub body: &'a str,
}

/// Parse a terminal tool result. Returns `None` if the header is missing or malformed.
pub fn parse_terminal_result(content: &str) -> Option<ParsedTerminalResult<'_>> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("[terminal_result ") {
        return None;
    }
    let end = trimmed.find(']')?;
    let header = &trimmed[1..end]; // without leading '['
    // header: terminal_result status=… backend=… cwd=… exit_code=…
    let mut status = "";
    let mut backend = "";
    let mut cwd = "";
    let mut exit_code: Option<i32> = None;
    for part in header.split_whitespace().skip(1) {
        if let Some(v) = part.strip_prefix("status=") {
            status = v;
        } else if let Some(v) = part.strip_prefix("backend=") {
            backend = v;
        } else if let Some(v) = part.strip_prefix("cwd=") {
            cwd = v;
        } else if let Some(v) = part.strip_prefix("exit_code=") {
            exit_code = v.parse().ok();
        }
    }
    let exit_code = exit_code?;
    let rest = trimmed[end + 1..].trim_start_matches(['\r', '\n']);
    Some(ParsedTerminalResult {
        status,
        backend,
        cwd,
        exit_code,
        body: rest,
    })
}

/// True when content is a successful terminal result (`exit_code == 0`).
pub fn terminal_result_succeeded(content: &str) -> bool {
    parse_terminal_result(content).is_some_and(|p| p.exit_code == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_success_header() {
        let raw = "[terminal_result status=success backend=local cwd=/tmp exit_code=0]\nhello\n";
        let p = parse_terminal_result(raw).expect("parse");
        assert_eq!(p.status, "success");
        assert_eq!(p.backend, "local");
        assert_eq!(p.cwd, "/tmp");
        assert_eq!(p.exit_code, 0);
        assert_eq!(p.body, "hello\n");
        assert!(terminal_result_succeeded(raw));
    }

    #[test]
    fn parse_error_header() {
        let raw = "[terminal_result status=error backend=local cwd=/proj exit_code=1]\ncargo test\nFAILED\n";
        let p = parse_terminal_result(raw).expect("parse");
        assert_eq!(p.exit_code, 1);
        assert!(p.body.contains("cargo test"));
        assert!(!terminal_result_succeeded(raw));
    }

    #[test]
    fn reject_missing_header() {
        assert!(parse_terminal_result("cargo test\nok").is_none());
        assert!(!terminal_result_succeeded("cargo test\nok"));
    }

    #[test]
    fn reject_malformed_exit_code() {
        assert!(
            parse_terminal_result(
                "[terminal_result status=success backend=local cwd=/tmp exit_code=abc]\nx"
            )
            .is_none()
        );
    }
}
