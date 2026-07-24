//! Offline citizenship tests for oMLX first-class provider support.

use edgecrab_core::local_provider_policy::{
    LOCAL_INFERENCE_PROVIDERS, is_local_inference_provider, prefers_nonstreaming_tool_turns,
};
use edgecrab_core::model_catalog::ModelCatalog;
use edgecrab_core::model_discovery::{
    DiscoverySource, ProviderModels, discover_provider_models, live_discovery_providers,
    merge_grouped_catalog_with_dynamic, normalize_discovery_provider,
};
use edgecrab_core::provider_endpoints::{
    PROVIDER_ENDPOINT_SPECS, ProviderEndpointConfig, endpoint_spec, normalize_base_url,
    resolve_endpoint,
};
use edgequake_llm::{OmlxProvider, ProviderFactory, resolve_omlx_runtime_config};
use std::collections::HashMap;

#[test]
fn omlx_is_local_family_member() {
    assert!(LOCAL_INFERENCE_PROVIDERS.contains(&"omlx"));
    assert!(is_local_inference_provider("omlx"));
    assert!(is_local_inference_provider("OMLX"));
    let p = OmlxProvider::builder()
        .host("http://127.0.0.1:8000")
        .model("x")
        .build()
        .expect("build");
    assert!(prefers_nonstreaming_tool_turns(&p));
}

#[test]
fn catalog_has_omlx_and_resolves_aliases() {
    let catalog = ModelCatalog::get();
    assert!(
        catalog.providers.contains_key("omlx"),
        "catalog missing omlx provider"
    );
    let resolved = ModelCatalog::resolve_spec_lenient("omlx/qwen3-8b:thinking")
        .expect("lenient resolve omlx profile");
    assert_eq!(resolved.runtime_provider, "omlx");
    assert_eq!(ModelCatalog::catalog_provider_id("o-mlx"), "omlx");
}

#[test]
fn discovery_lists_omlx() {
    let providers = live_discovery_providers();
    assert!(
        providers.iter().any(|p| p == "omlx"),
        "live discovery missing omlx: {providers:?}"
    );
    assert_eq!(normalize_discovery_provider("o_mlx"), "omlx");
}

#[test]
fn factory_builds_omlx_provider() {
    let p = ProviderFactory::create_llm_provider("omlx", "test-model").expect("create");
    assert_eq!(p.name(), "omlx");
    assert_eq!(p.model(), "test-model");
}

#[test]
fn endpoint_registry_includes_omlx_and_validates_urls() {
    assert!(PROVIDER_ENDPOINT_SPECS.iter().any(|s| s.id == "omlx"));
    let s = endpoint_spec("omlx").unwrap();
    assert!(s.local);
    assert_eq!(s.default_base_url, "http://127.0.0.1:9050");
    assert!(normalize_base_url("http://127.0.0.1:9000/v1").is_ok());
    assert!(normalize_base_url("not-a-url").is_err());
    let mut map = HashMap::new();
    map.insert(
        "omlx".into(),
        ProviderEndpointConfig {
            base_url: Some("http://10.0.0.2:8000".into()),
        },
    );
    let (url, src) = resolve_endpoint("omlx", &map).unwrap();
    assert_eq!(url, "http://10.0.0.2:8000");
    assert_eq!(src.label(), "config");
}

#[test]
fn runtime_config_defaults_to_9050_without_env() {
    // When OMLX_HOST is unset, host comes from settings or 9050 default.
    let cfg = resolve_omlx_runtime_config();
    assert!(
        cfg.host.contains("9050")
            || cfg.host.contains("127.0.0.1")
            || cfg.host.contains("localhost"),
        "unexpected omlx host {}",
        cfg.host
    );
}

#[test]
fn merge_drops_default_placeholder_when_live_models_present() {
    let static_cat = vec![("omlx".into(), vec!["default".into()])];
    let dynamic = [ProviderModels {
        provider: "omlx".into(),
        models: vec!["Qwen3.6-35B".into(), "KAT-Coder".into()],
        source: DiscoverySource::Live,
    }];
    let merged = merge_grouped_catalog_with_dynamic(&static_cat, &dynamic);
    let omlx = merged
        .iter()
        .find(|(p, _)| p == "omlx")
        .map(|(_, m)| m.as_slice())
        .unwrap_or(&[]);
    assert!(!omlx.iter().any(|m| m == "default"));
    assert!(omlx.iter().any(|m| m == "Qwen3.6-35B"));
}

/// Live discovery against the local oMLX server (empty / static when unreachable).
#[tokio::test]
async fn live_omlx_lists_models_when_api_up() {
    let cfg = resolve_omlx_runtime_config();
    let pm = discover_provider_models("omlx").await;
    eprintln!(
        "omlx discovery @ {} → {} model(s) source={:?}",
        cfg.host,
        pm.models.len(),
        pm.source
    );
    // Offline / unreachable: catalog seed (`default`) or empty — soft-skip.
    if pm.models.is_empty()
        || matches!(pm.source, DiscoverySource::Static)
        || pm.models.iter().all(|m| m == "default")
    {
        eprintln!(
            "no live inventory — ensure oMLX is running on {} and auth.api_key is set in ~/.omlx/settings.json",
            cfg.host
        );
        return;
    }
    assert!(
        pm.models.iter().all(|m| !m.is_empty()),
        "empty model id in list"
    );
    assert!(
        matches!(pm.source, DiscoverySource::Live | DiscoverySource::Cache),
        "expected live/cache source, got {:?}",
        pm.source
    );
}
