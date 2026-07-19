//! Hermes DEFAULT_TAPS parity — seeded into `taps.json` on hub init.
//!
//! Tap seeds derive from [`super::catalog::HUB_CATALOG`] (DRY).

use super::catalog::{TapSeed, default_tap_seeds};
use super::{Tap, add_tap_if_missing, read_taps};

/// Builtin taps matching Hermes `GitHubSource.DEFAULT_TAPS` (+ EdgeCrab notes).
pub fn default_taps() -> Vec<TapSeed> {
    default_tap_seeds()
}

fn seed_to_tap(seed: &TapSeed) -> Tap {
    Tap {
        name: seed.name.to_string(),
        url: seed.github_url(),
        trust_level: seed.trust_level.to_string(),
    }
}

/// Ensure Hermes-parity default taps exist (idempotent).
pub fn ensure_default_taps() -> usize {
    let mut added = 0;
    for seed in default_tap_seeds() {
        if add_tap_if_missing(&seed_to_tap(&seed)) {
            added += 1;
        }
    }
    added
}

/// Whether default taps include the given repo (case-insensitive).
#[allow(dead_code)]
pub fn default_taps_include_repo(repo: &str) -> bool {
    let repo = repo.to_ascii_lowercase();
    default_tap_seeds()
        .iter()
        .any(|t| t.repo.eq_ignore_ascii_case(&repo))
}

pub fn format_default_taps_summary() -> String {
    let _ = ensure_default_taps();
    let taps = read_taps();
    let seeds = default_tap_seeds();
    let mut out = format!("Hub taps ({} total, defaults seeded):\n", taps.len());
    for seed in &seeds {
        let present = taps.iter().any(|t| t.name == seed.name);
        out.push_str(&format!(
            "  {} {}/{} [{}] {}\n",
            if present { "✓" } else { "·" },
            seed.repo,
            if seed.root.is_empty() {
                "(root)"
            } else {
                seed.root
            },
            seed.trust_level,
            seed.name
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEdgecrabHome as TestHome;

    #[test]
    fn seeds_huggingface_and_nvidia() {
        let _home = TestHome::new();
        let added = ensure_default_taps();
        assert!(added >= 1);
        assert!(default_taps_include_repo("huggingface/skills"));
        assert!(default_taps_include_repo("NVIDIA/skills"));
        let taps = read_taps();
        assert!(taps.iter().any(|t| t.name == "huggingface-skills"));
        assert!(taps.iter().any(|t| t.name == "nvidia-skills"));
        assert!(taps.iter().any(|t| t.name == "gstack"));
        // idempotent
        assert_eq!(ensure_default_taps(), 0);
    }
}
