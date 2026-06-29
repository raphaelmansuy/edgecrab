//! Tool-name repair — Hermes `repair_tool_call` parity in one DRY module.
//!
//! Models emit class-like names (`Patch_tool`, `BrowserClick`), XML pollution
//! from some gateways, and chat-template token leaks (`<|channel|>commentary`).
//! All dispatch paths normalize through [`ToolRegistry::resolve_tool_call_name`].

use std::collections::HashSet;

use crate::registry::ToolRegistry;

/// Result of resolving a raw wire name to a registry-known tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedToolName {
    pub original: String,
    pub canonical: String,
    /// True when repair changed the name (not just trim/normalize).
    pub repaired: bool,
}

/// Strip gateway XML leaks and chat-template suffixes before candidate generation.
///
/// Hermes trims at the first `"` / `'` / `<` / `>` (VolcEngine #33007).
/// EdgeCrab additionally strips `<|…` special-token suffixes (FP54).
pub fn strip_tool_name_pollution(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut name = trimmed.to_string();
    for sep in ['"', '\'', '<', '>'] {
        if let Some(idx) = name.find(sep)
            && idx > 0
        {
            name.truncate(idx);
        }
    }

    let base = if let Some(pos) = name.find("<|") {
        name[..pos].trim_end()
    } else {
        name.trim_end()
    };

    base.replace([' ', '-'], "_")
}

fn norm_separators(s: &str) -> String {
    s.to_lowercase().replace(['-', ' '], "_")
}

fn camel_to_snake(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn strip_tool_suffix(s: &str) -> Option<String> {
    let lc = s.to_lowercase();
    for suffix in ["_tool", "-tool", "tool"] {
        if lc.ends_with(suffix) {
            let end = s.len().saturating_sub(suffix.len());
            return Some(s[..end].trim_end_matches(['_', '-']).to_string());
        }
    }
    None
}

/// Attempt to repair a mismatched tool name against the registry (Hermes parity).
pub fn repair_tool_name(registry: &ToolRegistry, raw: &str) -> Option<String> {
    let cleaned = strip_tool_name_pollution(raw);
    if cleaned.is_empty() {
        return None;
    }

    if let Some(canonical) = registry.lookup_tool_name(&cleaned) {
        return Some(canonical);
    }

    let lowered = cleaned.to_lowercase();
    if let Some(canonical) = registry.lookup_tool_name(&lowered) {
        return Some(canonical);
    }

    let normalized = norm_separators(&cleaned);
    if let Some(canonical) = registry.lookup_tool_name(&normalized) {
        return Some(canonical);
    }

    let mut candidates: HashSet<String> = HashSet::new();
    candidates.insert(cleaned.clone());
    candidates.insert(lowered.clone());
    candidates.insert(normalized.clone());
    candidates.insert(camel_to_snake(&cleaned));

    for _ in 0..2 {
        let mut extra = HashSet::new();
        for c in &candidates {
            if let Some(stripped) = strip_tool_suffix(c) {
                extra.insert(stripped.clone());
                extra.insert(norm_separators(&stripped));
                extra.insert(camel_to_snake(&stripped));
            }
        }
        candidates.extend(extra);
    }

    for c in candidates {
        if c.is_empty() {
            continue;
        }
        if let Some(canonical) = registry.lookup_tool_name(&c) {
            return Some(canonical);
        }
    }

    fuzzy_match_tool_name(registry, &lowered)
}

/// Resolve a wire tool name: exact lookup → repair → pollution-stripped fallback.
pub fn resolve_tool_call_name(registry: &ToolRegistry, raw: &str) -> ResolvedToolName {
    let original = raw.to_string();
    let trimmed = raw.trim();

    if let Some(canonical) = registry.lookup_tool_name(trimmed) {
        return ResolvedToolName {
            original,
            canonical,
            repaired: false,
        };
    }

    if let Some(canonical) = repair_tool_name(registry, raw) {
        return ResolvedToolName {
            original,
            canonical,
            repaired: true,
        };
    }

    let stripped = strip_tool_name_pollution(raw);
    let repaired = stripped != trimmed;
    ResolvedToolName {
        original,
        canonical: stripped,
        repaired,
    }
}

/// Fuzzy match with Jaro-Winkler ≥ 0.7 (Hermes difflib cutoff parity).
pub fn fuzzy_match_tool_name(registry: &ToolRegistry, name: &str) -> Option<String> {
    const CUTOFF: f64 = 0.7;
    let mut best: Option<(String, f64)> = None;

    for candidate in registry.tool_names() {
        let score = strsim::jaro_winkler(name, candidate);
        if score >= CUTOFF && best.as_ref().is_none_or(|(_, s)| score > *s) {
            best = Some((candidate.to_string(), score));
        }
    }

    best.map(|(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ToolRegistry {
        ToolRegistry::new()
    }

    #[test]
    fn tn01_clean_name_borrowed_path() {
        let reg = registry();
        let resolved = resolve_tool_call_name(&reg, "write_file");
        assert_eq!(resolved.canonical, "write_file");
        assert!(!resolved.repaired);
    }

    #[test]
    fn tn02_strips_channel_token() {
        let reg = registry();
        let resolved = resolve_tool_call_name(&reg, "web_extract<|channel|>commentary");
        assert_eq!(resolved.canonical, "web_extract");
    }

    #[test]
    fn tn03_normalizes_spaces_and_hyphens() {
        let reg = registry();
        assert_eq!(
            resolve_tool_call_name(&reg, "write file").canonical,
            "write_file"
        );
        assert_eq!(
            resolve_tool_call_name(&reg, "web-extract").canonical,
            "web_extract"
        );
    }

    #[test]
    fn tn04_class_like_patch_tool_suffix() {
        let reg = registry();
        assert_eq!(repair_tool_name(&reg, "Patch_tool"), Some("patch".into()));
        assert_eq!(repair_tool_name(&reg, "PatchTool"), Some("patch".into()));
    }

    #[test]
    fn tn05_browser_click_variants() {
        let reg = registry();
        assert_eq!(
            repair_tool_name(&reg, "BrowserClick_tool"),
            Some("browser_click".into())
        );
        assert_eq!(
            repair_tool_name(&reg, "BrowserClick"),
            Some("browser_click".into())
        );
    }

    #[test]
    fn tn06_write_file_camel_case() {
        let reg = registry();
        assert_eq!(
            repair_tool_name(&reg, "WriteFileTool"),
            Some("write_file".into())
        );
    }

    #[test]
    fn tn07_fuzzy_typo_terminal() {
        let reg = registry();
        assert_eq!(repair_tool_name(&reg, "terminall"), Some("terminal".into()));
    }

    #[test]
    fn tn08_xml_pollution_terminal() {
        let reg = registry();
        let polluted = r#"terminal" parameter="command" string="true"#;
        assert_eq!(repair_tool_name(&reg, polluted), Some("terminal".into()));
    }

    #[test]
    fn tn09_unknown_returns_none() {
        let reg = registry();
        assert!(repair_tool_name(&reg, "xyz_no_such_tool").is_none());
    }

    #[test]
    fn tn10_empty_returns_none() {
        let reg = registry();
        assert!(repair_tool_name(&reg, "").is_none());
        assert!(repair_tool_name(&reg, "<|channel|>commentary").is_none());
    }
}
