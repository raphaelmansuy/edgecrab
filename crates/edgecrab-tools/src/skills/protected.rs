//! Load-bearing built-in skills the curator never archives (Hermes parity).

use std::collections::HashSet;
use std::sync::OnceLock;

static PROTECTED: OnceLock<HashSet<&'static str>> = OnceLock::new();

fn protected_set() -> &'static HashSet<&'static str> {
    PROTECTED.get_or_init(|| {
        // Keep tiny — skills that back slash-command UX (e.g. /plan).
        ["plan"].into_iter().collect()
    })
}

/// Whether a skill name is hardcoded as never archivable by the curator.
pub fn is_protected_builtin(skill_name: &str) -> bool {
    protected_set().contains(skill_name.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_protected() {
        assert!(is_protected_builtin("plan"));
        assert!(!is_protected_builtin("random-skill"));
    }
}
