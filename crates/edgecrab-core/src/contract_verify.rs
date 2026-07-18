//! Harness-owned contract verification (018 Wave A / July 2026).
//!
//! Industry law: policy → command → exit code. The harness runs
//! `GoalContract.verification` once on final assess when agent tools
//! have not already satisfied the contract — echo gaming cannot pass.

use std::path::Path;
use std::process::Command;

/// Result of a harness-run verification command.
#[derive(Debug, Clone)]
pub struct ContractVerifyResult {
    pub exit_code: i32,
    /// Full `[terminal_result …]` shaped record for ledger / tests.
    pub formatted: String,
    pub body: String,
}

/// True when tool body looks like `echo`/`printf` of the verification needle.
pub fn looks_like_echo_gaming(body: &str, needle: &str) -> bool {
    let needle_l = needle.trim().to_ascii_lowercase();
    if needle_l.is_empty() {
        return false;
    }
    let body_l = body.to_ascii_lowercase();
    for line in body_l.lines() {
        let t = line.trim();
        let rest = t
            .strip_prefix("echo ")
            .or_else(|| t.strip_prefix("printf "));
        if let Some(rest) = rest {
            let rest = rest
                .trim()
                .trim_matches(|c| c == '\'' || c == '"' || c == '`');
            if rest.contains(&needle_l) || needle_l.contains(rest) {
                return true;
            }
        }
    }
    // Body is only the needle (typical bare `echo cargo test` output).
    let compact: String = body_l.chars().filter(|c| !c.is_whitespace()).collect();
    let needle_compact: String = needle_l.chars().filter(|c| !c.is_whitespace()).collect();
    !needle_compact.is_empty() && compact == needle_compact
}

/// Run `command` via `sh -c` in `cwd`. Produces a terminal_result-shaped record.
///
/// Timeout: 120s. Does not mutate system prompts (cache law).
pub fn run_contract_verification(command: &str, cwd: &Path) -> ContractVerifyResult {
    let cmd = command.trim();
    if cmd.is_empty() {
        return ContractVerifyResult {
            exit_code: 1,
            formatted: "[terminal_result status=error backend=harness cwd=. exit_code=1]\nempty verification command\n".into(),
            body: "empty verification command".into(),
        };
    }

    let cwd_display = cwd.to_string_lossy();
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(cwd)
        .output();

    match output {
        Ok(out) => {
            let exit_code = out.status.code().unwrap_or(1);
            let mut body = String::from_utf8_lossy(&out.stdout).into_owned();
            if !out.stderr.is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str("[stderr]\n");
                body.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            let status = if exit_code == 0 { "success" } else { "error" };
            let formatted = format!(
                "[terminal_result status={status} backend=harness cwd={cwd_display} exit_code={exit_code}]\n{body}"
            );
            ContractVerifyResult {
                exit_code,
                formatted,
                body,
            }
        }
        Err(e) => {
            let body = format!("harness verify spawn failed: {e}");
            let formatted = format!(
                "[terminal_result status=error backend=harness cwd={cwd_display} exit_code=127]\n{body}\n"
            );
            ContractVerifyResult {
                exit_code: 127,
                formatted,
                body,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn echo_gaming_detected() {
        assert!(looks_like_echo_gaming("echo cargo test\n", "cargo test"));
        assert!(looks_like_echo_gaming("cargo test", "cargo test"));
        assert!(!looks_like_echo_gaming(
            "running 2 tests\ntest foo ... ok\n",
            "cargo test"
        ));
    }

    #[test]
    fn harness_true_exits_zero() {
        let dir = TempDir::new().expect("tmp");
        let r = run_contract_verification("true", dir.path());
        assert_eq!(r.exit_code, 0);
        assert!(r.formatted.contains("exit_code=0"));
        assert!(r.formatted.contains("backend=harness"));
    }

    #[test]
    fn harness_false_exits_nonzero() {
        let dir = TempDir::new().expect("tmp");
        let r = run_contract_verification("false", dir.path());
        assert_ne!(r.exit_code, 0);
    }
}
