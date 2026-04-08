//! Dynamic provider model discovery.
//!
//! WHY this module lives in `edgecrab-core`:
//! - discovery policy is domain logic, not TUI logic
//! - the CLI, setup flows, and future gateway/admin surfaces should share it
//! - the embedded catalog remains the fallback source of truth

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_trait::async_trait;
use edgecrab_tools::build_copilot_provider;
use edgequake_llm::providers::gemini::GeminiModelsResponse;
use edgequake_llm::{
    CopilotModel, CopilotModelsResponse, GeminiProvider, LMStudioProvider, OllamaProvider,
    OpenRouterProvider,
};
use futures::future::join_all;
use serde::{Deserialize, Serialize};

use crate::{ModelCatalog, edgecrab_home};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    Live,
    Cache,
    Static,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryAvailability {
    Supported,
    FeatureGated(&'static str),
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderModels {
    pub provider: String,
    pub models: Vec<String>,
    pub source: DiscoverySource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderCacheEntry {
    updated_at: i64,
    #[serde(default)]
    models: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DiscoveryCache {
    #[serde(default)]
    providers: BTreeMap<String, ProviderCacheEntry>,
}

#[async_trait]
trait ModelDiscoveryAdapter: Sync {
    fn canonical_name(&self) -> &'static str;
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }
    fn cache_ttl(&self) -> Duration;
    async fn fetch_models(&self) -> anyhow::Result<Vec<String>>;
}

struct OpenRouterDiscovery;
struct OllamaDiscovery;
struct LMStudioDiscovery;
struct GeminiDiscovery;
struct CopilotDiscovery;

#[cfg(feature = "bedrock-model-discovery")]
struct BedrockDiscovery;

static OPENROUTER_DISCOVERY: OpenRouterDiscovery = OpenRouterDiscovery;
static OLLAMA_DISCOVERY: OllamaDiscovery = OllamaDiscovery;
static LMSTUDIO_DISCOVERY: LMStudioDiscovery = LMStudioDiscovery;
static GEMINI_DISCOVERY: GeminiDiscovery = GeminiDiscovery;
static COPILOT_DISCOVERY: CopilotDiscovery = CopilotDiscovery;

#[cfg(feature = "bedrock-model-discovery")]
static BEDROCK_DISCOVERY: BedrockDiscovery = BedrockDiscovery;

const LOCAL_CACHE_TTL_SECS: u64 = 60;
const REMOTE_CACHE_TTL_SECS: u64 = 1800;
#[cfg(feature = "bedrock-model-discovery")]
const BEDROCK_CACHE_TTL_SECS: u64 = 900;
const FEATURE_GATED_DISCOVERY_PROVIDERS: &[(&str, &str)] =
    &[("bedrock", "bedrock-model-discovery")];

fn adapters() -> Vec<&'static dyn ModelDiscoveryAdapter> {
    #[allow(unused_mut)]
    let mut adapters: Vec<&'static dyn ModelDiscoveryAdapter> = vec![
        &OPENROUTER_DISCOVERY,
        &OLLAMA_DISCOVERY,
        &LMSTUDIO_DISCOVERY,
        &GEMINI_DISCOVERY,
        &COPILOT_DISCOVERY,
    ];
    #[cfg(feature = "bedrock-model-discovery")]
    {
        adapters.push(&BEDROCK_DISCOVERY);
    }
    adapters
}

pub fn normalize_discovery_provider(provider: &str) -> String {
    let provider = provider.trim().to_ascii_lowercase();
    let canonical = match provider.as_str() {
        "gemini" => "google".to_string(),
        "copilot" | "vscode-copilot" | "vscode" => "copilot".to_string(),
        "lm-studio" | "lm_studio" => "lmstudio".to_string(),
        "open-router" => "openrouter".to_string(),
        "aws-bedrock" | "aws_bedrock" | "aws bedrock" => "bedrock".to_string(),
        other => other.to_string(),
    };

    for adapter in adapters() {
        if adapter.canonical_name() == canonical || adapter.aliases().contains(&canonical.as_str())
        {
            return adapter.canonical_name().to_string();
        }
    }

    canonical
}

pub fn live_discovery_providers() -> Vec<String> {
    adapters()
        .into_iter()
        .map(|adapter| adapter.canonical_name().to_string())
        .collect()
}

pub fn discovery_provider_statuses() -> Vec<(String, DiscoveryAvailability)> {
    let mut statuses: BTreeMap<String, DiscoveryAvailability> = adapters()
        .into_iter()
        .map(|adapter| {
            (
                adapter.canonical_name().to_string(),
                DiscoveryAvailability::Supported,
            )
        })
        .collect();

    for (provider, feature) in FEATURE_GATED_DISCOVERY_PROVIDERS {
        statuses
            .entry((*provider).to_string())
            .or_insert(DiscoveryAvailability::FeatureGated(feature));
    }

    statuses.into_iter().collect()
}

pub fn live_discovery_availability(provider: &str) -> DiscoveryAvailability {
    let canonical = normalize_discovery_provider(provider);
    if adapters()
        .into_iter()
        .any(|adapter| adapter.canonical_name() == canonical)
    {
        return DiscoveryAvailability::Supported;
    }

    if let Some((_, feature)) = FEATURE_GATED_DISCOVERY_PROVIDERS
        .iter()
        .find(|(provider_name, _)| *provider_name == canonical)
    {
        return DiscoveryAvailability::FeatureGated(feature);
    }

    DiscoveryAvailability::Unsupported
}

pub async fn discover_provider_models(provider: &str) -> ProviderModels {
    let provider = normalize_discovery_provider(provider);
    if provider.is_empty() {
        return ProviderModels {
            provider,
            models: Vec::new(),
            source: DiscoverySource::Static,
        };
    }

    if let Some(adapter) = adapters()
        .into_iter()
        .find(|adapter| adapter.canonical_name() == provider)
    {
        match adapter.fetch_models().await {
            Ok(mut live) => {
                dedupe_sort(&mut live);
                let _ = write_provider_cache(&provider, &live);
                return ProviderModels {
                    provider,
                    models: live,
                    source: DiscoverySource::Live,
                };
            }
            Err(error) => {
                tracing::debug!(
                    provider,
                    error = %error,
                    "dynamic model discovery failed, checking cache"
                );
            }
        }

        if let Some(mut cached) = read_provider_cache(&provider, adapter.cache_ttl()) {
            dedupe_sort(&mut cached);
            return ProviderModels {
                provider,
                models: cached,
                source: DiscoverySource::Cache,
            };
        }
    }

    let mut fallback: Vec<String> = ModelCatalog::models_for_provider(&provider)
        .into_iter()
        .map(|(_, model)| model)
        .collect();
    dedupe_sort(&mut fallback);

    ProviderModels {
        provider,
        models: fallback,
        source: DiscoverySource::Static,
    }
}

pub async fn discover_multiple(providers: &[String]) -> Vec<ProviderModels> {
    let mut unique = Vec::new();
    for provider in providers {
        let canonical = normalize_discovery_provider(provider);
        if !canonical.is_empty() && !unique.iter().any(|seen: &String| seen == &canonical) {
            unique.push(canonical);
        }
    }

    join_all(
        unique
            .into_iter()
            .map(|provider| async move { discover_provider_models(&provider).await }),
    )
    .await
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
        .map(|(provider, models)| (provider, models.into_iter().collect()))
        .collect()
}

fn dedupe_sort(models: &mut Vec<String>) {
    models.sort();
    models.dedup();
}

fn cache_path() -> PathBuf {
    edgecrab_home().join("model_discovery_cache.json")
}

fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => 0,
    }
}

