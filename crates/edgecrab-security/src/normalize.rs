//! Command normalization to prevent obfuscation bypasses.
//!
//! Before scanning for dangerous patterns, we normalize:
//! 1. Strip ANSI escape sequences (CSI, OSC, DCS)
//! 2. Remove null bytes
//! 3. NFKC normalize Unicode (fullwidth → ASCII: `ｒｍ` → `rm`)
//! 4. Lowercase for case-insensitive matching

use unicode_normalization::UnicodeNormalization;

/// Normalize a command string before dangerous-pattern matching.
pub fn normalize_command(command: &str) -> String {
    let stripped = strip_ansi(command);
    let no_nulls = stripped.replace('\x00', "");
    let nfkc: String = no_nulls.nfkc().collect();
    nfkc.to_lowercase()
}

/// Strip ANSI escape sequences from a string.
fn strip_ansi(input: &str) -> String {
    // strip-ansi-escapes works on bytes
    let bytes = input.as_bytes();
    let stripped = strip_ansi_escapes::strip(bytes);
    String::from_utf8_lossy(&stripped).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_null_bytes() {
        assert_eq!(normalize_command("rm\x00 -rf /"), "rm -rf /");
    }

    #[test]
    fn normalizes_fullwidth_unicode() {
        // Fullwidth 'ｒｍ' should normalize to 'rm'
        assert_eq!(normalize_command("ｒｍ -rf /tmp"), "rm -rf /tmp");
    }

    #[test]
    fn lowercases() {
        assert_eq!(normalize_command("DROP TABLE users"), "drop table users");
    }

    #[test]
    fn strips_ansi_codes() {
        let with_ansi = "\x1b[31mrm -rf /\x1b[0m";
        let result = normalize_command(with_ansi);
        assert_eq!(result, "rm -rf /");
    }

    #[test]
    fn combined_bypass_attempt() {
        // Attacker tries: fullwidth + null bytes + ANSI
        let malicious = "\x1b[0mｒ\x00ｍ -rf /etc";
        let result = normalize_command(malicious);
        assert!(result.contains("rm -rf /etc"));
    }
}
