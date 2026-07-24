//! Per-provider base URL overrides — single source of truth for host resolution.
//!
//! # Design (SOLID / DRY)
//!
//! - **S**: This module owns endpoint metadata (defaults, env keys, validation).
//! - **O**: New providers add one [`ProviderEndpointSpec`] row.
//! - **D**: Factory/TUI depend on this registry, not hard-coded port tables.
//!
//! Resolution order for effective base URL:
//! 1. Config override (`AppConfig.provider_endpoints.<id>.base_url`)
//! 2. Environment (`OMLX_HOST`, `OLLAMA_HOST`, …)
//! 3. Compiled default
//!
//! Applying overrides writes the primary env var so `edgequake_llm` factory
//! paths keep working without a parallel API.

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

use serde::{Deserialize, Serialize};

/// Persisted override for one provider.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderEndpointConfig {
    /// Base URL (with or without `/v1`). Empty / null clears override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// Static metadata for a provider that can have a base URL.
#[derive(Debug, Clone, Copy)]
pub struct ProviderEndpointSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub default_base_url: &'static str,
    /// Env vars checked in order (first set wins).
    pub env_keys: &'static [&'static str],
    /// Whether this is typically a local server (port probes, long timeout).
    pub local: bool,
    pub description: &'static str,
}

/// All providers that support a configurable base URL in the TUI.
pub const PROVIDER_ENDPOINT_SPECS: &[ProviderEndpointSpec] = &[
    ProviderEndpointSpec {
        id: "omlx",
        label: "oMLX",
        default_base_url: "http://127.0.0.1:9050",
        env_keys: &["OMLX_HOST", "OMLX_BASE_URL"],
        local: true,
        description: "Apple Silicon MLX server (menu bar · default :9050 · reads ~/.omlx/settings.json)",
    },
    ProviderEndpointSpec {
        id: "mtplx",
        label: "MTPLX",
        default_base_url: "http://127.0.0.1:8000",
        env_keys: &["MTPLX_HOST", "MTPLX_BASE_URL"],
        local: true,
        description: "Native MTP on Apple Silicon (settings.port · Application Support/MTPLX)",
    },
    ProviderEndpointSpec {
        id: "llamacpp",
        label: "llama-server",
        default_base_url: "http://127.0.0.1:8080",
        env_keys: &[
            "LLAMACPP_HOST",
            "LLAMA_SERVER_HOST",
            "LLAMACPP_BASE_URL",
            "LLAMA_SERVER_BASE_URL",
        ],
        local: true,
        description: "llama.cpp llama-server (Metal GGUF · default :8080)",
    },
    ProviderEndpointSpec {
        id: "vllm-mlx",
        label: "vLLM-MLX",
        default_base_url: "http://127.0.0.1:8000",
        env_keys: &["VLLM_MLX_HOST", "VLLM_MLX_BASE_URL"],
        local: true,
        description: "vLLM-MLX (MLX continuous batching · default :8000 · may share port with MTPLX)",
    },
    ProviderEndpointSpec {
        id: "mlx-lm",
        label: "mlx-lm",
        default_base_url: "http://127.0.0.1:8080",
        env_keys: &["MLX_LM_HOST", "MLX_LM_BASE_URL", "MLXLM_HOST"],
        local: true,
        description: "mlx_lm.server (official Apple MLX-LM · default :8080 · may share port with llama-server)",
    },
    ProviderEndpointSpec {
        id: "ollama",
        label: "Ollama",
        default_base_url: "http://127.0.0.1:11434",
        env_keys: &["OLLAMA_HOST", "OLLAMA_BASE_URL"],
        local: true,
        description: "Local Ollama daemon",
    },
    ProviderEndpointSpec {
        id: "lmstudio",
        label: "LM Studio",
        default_base_url: "http://127.0.0.1:1234",
        env_keys: &["LMSTUDIO_HOST", "LMSTUDIO_BASE_URL"],
        local: true,
        description: "LM Studio local server",
    },
    ProviderEndpointSpec {
        id: "openai",
        label: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
        env_keys: &["OPENAI_BASE_URL"],
        local: false,
        description: "OpenAI API or compatible gateway",
    },
    ProviderEndpointSpec {
        id: "anthropic",
        label: "Anthropic",
        default_base_url: "https://api.anthropic.com",
        env_keys: &["ANTHROPIC_BASE_URL"],
        local: false,
        description: "Anthropic Messages API or compatible gateway",
    },
    ProviderEndpointSpec {
        id: "openrouter",
        label: "OpenRouter",
        default_base_url: "https://openrouter.ai/api/v1",
        env_keys: &["OPENROUTER_BASE_URL"],
        local: false,
        description: "OpenRouter multi-model gateway",
    },
    ProviderEndpointSpec {
        id: "xai",
        label: "xAI",
        default_base_url: "https://api.x.ai/v1",
        env_keys: &["XAI_BASE_URL"],
        local: false,
        description: "xAI Grok API",
    },
    ProviderEndpointSpec {
        id: "mistral",
        label: "Mistral",
        default_base_url: "https://api.mistral.ai",
        env_keys: &["MISTRAL_BASE_URL"],
        local: false,
        description: "Mistral La Plateforme",
    },
    ProviderEndpointSpec {
        id: "gemini",
        label: "Gemini",
        default_base_url: "https://generativelanguage.googleapis.com",
        env_keys: &["GEMINI_BASE_URL", "GOOGLE_GEMINI_BASE_URL"],
        local: false,
        description: "Google AI Studio Gemini",
    },
    ProviderEndpointSpec {
        id: "deepseek",
        label: "DeepSeek",
        default_base_url: "https://api.deepseek.com",
        env_keys: &["DEEPSEEK_BASE_URL"],
        local: false,
        description: "DeepSeek API",
    },
    ProviderEndpointSpec {
        id: "groq",
        label: "Groq",
        default_base_url: "https://api.groq.com/openai",
        env_keys: &["GROQ_BASE_URL"],
        local: false,
        description: "Groq OpenAI-compatible API",
    },
    ProviderEndpointSpec {
        id: "nvidia",
        label: "NVIDIA NIM",
        default_base_url: "https://integrate.api.nvidia.com/v1",
        env_keys: &["NVIDIA_BASE_URL"],
        local: false,
        description: "NVIDIA NIM hosted inference",
    },
];

