//! # pricing — Per-model cost estimation engine
//!
//! WHY a hardcoded table: Official API pricing pages are the source of truth.
//! Runtime API discovery (OpenRouter `/models`, Anthropic headers) is unreliable
//! across providers and adds latency. A static snapshot + override config covers
//! 95% of real-world usage. The table is versioned for auditability.
//!
//! ```text
//!   LLMResponse.usage
//!       │
//!       ├── normalize_usage()  → CanonicalUsage (5 buckets)
//!       ├── resolve_billing()  → BillingRoute (provider/model/mode)
//!       └── estimate_cost()    → CostResult (USD amount + status)
//! ```

use std::collections::HashMap;
use std::sync::LazyLock;

// ── Types ────────────────────────────────────────────────────────────

/// Normalized token usage across all providers.
#[derive(Debug, Clone, Default)]
pub struct CanonicalUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

impl CanonicalUsage {
    pub fn prompt_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_tokens + self.cache_write_tokens
    }

    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens() + self.output_tokens + self.reasoning_tokens
    }
}

/// Per-model pricing snapshot (cost per million tokens).
#[derive(Debug, Clone)]
pub struct PricingEntry {
    pub input_cost_per_million: f64,
    pub output_cost_per_million: f64,
    pub cache_read_cost_per_million: f64,
    pub cache_write_cost_per_million: f64,
    pub source: CostSource,
}

/// Where the pricing data came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostSource {
    OfficialDocsSnapshot,
    UserOverride,
    Unknown,
}

/// Cost estimation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostStatus {
    Estimated,
    Included,
    Unknown,
}

/// Final cost result.
#[derive(Debug, Clone)]
pub struct CostResult {
    pub amount_usd: Option<f64>,
    pub status: CostStatus,
    pub source: CostSource,
    pub label: String,
}

// ── Pricing table ────────────────────────────────────────────────────

/// Static pricing snapshot — built from ModelCatalog on first access.
/// Models without pricing in the catalog are not included.
static PRICING_TABLE: LazyLock<HashMap<String, PricingEntry>> = LazyLock::new(|| {
    let cat = crate::model_catalog::ModelCatalog::get();
    let mut m = HashMap::new();
    for (pid, entry) in &cat.providers {
        for model in &entry.models {
            if let Some(ref p) = model.pricing {
                let key = format!("{pid}/{}", model.model).to_lowercase();
                m.insert(
                    key,
                    PricingEntry {
                        input_cost_per_million: p.input,
                        output_cost_per_million: p.output,
                        cache_read_cost_per_million: p.cache_read,
                        cache_write_cost_per_million: p.cache_write,
                        source: CostSource::OfficialDocsSnapshot,
                    },
                );
            }
        }
    }
    m
});

// ── Public API ───────────────────────────────────────────────────────

/// Zero-cost entry for subscription-included or local models.
static ZERO_COST: LazyLock<PricingEntry> = LazyLock::new(|| PricingEntry {
    input_cost_per_million: 0.0,
    output_cost_per_million: 0.0,
    cache_read_cost_per_million: 0.0,
    cache_write_cost_per_million: 0.0,
    source: CostSource::OfficialDocsSnapshot,
});

/// Providers that are subscription-included or local (zero cost).
const ZERO_COST_PROVIDERS: &[&str] = &["copilot", "ollama", "lmstudio"];

/// Look up pricing for a model. Tries exact match first, then prefix/fuzzy,
/// then falls back to zero-cost for subscription/local providers.
pub fn get_pricing(model: &str) -> Option<&PricingEntry> {
    let key = model.to_lowercase();
    PRICING_TABLE
        .get(&key)
        .or_else(|| {
            // Fuzzy: try stripping date suffixes (e.g. "anthropic/claude-3-5-sonnet-20241022")
            // Also match "model-latest" ↔ "model-YYYYMMDD"
            let stem = strip_date_suffix(&key);
            PRICING_TABLE
                .iter()
                .find(|(k, _)| {
                    let k_stem = strip_date_suffix(k);
                    key.starts_with(k.as_str()) || k.starts_with(&key) || stem == k_stem
                })
                .map(|(_, v)| v)
        })
        .or_else(|| {
            // Zero-cost fallback for subscription-included / local providers
            let provider = key.split('/').next().unwrap_or("");
            if ZERO_COST_PROVIDERS.contains(&provider) {
                Some(&*ZERO_COST)
            } else {
                None
            }
        })
}