fn read_provider_cache(provider: &str, ttl: Duration) -> Option<Vec<String>> {
    let cache_content = std::fs::read_to_string(cache_path()).ok()?;
    let cache: DiscoveryCache = serde_json::from_str(&cache_content).ok()?;
    let entry = cache.providers.get(provider)?;

    let max_age = ttl.as_secs().min(i64::MAX as u64) as i64;
    if unix_now().saturating_sub(entry.updated_at) > max_age {
        return None;
    }

    Some(entry.models.clone())
}

fn write_provider_cache(provider: &str, models: &[String]) -> anyhow::Result<()> {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cache = if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<DiscoveryCache>(&content).ok())
            .unwrap_or_default()
    } else {
        DiscoveryCache::default()
    };

    cache.providers.insert(
        provider.to_string(),
        ProviderCacheEntry {
            updated_at: unix_now(),
            models: models.to_vec(),
        },
    );

    let serialized = serde_json::to_string_pretty(&cache)?;
    std::fs::write(path, serialized)?;
    Ok(())
}

fn parse_openai_models_payload(payload: &str) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct OpenAIModel {
        id: String,
    }

    #[derive(Deserialize)]
    struct OpenAIModelsResponse {
        data: Vec<OpenAIModel>,
    }

    let response: OpenAIModelsResponse = serde_json::from_str(payload)
        .context("failed to parse openai-compatible models payload")?;
    Ok(response.data.into_iter().map(|model| model.id).collect())
}