/// Where the effective URL came from (for TUI badges).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointSource {
    Config,
    Env,
    Default,
}

impl EndpointSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Env => "env",
            Self::Default => "default",
        }
    }
}

/// Lookup static spec by canonical id or alias.
pub fn endpoint_spec(provider: &str) -> Option<&'static ProviderEndpointSpec> {
    let key = provider.trim().to_ascii_lowercase();
    let key = match key.as_str() {
        "lm-studio" | "lm_studio" => "lmstudio",
        "o-mlx" | "o_mlx" => "omlx",
        "mtp-lx" | "mtp_lx" | "mtpl-x" => "mtplx",
        "open-router" => "openrouter",
        "grok" => "xai",
        "google" => "gemini",
        other => other,
    };
    PROVIDER_ENDPOINT_SPECS.iter().find(|s| s.id == key)
}

/// Normalize URL: trim, strip trailing slash (keep `/v1` if present as path).
pub fn normalize_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("URL is empty".into());
    }
    if trimmed.eq_ignore_ascii_case("clear") || trimmed.eq_ignore_ascii_case("default") {
        return Ok(String::new());
    }
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("URL must start with http:// or https://".into());
    }
    // Reject whitespace and control chars
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL must not contain whitespace".into());
    }
    let url = trimmed.trim_end_matches('/').to_string();
    // Validate host portion exists
    let without_scheme = url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(url.as_str());
    if without_scheme.is_empty() || without_scheme.starts_with('/') {
        return Err("URL is missing a host".into());
    }
    Ok(url)
}

/// Read env for a spec (first non-empty wins).
pub fn env_base_url(spec: &ProviderEndpointSpec) -> Option<String> {
    for key in spec.env_keys {
        if let Ok(val) = std::env::var(key) {
            let t = val.trim();
            if !t.is_empty() {
                return Some(t.trim_end_matches('/').to_string());
            }
        }
    }
    None
}

/// Resolve effective base URL + source.
pub fn resolve_endpoint(
    provider: &str,
    config_map: &HashMap<String, ProviderEndpointConfig>,
) -> Option<(String, EndpointSource)> {
    let spec = endpoint_spec(provider)?;
    if let Some(entry) = config_map.get(spec.id)
        && let Some(url) = entry.base_url.as_ref()
    {
        let t = url.trim();
        if !t.is_empty() {
            return Some((t.trim_end_matches('/').to_string(), EndpointSource::Config));
        }
    }
    if let Some(url) = env_base_url(spec) {
        return Some((url, EndpointSource::Env));
    }
    Some((
        spec.default_base_url.trim_end_matches('/').to_string(),
        EndpointSource::Default,
    ))
}

/// Process-wide config overrides (loaded from AppConfig; updated by TUI).
fn runtime_overrides() -> &'static RwLock<HashMap<String, String>> {
    static OVERRIDES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    OVERRIDES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Replace runtime overrides from config map (call on load / save).