/// Strip common date suffixes and "-latest" from model names for fuzzy matching.
fn strip_date_suffix(name: &str) -> String {
    if name.ends_with("-latest") {
        return name.strip_suffix("-latest").unwrap_or(name).to_string();
    }
    // Strip -YYYYMMDD (8 digits after a dash)
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        if suffix.len() == 8
            && suffix.starts_with("20")
            && suffix.chars().all(|c| c.is_ascii_digit())
        {
            return name[..pos].to_string();
        }
    }
    name.to_string()
}

/// Estimate cost for a canonical usage snapshot against a model's pricing.
pub fn estimate_cost(usage: &CanonicalUsage, model: &str) -> CostResult {
    let Some(pricing) = get_pricing(model) else {
        return CostResult {
            amount_usd: None,
            status: CostStatus::Unknown,
            source: CostSource::Unknown,
            label: format!("No pricing data for {model}"),
        };
    };

    // Zero-cost models (copilot, local)
    if pricing.input_cost_per_million == 0.0 && pricing.output_cost_per_million == 0.0 {
        return CostResult {
            amount_usd: Some(0.0),
            status: CostStatus::Included,
            source: pricing.source,
            label: "Included in subscription / local".into(),
        };
    }

    let cost = (usage.input_tokens as f64 * pricing.input_cost_per_million
        + usage.output_tokens as f64 * pricing.output_cost_per_million
        + usage.cache_read_tokens as f64 * pricing.cache_read_cost_per_million
        + usage.cache_write_tokens as f64 * pricing.cache_write_cost_per_million)
        / 1_000_000.0;

    CostResult {
        amount_usd: Some(cost),
        status: CostStatus::Estimated,
        source: pricing.source,
        label: "Estimated from official docs pricing".into(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn pricing_table_has_entries() {
        assert!(
            PRICING_TABLE.len() >= 20,
            "expected at least 20 pricing entries"
        );
    }

    #[test]
    fn get_pricing_exact_match() {
        let p = get_pricing("openai/gpt-4o").unwrap();
        assert!((p.input_cost_per_million - 2.5).abs() < 0.01);
        assert!((p.output_cost_per_million - 10.0).abs() < 0.01);
    }

    #[test]
    fn get_pricing_copilot_is_zero() {
        let p = get_pricing("copilot/gpt-4.1-mini").unwrap();
        assert_eq!(p.input_cost_per_million, 0.0);
        assert_eq!(p.output_cost_per_million, 0.0);
    }

    #[test]
    fn get_pricing_unknown_model_returns_none() {
        assert!(get_pricing("unknown/model").is_none());
    }

    #[test]
    fn estimate_cost_simple() {
        let usage = CanonicalUsage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            ..Default::default()
        };
        let result = estimate_cost(&usage, "openai/gpt-4o");
        assert_eq!(result.status, CostStatus::Estimated);
        // 1M * $2.5/M + 0.5M * $10/M = $2.5 + $5.0 = $7.5
        let cost = result.amount_usd.unwrap();
        assert!((cost - 7.5).abs() < 0.01, "expected ~$7.50, got {cost}");
    }

    #[test]
    fn estimate_cost_copilot_is_included() {
        let usage = CanonicalUsage {
            input_tokens: 100_000,
            output_tokens: 50_000,
            ..Default::default()
        };
        let result = estimate_cost(&usage, "copilot/gpt-5-mini");
        assert_eq!(result.status, CostStatus::Included);
        assert_eq!(result.amount_usd, Some(0.0));
    }

    #[test]
    fn estimate_cost_unknown_model() {
        let usage = CanonicalUsage::default();
        let result = estimate_cost(&usage, "foo/bar");
        assert_eq!(result.status, CostStatus::Unknown);
        assert!(result.amount_usd.is_none());
    }

    #[test]
    fn estimate_cost_with_cache_tokens() {
        let usage = CanonicalUsage {
            input_tokens: 500_000,
            output_tokens: 100_000,
            cache_read_tokens: 200_000,
            ..Default::default()
        };
        let result = estimate_cost(&usage, "anthropic/claude-3-5-sonnet-20241022");
        assert_eq!(result.status, CostStatus::Estimated);
        // 0.5M * $3/M + 0.1M * $15/M + 0.2M * $0.3/M = $1.5 + $1.5 + $0.06 = $3.06
        let cost = result.amount_usd.unwrap();
        assert!((cost - 3.06).abs() < 0.01, "expected ~$3.06, got {cost}");
    }

    #[test]
    fn canonical_usage_total_tokens() {
        let u = CanonicalUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
            reasoning_tokens: 20,
        };
        assert_eq!(u.prompt_tokens(), 115);
        assert_eq!(u.total_tokens(), 185);
    }
}
