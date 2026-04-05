//! Dynamic model discovery with cache + static fallback.
//!
//! WHY this module exists:
//! - Fast local UX for `/models` and `/model` selector
//! - Best-effort live discovery from provider APIs
//! - Graceful fallback to embedded catalog when APIs are unavailable

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use edgecrab_core::{ModelCatalog, edgecrab_home};
use serde::{Deserialize, Serialize};

const CACHE_TTL_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    Live,
    Cache,
    Static,
}

#[derive(Debug, Clone)]
pub struct ProviderModels {
    pub provider: String,
    pub models: Vec<String>,
    pub source: DiscoverySource,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DiscoveryCache {
    updated_at: i64,
    providers: BTreeMap<String, Vec<String>>,
}

pub async fn discover_provider_models(provider: &str) -> ProviderModels {
    let provider = provider.trim().to_lowercase();
    if provider.is_empty() {
        return ProviderModels {
            provider,
            models: Vec::new(),
            source: DiscoverySource::Static,
        };
    }

    if let Ok(mut live) = fetch_provider_live_models(&provider).await {
        dedupe_sort(&mut live);
        if !live.is_empty() {
            let _ = write_provider_cache(&provider, &live);
            return ProviderModels {
                provider,
                models: live,
                source: DiscoverySource::Live,
            };
        }
    }

    if let Some(mut cached) = read_provider_cache(&provider) {
        dedupe_sort(&mut cached);
        if !cached.is_empty() {
            return ProviderModels {
                provider,
                models: cached,
                source: DiscoverySource::Cache,
            };
        }
    }

    let mut fallback: Vec<String> = ModelCatalog::models_for_provider(&provider)
        .into_iter()
        .map(|(_, m)| m)
        .collect();
    dedupe_sort(&mut fallback);

    ProviderModels {
        provider,
        models: fallback,
        source: DiscoverySource::Static,
    }
}

pub async fn discover_multiple(providers: &[String]) -> Vec<ProviderModels> {
    let mut out = Vec::new();
    for p in providers {
        out.push(discover_provider_models(p).await);
    }
    out
}

pub fn merge_grouped_catalog_with_dynamic(
    grouped: &[(String, Vec<String>)],
    dynamic: &[ProviderModels],
) -> Vec<(String, Vec<String>)> {
    let mut map: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for (provider, models) in grouped {
        let set = map.entry(provider.clone()).or_default();
        for model in models {
            set.insert(model.clone());
        }
    }

    for entry in dynamic {
        let set = map.entry(entry.provider.clone()).or_default();
        for model in &entry.models {
            set.insert(model.clone());
        }
    }

    map.into_iter()
        .map(|(provider, set)| (provider, set.into_iter().collect()))
        .collect()
}

fn dedupe_sort(models: &mut Vec<String>) {
    models.sort();
    models.dedup();
}

async fn fetch_provider_live_models(provider: &str) -> anyhow::Result<Vec<String>> {
    match provider {
        "ollama" => fetch_ollama_models().await,
        "openrouter" => {
            fetch_openai_compatible_models(
                "https://openrouter.ai/api/v1",
                std::env::var("OPENROUTER_API_KEY").ok(),
                vec![],
            )
            .await
        }
        "lmstudio" => {
            let base = std::env::var("LMSTUDIO_BASE_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234/v1".to_string());
            fetch_openai_compatible_models(&base, None, vec![]).await
        }
        _ => {
            if let Some((base, key_env)) = provider_openai_compatible_config(provider) {
                let key = key_env.and_then(|name| std::env::var(name).ok());
                fetch_openai_compatible_models(base, key, vec![]).await
            } else {
                Ok(Vec::new())
            }
        }
    }
}

fn provider_openai_compatible_config(
    provider: &str,
) -> Option<(&'static str, Option<&'static str>)> {
    match provider {
        "openai" => Some(("https://api.openai.com/v1", Some("OPENAI_API_KEY"))),
        "deepseek" => Some(("https://api.deepseek.com/v1", Some("DEEPSEEK_API_KEY"))),
        "xai" => Some(("https://api.x.ai/v1", Some("XAI_API_KEY"))),
        "mistral" => Some(("https://api.mistral.ai/v1", Some("MISTRAL_API_KEY"))),
        "groq" => Some(("https://api.groq.com/openai/v1", Some("GROQ_API_KEY"))),
        _ => None,
    }
}

async fn fetch_ollama_models() -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct OllamaTagModel {
        name: Option<String>,
        model: Option<String>,
    }

    #[derive(Deserialize)]
    struct OllamaTags {
        models: Vec<OllamaTagModel>,
    }

    let base = std::env::var("OLLAMA_BASE_URL")
        .or_else(|_| std::env::var("OLLAMA_HOST"))
        .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    let url = format!("{}/api/tags", base.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()?;
    let payload: OllamaTags = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut out = Vec::new();
    for m in payload.models {
        if let Some(name) = m.name.or(m.model) {
            out.push(name);
        }
    }
    Ok(out)
}

async fn fetch_openai_compatible_models(
    base_url: &str,
    api_key: Option<String>,
    extra_headers: Vec<(&str, &str)>,
) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct OpenAIModel {
        id: String,
    }

    #[derive(Deserialize)]
    struct OpenAIModelsResponse {
        data: Vec<OpenAIModel>,
    }

    let mut base = base_url.trim_end_matches('/').to_string();
    if !base.ends_with("/v1") {
        base = format!("{base}/v1");
    }
    let url = format!("{base}/models");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()?;

    let mut req = client.get(url);
    if let Some(key) = api_key {
        if !key.trim().is_empty() {
            req = req.bearer_auth(key);
        }
    }
    for (k, v) in extra_headers {
        req = req.header(k, v);
    }

    let payload: OpenAIModelsResponse = req.send().await?.error_for_status()?.json().await?;
    Ok(payload.data.into_iter().map(|m| m.id).collect())
}

fn cache_path() -> PathBuf {
    edgecrab_home().join("model_discovery_cache.json")
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

fn read_provider_cache(provider: &str) -> Option<Vec<String>> {
    let path = cache_path();
    let content = std::fs::read_to_string(path).ok()?;
    let cache: DiscoveryCache = serde_json::from_str(&content).ok()?;

    if unix_now().saturating_sub(cache.updated_at) > CACHE_TTL_SECS {
        return None;
    }

    cache.providers.get(provider).cloned()
}

fn write_provider_cache(provider: &str, models: &[String]) -> anyhow::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cache = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<DiscoveryCache>(&s).ok())
            .unwrap_or_default()
    } else {
        DiscoveryCache::default()
    };

    cache.updated_at = unix_now();
    cache
        .providers
        .insert(provider.to_string(), models.to_vec());

    let serialized = serde_json::to_string_pretty(&cache)?;
    std::fs::write(path, serialized)?;
    Ok(())
}