pub fn load_endpoint_overrides(config_map: &HashMap<String, ProviderEndpointConfig>) {
    let mut map = HashMap::new();
    for (k, v) in config_map {
        if let Some(url) = v.base_url.as_ref() {
            let t = url.trim();
            if !t.is_empty() {
                map.insert(k.to_ascii_lowercase(), t.trim_end_matches('/').to_string());
            }
        }
    }
    if let Ok(mut guard) = runtime_overrides().write() {
        *guard = map;
    }
    apply_runtime_overrides_to_env();
}

/// Apply current runtime overrides into process env (primary key only).
pub fn apply_runtime_overrides_to_env() {
    let Ok(guard) = runtime_overrides().read() else {
        return;
    };
    for (provider, url) in guard.iter() {
        if let Some(spec) = endpoint_spec(provider)
            && let Some(key) = spec.env_keys.first()
        {
            // SAFETY: provider construction uses process-wide env; same pattern as Vertex.
            unsafe { std::env::set_var(key, url) };
        }
    }
}

/// Set or clear one override in runtime + return env key written.
pub fn set_runtime_override(provider: &str, base_url: Option<&str>) -> Result<(), String> {
    let spec = endpoint_spec(provider).ok_or_else(|| format!("Unknown provider: {provider}"))?;
    let mut guard = runtime_overrides()
        .write()
        .map_err(|_| "endpoint override lock poisoned".to_string())?;
    match base_url {
        None | Some("") => {
            guard.remove(spec.id);
            if let Some(key) = spec.env_keys.first() {
                unsafe { std::env::remove_var(key) };
            }
        }
        Some(url) => {
            let normalized = normalize_base_url(url)?;
            if normalized.is_empty() {
                guard.remove(spec.id);
                if let Some(key) = spec.env_keys.first() {
                    unsafe { std::env::remove_var(key) };
                }
            } else {
                guard.insert(spec.id.to_string(), normalized.clone());
                if let Some(key) = spec.env_keys.first() {
                    unsafe { std::env::set_var(key, &normalized) };
                }
            }
        }
    }
    Ok(())
}

/// Effective URL using runtime overrides then env then default.
pub fn effective_base_url(provider: &str) -> Option<(String, EndpointSource)> {
    let spec = endpoint_spec(provider)?;
    if let Ok(guard) = runtime_overrides().read()
        && let Some(url) = guard.get(spec.id)
        && !url.is_empty()
    {
        return Some((url.clone(), EndpointSource::Config));
    }
    if let Some(url) = env_base_url(spec) {
        return Some((url, EndpointSource::Env));
    }
    Some((
        spec.default_base_url.trim_end_matches('/').to_string(),
        EndpointSource::Default,
    ))
}

/// Probe `GET {base}/v1/models` (or base if already ends with /v1).
pub async fn probe_endpoint(base_url: &str, timeout_ms: u64) -> Result<String, String> {
    let base = base_url.trim().trim_end_matches('/');
    let url = if base.ends_with("/v1") {
        format!("{base}/models")
    } else {
        format!("{base}/v1/models")
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms.max(200)))
        .no_proxy()
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("unreachable: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
        let n = body
            .get("data")
            .and_then(|d| d.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        Ok(format!("ok · HTTP {status} · {n} model(s)"))
    } else if status.as_u16() == 401 || status.as_u16() == 403 {
        Ok(format!("reachable · HTTP {status} (auth required)"))
    } else {
        Err(format!("HTTP {status}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_http() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:8000/").unwrap(),
            "http://127.0.0.1:8000"
        );
    }

    #[test]
    fn normalize_rejects_bare_host() {
        assert!(normalize_base_url("127.0.0.1:8000").is_err());
    }

    #[test]
    fn clear_tokens() {
        assert_eq!(normalize_base_url("clear").unwrap(), "");
        assert_eq!(normalize_base_url("default").unwrap(), "");
    }

    #[test]
    fn omlx_spec_exists() {
        let s = endpoint_spec("omlx").expect("omlx");
        assert_eq!(s.default_base_url, "http://127.0.0.1:9050");
        assert!(s.local);
    }

    #[test]
    fn resolve_default() {
        let map = HashMap::new();
        let (url, src) = resolve_endpoint("omlx", &map).unwrap();
        assert_eq!(url, "http://127.0.0.1:9050");
        assert_eq!(src, EndpointSource::Default);
    }

    #[test]
    fn resolve_config_override() {
        let mut map = HashMap::new();
        map.insert(
            "omlx".into(),
            ProviderEndpointConfig {
                base_url: Some("http://192.168.1.5:8000".into()),
            },
        );
        let (url, src) = resolve_endpoint("omlx", &map).unwrap();
        assert_eq!(url, "http://192.168.1.5:8000");
        assert_eq!(src, EndpointSource::Config);
    }
}
