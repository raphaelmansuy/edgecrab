//! Offline citizenship tests for Wave D–F local Mac providers:
//! `llamacpp` (llama-server), `vllm-mlx`, `mlx-lm`.
//!
//! DRY: one parameterized suite for the shared LocalOpenAi identity family.
//! Live discovery is soft (no fail when daemon is down).

use edgecrab_core::local_provider_policy::{
    LOCAL_INFERENCE_PROVIDERS, is_local_inference_provider, prefers_nonstreaming_tool_turns,
};
use edgecrab_core::model_catalog::ModelCatalog;
use edgecrab_core::model_discovery::{
    DiscoverySource, ProviderModels, discover_provider_models, live_discovery_providers,
    merge_grouped_catalog_with_dynamic, normalize_discovery_provider,
};
use edgecrab_core::provider_endpoints::{
    PROVIDER_ENDPOINT_SPECS, ProviderEndpointConfig, endpoint_spec, resolve_endpoint,
};
use edgequake_llm::{
    LLMProvider, LocalOpenAiProvider, ProviderFactory, llamacpp_builder, mlx_lm_builder,
    resolve_llamacpp_runtime_config, resolve_mlx_lm_runtime_config,
    resolve_vllm_mlx_runtime_config, vllm_mlx_builder,
};
use std::collections::HashMap;

struct Citizen {
    id: &'static str,
    aliases: &'static [&'static str],
    default_port_substr: &'static str,
    build: fn() -> LocalOpenAiProvider,
}

fn citizens() -> &'static [Citizen] {
    &[
        Citizen {
            id: "llamacpp",
            aliases: &["llama-server", "llama.cpp"],
            default_port_substr: "8080",
            build: || {
                llamacpp_builder()
                    .model("test-model")
                    .build()
                    .expect("llamacpp build")
            },
        },
        Citizen {
            id: "vllm-mlx",
            aliases: &["vllm_mlx"],
            default_port_substr: "8000",
            build: || {
                vllm_mlx_builder()
                    .model("test-model")
                    .build()
                    .expect("vllm-mlx build")
            },
        },
        Citizen {
            id: "mlx-lm",
            aliases: &["mlx_lm"],
            default_port_substr: "8080",
            build: || {
                mlx_lm_builder()
                    .model("test-model")
                    .build()
                    .expect("mlx-lm build")
            },
        },
    ]
}

#[test]
fn local_family_includes_wave_def() {
    for c in citizens() {
        assert!(
            LOCAL_INFERENCE_PROVIDERS.contains(&c.id),
            "LOCAL_INFERENCE_PROVIDERS missing {}",
            c.id
        );
        assert!(is_local_inference_provider(c.id));
        assert!(is_local_inference_provider(&c.id.to_uppercase()));
        let p = (c.build)();
        assert!(prefers_nonstreaming_tool_turns(&p));
        assert_eq!(LLMProvider::name(&p), c.id);
    }
}

#[test]
fn catalog_has_each_and_resolves_aliases() {
    let catalog = ModelCatalog::get();
    for c in citizens() {
        assert!(
            catalog.providers.contains_key(c.id),
            "catalog missing {}",
            c.id
        );
        let resolved = ModelCatalog::resolve_spec_lenient(&format!("{}/default", c.id))
            .unwrap_or_else(|| panic!("lenient resolve {}", c.id));
        assert_eq!(resolved.runtime_provider, c.id);
        for alias in c.aliases {
            assert_eq!(
                ModelCatalog::catalog_provider_id(alias),
                c.id,
                "alias {alias} → {}",
                c.id
            );
        }
    }
}

#[test]
fn discovery_lists_each() {
    let providers = live_discovery_providers();
    for c in citizens() {
        assert!(
            providers.iter().any(|p| p == c.id),
            "live discovery missing {}: {providers:?}",
            c.id
        );
        for alias in c.aliases {
            assert_eq!(
                normalize_discovery_provider(alias),
                c.id,
                "discovery normalize {alias}"
            );
        }
    }
}

#[test]
fn factory_builds_each_provider() {
    for c in citizens() {
        let p = ProviderFactory::create_llm_provider(c.id, "citizenship-model").expect(c.id);
        assert_eq!(p.name(), c.id);
        assert_eq!(p.model(), "citizenship-model");
    }
}

