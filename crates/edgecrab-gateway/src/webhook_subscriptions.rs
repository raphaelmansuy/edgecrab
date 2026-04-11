use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, bail};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookSubscription {
    pub name: String,
    pub description: String,
    pub events: Vec<String>,
    pub secret: String,
    pub prompt: String,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub deliver: String,
    #[serde(default)]
    pub deliver_extra: BTreeMap<String, String>,
    #[serde(default = "default_rate_limit_per_minute")]
    pub rate_limit_per_minute: u32,
    #[serde(default = "default_max_body_bytes")]
    pub max_body_bytes: usize,
    pub created_at: String,
}

impl WebhookSubscription {
    pub fn normalized_events(&self) -> Vec<String> {
        self.events
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect()
    }

    pub fn accepts_event(&self, event: Option<&str>) -> bool {
        let filters = self.normalized_events();
        if filters.is_empty() {
            return true;
        }
        let Some(event) = event else {
            return false;
        };
        let event = event.trim().to_ascii_lowercase();
        filters.iter().any(|candidate| candidate == &event)
    }
}

pub struct CreateSubscriptionParams<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub events: &'a [String],
    pub prompt: Option<&'a str>,
    pub skills: &'a [String],
    pub secret: String,
    pub deliver: Option<&'a str>,
    pub deliver_extra: BTreeMap<String, String>,
    pub rate_limit_per_minute: Option<u32>,
    pub max_body_bytes: Option<usize>,
}

pub fn subscriptions_path() -> PathBuf {
    edgecrab_core::edgecrab_home().join("webhook_subscriptions.json")
}

pub fn load_subscriptions() -> anyhow::Result<BTreeMap<String, WebhookSubscription>> {
    let path = subscriptions_path();
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(parsed)
}

pub fn save_subscriptions(
    subscriptions: &BTreeMap<String, WebhookSubscription>,
) -> anyhow::Result<()> {
    let path = subscriptions_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(subscriptions)?;
    std::fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub fn normalize_subscription_name(raw: &str) -> anyhow::Result<String> {
    let name = raw.trim().to_ascii_lowercase().replace(' ', "-");
    if name.is_empty() {
        bail!("webhook name cannot be empty");
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        bail!("invalid webhook name '{raw}' (use lowercase letters, digits, '-' or '_')");
    }
    Ok(name)
}

pub fn create_subscription(
    params: CreateSubscriptionParams<'_>,
) -> anyhow::Result<WebhookSubscription> {
    let name = normalize_subscription_name(params.name)?;
    Ok(WebhookSubscription {
        name,
        description: params
            .description
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("EdgeCrab dynamic webhook subscription")
            .to_string(),
        events: params
            .events
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect(),
        secret: params.secret,
        prompt: params
            .prompt
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_default()
            .to_string(),
        skills: params
            .skills
            .iter()
            .flat_map(|value| value.split(','))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect(),
        deliver: params.deliver.unwrap_or("log").trim().to_string(),
        deliver_extra: params.deliver_extra,
        rate_limit_per_minute: params
            .rate_limit_per_minute
            .unwrap_or_else(default_rate_limit_per_minute),
        max_body_bytes: params.max_body_bytes.unwrap_or_else(default_max_body_bytes),
        created_at: Utc::now().to_rfc3339(),
    })
}

pub fn verify_signature(secret: &str, payload: &str, provided: Option<&str>) -> bool {
    if secret.trim().is_empty() {
        return true;
    }
    if secret.trim() == insecure_no_auth_secret() {
        return true;
    }
    let Some(provided) = provided.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    if let Some(value) = provided.strip_prefix("sha256=") {
        return hmac_sha256_hex(secret.as_bytes(), payload.as_bytes()) == value;
    }
    if provided.len() == 64 && provided.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return hmac_sha256_hex(secret.as_bytes(), payload.as_bytes()) == provided;
    }
    provided == secret
}

pub fn hmac_sha256_header(secret: &str, payload: &str) -> String {
    format!(
        "sha256={}",
        hmac_sha256_hex(secret.as_bytes(), payload.as_bytes())
    )
}

pub fn base_url(config: &edgecrab_core::config::GatewayConfig) -> String {
    let host = if config.host == "0.0.0.0" {
        "localhost"
    } else {
        config.host.as_str()
    };
    format!("http://{host}:{}", config.port)
}

fn hmac_sha256_hex(key: &[u8], data: &[u8]) -> String {
    const BLOCK_SIZE: usize = 64;

    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let digest = Sha256::digest(key);
        key_block[..digest.len()].copy_from_slice(&digest);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for idx in 0..BLOCK_SIZE {
        ipad[idx] ^= key_block[idx];
        opad[idx] ^= key_block[idx];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let digest = outer.finalize();

    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub const fn insecure_no_auth_secret() -> &'static str {
    "_INSECURE_NO_AUTH"
}

pub const fn default_rate_limit_per_minute() -> u32 {
    30
}

pub const fn default_max_body_bytes() -> usize {
    1_048_576
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_names() {
        assert_eq!(normalize_subscription_name("My Hook").unwrap(), "my-hook");
        assert!(normalize_subscription_name("bad/name").is_err());
    }

    #[test]
    fn verifies_hmac_signature() {
        let header = hmac_sha256_header("secret", "{\"ok\":true}");
        assert!(verify_signature("secret", "{\"ok\":true}", Some(&header)));
        assert!(!verify_signature("secret", "{\"ok\":false}", Some(&header)));
    }

    #[test]
    fn event_filters_work() {
        let subscription = create_subscription(CreateSubscriptionParams {
            name: "demo",
            description: None,
            events: &["push".into(), "pull_request".into()],
            prompt: None,
            skills: &[],
            secret: "secret".into(),
            deliver: None,
            deliver_extra: BTreeMap::new(),
            rate_limit_per_minute: None,
            max_body_bytes: None,
        })
        .unwrap();
        assert!(subscription.accepts_event(Some("push")));
        assert!(!subscription.accepts_event(Some("issues")));
        assert!(!subscription.accepts_event(None));
    }

    #[test]
    fn insecure_no_auth_bypasses_signature_validation() {
        assert!(verify_signature(
            insecure_no_auth_secret(),
            "{\"ok\":true}",
            None
        ));
    }
}