#[cfg(test)]
fn parse_ollama_models_payload(payload: &str) -> anyhow::Result<Vec<String>> {
    #[derive(Deserialize)]
    struct OllamaTagModel {
        name: Option<String>,
        model: Option<String>,
    }

    #[derive(Deserialize)]
    struct OllamaTags {
        models: Vec<OllamaTagModel>,
    }

    let response: OllamaTags =
        serde_json::from_str(payload).context("failed to parse ollama models payload")?;
    Ok(response
        .models
        .into_iter()
        .filter_map(|model| model.name.or(model.model))
        .collect())
}

fn extract_gemini_models(response: GeminiModelsResponse) -> Vec<String> {
    response
        .models
        .into_iter()
        .filter(|model| {
            model
                .supported_generation_methods
                .iter()
                .any(|method: &String| {
                    matches!(method.as_str(), "generateContent" | "streamGenerateContent")
                })
        })
        .map(|model| {
            model
                .name
                .strip_prefix("models/")
                .unwrap_or(model.name.as_str())
                .to_string()
        })
        .collect()
}

fn extract_ollama_models(response: edgequake_llm::OllamaModelsResponse) -> Vec<String> {
    response
        .models
        .into_iter()
        .map(|model| {
            if model.name.trim().is_empty() {
                model.model
            } else {
                model.name
            }
        })
        .collect()
}

fn extract_copilot_models(response: CopilotModelsResponse) -> Vec<String> {
    response
        .data
        .into_iter()
        .filter(copilot_model_is_selectable)
        .map(|model| model.id)
        .collect()
}

fn copilot_model_is_selectable(model: &CopilotModel) -> bool {
    let picker_enabled = model.model_picker_enabled.unwrap_or(true);
    let is_chat = model
        .capabilities
        .as_ref()
        .and_then(|capabilities| capabilities.model_type.as_deref())
        .map(|model_type| model_type == "chat")
        .unwrap_or(true);

    picker_enabled && is_chat
}