#[test]
fn endpoint_registry_includes_each() {
    for c in citizens() {
        assert!(
            PROVIDER_ENDPOINT_SPECS.iter().any(|s| s.id == c.id),
            "endpoint specs missing {}",
            c.id
        );
        let s = endpoint_spec(c.id).unwrap();
        assert!(s.local);
        assert!(
            s.default_base_url.contains(c.default_port_substr),
            "{} default URL {}",
            c.id,
            s.default_base_url
        );
        let mut map = HashMap::new();
        map.insert(
            c.id.into(),
            ProviderEndpointConfig {
                base_url: Some("http://10.0.0.9:9999".into()),
            },
        );
        let (url, src) = resolve_endpoint(c.id, &map).unwrap();
        assert_eq!(url, "http://10.0.0.9:9999");
        assert_eq!(src.label(), "config");
    }
}

#[test]
fn runtime_configs_have_http_hosts() {
    for (id, host) in [
        ("llamacpp", resolve_llamacpp_runtime_config().host),
        ("vllm-mlx", resolve_vllm_mlx_runtime_config().host),
        ("mlx-lm", resolve_mlx_lm_runtime_config().host),
    ] {
        assert!(
            host.starts_with("http://") || host.starts_with("https://"),
            "{id} host={host}"
        );
    }
}

#[test]
fn merge_drops_default_when_live_present() {
    for c in citizens() {
        let static_cat = vec![(c.id.into(), vec!["default".into()])];
        let dynamic = [ProviderModels {
            provider: c.id.into(),
            models: vec!["live-model-a".into(), "org/live-b".into()],
            source: DiscoverySource::Live,
        }];
        let merged = merge_grouped_catalog_with_dynamic(&static_cat, &dynamic);
        let models = merged
            .iter()
            .find(|(p, _)| p == c.id)
            .map(|(_, m)| m.as_slice())
            .unwrap_or(&[]);
        assert!(
            !models.iter().any(|m| m == "default"),
            "{} kept default seed",
            c.id
        );
        assert!(models.iter().any(|m| m == "live-model-a"));
    }
}

/// Soft live discovery — empty when server down is OK for CI.
#[tokio::test]
async fn soft_live_discovery_for_wave_def() {
    for c in citizens() {
        let pm = discover_provider_models(c.id).await;
        eprintln!(
            "{} discovery → {} model(s) source={:?}: {:?}",
            c.id,
            pm.models.len(),
            pm.source,
            pm.models
        );
        if pm.models.is_empty() {
            continue;
        }
        assert!(pm.models.iter().all(|m| !m.is_empty()));
    }
}

/// Optional hard e2e: `LLAMACPP_E2E=1` requires a reachable llama-server.
#[tokio::test]
async fn llamacpp_e2e_when_enabled() {
    if std::env::var("LLAMACPP_E2E").ok().as_deref() != Some("1") {
        eprintln!("skip LLAMACPP_E2E (set LLAMACPP_E2E=1 to enforce live models)");
        return;
    }
    let pm = discover_provider_models("llamacpp").await;
    assert!(
        !pm.models.is_empty(),
        "LLAMACPP_E2E=1 but no models from {}",
        resolve_llamacpp_runtime_config().host
    );
}

#[tokio::test]
async fn vllm_mlx_e2e_when_enabled() {
    if std::env::var("VLLM_MLX_E2E").ok().as_deref() != Some("1") {
        eprintln!("skip VLLM_MLX_E2E");
        return;
    }
    let pm = discover_provider_models("vllm-mlx").await;
    assert!(!pm.models.is_empty(), "VLLM_MLX_E2E=1 but no models");
}

#[tokio::test]
async fn mlx_lm_e2e_when_enabled() {
    if std::env::var("MLX_LM_E2E").ok().as_deref() != Some("1") {
        eprintln!("skip MLX_LM_E2E");
        return;
    }
    let pm = discover_provider_models("mlx-lm").await;
    assert!(!pm.models.is_empty(), "MLX_LM_E2E=1 but no models");
}