async fn fetch_openai_compatible_models(
    base_url: &str,
    api_key: Option<&str>,
) -> anyhow::Result<Vec<String>> {
    let mut base = base_url.trim_end_matches('/').to_string();
    if !base.ends_with("/v1") {
        base = format!("{base}/v1");
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()?;
    let mut request = client.get(format!("{base}/models"));
    if let Some(api_key) = api_key {
        if !api_key.trim().is_empty() {
            request = request.bearer_auth(api_key);
        }
    }

    let payload = request.send().await?.error_for_status()?.text().await?;
    parse_openai_models_payload(&payload)
}

#[async_trait]
impl ModelDiscoveryAdapter for OpenRouterDiscovery {
    fn canonical_name(&self) -> &'static str {
        "openrouter"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["open-router"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(REMOTE_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let provider = OpenRouterProvider::from_env()?;
        Ok(provider
            .list_models()
            .await?
            .into_iter()
            .map(|model| model.id)
            .collect())
    }
}

#[async_trait]
impl ModelDiscoveryAdapter for OllamaDiscovery {
    fn canonical_name(&self) -> &'static str {
        "ollama"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["ollama-host"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(LOCAL_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let provider = OllamaProvider::from_env()?;
        Ok(extract_ollama_models(provider.list_models().await?))
    }
}

#[async_trait]
impl ModelDiscoveryAdapter for LMStudioDiscovery {
    fn canonical_name(&self) -> &'static str {
        "lmstudio"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["lm-studio", "lm_studio"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(LOCAL_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let _provider = LMStudioProvider::from_env()?;
        let base_url = std::env::var("LMSTUDIO_BASE_URL")
            .or_else(|_| std::env::var("LMSTUDIO_HOST"))
            .unwrap_or_else(|_| "http://127.0.0.1:1234".to_string());
        fetch_openai_compatible_models(&base_url, None).await
    }
}

#[async_trait]
impl ModelDiscoveryAdapter for GeminiDiscovery {
    fn canonical_name(&self) -> &'static str {
        "google"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["gemini"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(REMOTE_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let provider = GeminiProvider::from_env()?;
        let response = provider.list_models().await?;
        Ok(extract_gemini_models(response))
    }
}

#[async_trait]
impl ModelDiscoveryAdapter for CopilotDiscovery {
    fn canonical_name(&self) -> &'static str {
        "copilot"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["vscode-copilot", "vscode"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(REMOTE_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let provider = build_copilot_provider("gpt-4o-mini", false).map_err(anyhow::Error::msg)?;
        let response = provider.list_models().await?;
        Ok(extract_copilot_models(response))
    }
}

#[cfg(feature = "bedrock-model-discovery")]
#[async_trait]
impl ModelDiscoveryAdapter for BedrockDiscovery {
    fn canonical_name(&self) -> &'static str {
        "bedrock"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["aws-bedrock", "aws_bedrock", "aws bedrock"]
    }

    fn cache_ttl(&self) -> Duration {
        Duration::from_secs(BEDROCK_CACHE_TTL_SECS)
    }

    async fn fetch_models(&self) -> anyhow::Result<Vec<String>> {
        let config = aws_config::load_from_env().await;
        let client = aws_sdk_bedrock::Client::new(&config);
        let response = client.list_foundation_models().send().await?;

        let mut models = Vec::new();
        for summary in response.model_summaries() {
            let output_modalities = summary
                .output_modalities()
                .iter()
                .map(|modality| modality.as_str())
                .collect::<Vec<_>>();
            if !output_modalities.contains(&"TEXT") {
                continue;
            }

            models.push(summary.model_id().to_string());
        }

        Ok(models)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn with_edgecrab_home<T>(test: impl FnOnce() -> T) -> T {
        let _guard = env_lock().lock().expect("env lock");
        let tempdir = TempDir::new().expect("temp dir");
        let original = std::env::var("EDGECRAB_HOME").ok();
        // SAFETY: tests in this module are single-threaded and restore the env var.
        unsafe { std::env::set_var("EDGECRAB_HOME", tempdir.path()) };
        let result = test();
        match original {
            Some(value) => unsafe { std::env::set_var("EDGECRAB_HOME", value) },
            None => unsafe { std::env::remove_var("EDGECRAB_HOME") },
        }
        result
    }

    #[test]
    fn normalize_provider_aliases() {
        assert_eq!(normalize_discovery_provider("gemini"), "google");
        assert_eq!(normalize_discovery_provider("vscode-copilot"), "copilot");
        assert_eq!(normalize_discovery_provider("lm-studio"), "lmstudio");
        assert_eq!(normalize_discovery_provider("aws-bedrock"), "bedrock");
    }

    #[test]
    fn live_discovery_provider_list_is_stable() {
        let providers = live_discovery_providers();
        assert!(providers.contains(&"openrouter".to_string()));
        assert!(providers.contains(&"ollama".to_string()));
        assert!(providers.contains(&"lmstudio".to_string()));
        assert!(providers.contains(&"google".to_string()));
        assert!(providers.contains(&"copilot".to_string()));
    }

    #[test]
    fn discovery_statuses_include_feature_gated_providers() {
        let statuses = discovery_provider_statuses();
        assert!(statuses.iter().any(|(provider, _)| provider == "bedrock"));
    }

    #[test]
    fn merge_grouped_catalog_keeps_unique_models() {
        let merged = merge_grouped_catalog_with_dynamic(
            &[("ollama".to_string(), vec!["llama3".to_string()])],
            &[ProviderModels {
                provider: "ollama".to_string(),
                models: vec!["llama3".to_string(), "qwen3".to_string()],
                source: DiscoverySource::Live,
            }],
        );
        assert_eq!(
            merged,
            vec![(
                "ollama".to_string(),
                vec!["llama3".to_string(), "qwen3".to_string()]
            )]
        );
    }

    #[test]
    fn cache_round_trip_is_per_provider() {
        with_edgecrab_home(|| {
            write_provider_cache("ollama", &["qwen3".to_string()]).expect("write cache");
            write_provider_cache("openrouter", &["anthropic/claude-4".to_string()])
                .expect("write cache");

            assert_eq!(
                read_provider_cache("ollama", Duration::from_secs(60)),
                Some(vec!["qwen3".to_string()])
            );
            assert_eq!(
                read_provider_cache("openrouter", Duration::from_secs(60)),
                Some(vec!["anthropic/claude-4".to_string()])
            );
        });
    }

    #[test]
    fn expired_cache_is_ignored() {
        with_edgecrab_home(|| {
            let cache = DiscoveryCache {
                providers: BTreeMap::from([(
                    "ollama".to_string(),
                    ProviderCacheEntry {
                        updated_at: unix_now() - 120,
                        models: vec!["qwen3".to_string()],
                    },
                )]),
            };
            let path = cache_path();
            std::fs::create_dir_all(path.parent().expect("cache parent")).expect("mkdirs");
            std::fs::write(
                path,
                serde_json::to_string_pretty(&cache).expect("serialize cache"),
            )
            .expect("write cache");

            assert_eq!(read_provider_cache("ollama", Duration::from_secs(10)), None);
        });
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        with_edgecrab_home(|| {
            let path = cache_path();
            std::fs::create_dir_all(path.parent().expect("cache parent")).expect("mkdirs");
            std::fs::write(path, "{not-json").expect("write corrupt cache");
            assert_eq!(read_provider_cache("ollama", Duration::from_secs(60)), None);
        });
    }

    #[test]
    fn parses_openai_compatible_models() {
        let payload = r#"{"data":[{"id":"gpt-4.1"},{"id":"gpt-4.1-mini"}]}"#;
        assert_eq!(
            parse_openai_models_payload(payload).expect("parse payload"),
            vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()]
        );
    }

    #[test]
    fn parses_ollama_models() {
        let payload =
            r#"{"models":[{"name":"qwen3:latest"},{"model":"llama3.2:3b"},{"name":null}]}"#;
        assert_eq!(
            parse_ollama_models_payload(payload).expect("parse payload"),
            vec!["qwen3:latest".to_string(), "llama3.2:3b".to_string()]
        );
    }

    #[test]
    fn filters_gemini_models_to_generation_capable_entries() {
        let response = GeminiModelsResponse {
            models: vec![
                edgequake_llm::providers::gemini::GeminiModelInfo {
                    name: "models/gemini-2.5-flash".to_string(),
                    display_name: String::new(),
                    description: String::new(),
                    input_token_limit: None,
                    output_token_limit: None,
                    supported_generation_methods: vec!["generateContent".to_string()],
                },
                edgequake_llm::providers::gemini::GeminiModelInfo {
                    name: "models/text-embedding-004".to_string(),
                    display_name: String::new(),
                    description: String::new(),
                    input_token_limit: None,
                    output_token_limit: None,
                    supported_generation_methods: vec!["embedContent".to_string()],
                },
            ],
        };
        assert_eq!(
            extract_gemini_models(response),
            vec!["gemini-2.5-flash".to_string()]
        );
    }

    #[test]
    fn filters_copilot_models_to_chat_picker_entries() {
        let response: CopilotModelsResponse = serde_json::from_value(serde_json::json!({
            "data": [
                {
                    "id": "gpt-4.1",
                    "model_picker_enabled": true,
                    "capabilities": { "type": "chat" }
                },
                {
                    "id": "text-embedding-3-small",
                    "model_picker_enabled": true,
                    "capabilities": { "type": "embedding" }
                },
                {
                    "id": "disabled-chat",
                    "model_picker_enabled": false,
                    "capabilities": { "type": "chat" }
                }
            ]
        }))
        .expect("copilot response");

        assert_eq!(
            extract_copilot_models(response),
            vec!["gpt-4.1".to_string()]
        );
    }

    #[test]
    fn unknown_provider_is_unsupported() {
        assert_eq!(
            live_discovery_availability("does-not-exist"),
            DiscoveryAvailability::Unsupported
        );
    }
}
