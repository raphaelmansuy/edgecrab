//! Offline + optional live citizenship tests for MTPLX first-class provider support.

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
use edgequake_llm::{
    MtplxProvider, ProviderFactory, list_cached_model_ids, resolve_mtplx_runtime_config,
};
use std::collections::HashMap;

#[test]
fn mtplx_is_local_family_member() {
    assert!(LOCAL_INFERENCE_PROVIDERS.contains(&"mtplx"));
    assert!(is_local_inference_provider("mtplx"));
    assert!(is_local_inference_provider("MTPLX"));
    let p = MtplxProvider::builder()
        .host("http://127.0.0.1:8000")
        .model("x")
        .build()
        .expect("build");
    assert!(prefers_nonstreaming_tool_turns(&p));
}

#[test]
fn catalog_has_mtplx_and_resolves_aliases() {
    let catalog = ModelCatalog::get();
    assert!(
        catalog.providers.contains_key("mtplx"),
        "catalog missing mtplx provider"
    );
    let resolved = ModelCatalog::resolve_spec_lenient("mtplx/Youssofal/Qwen3.6-27B")
        .expect("lenient resolve mtplx multi-segment");
    assert_eq!(resolved.runtime_provider, "mtplx");
    assert_eq!(ModelCatalog::catalog_provider_id("mtp-lx"), "mtplx");
}

#[test]
fn discovery_lists_mtplx() {
    let providers = live_discovery_providers();
    assert!(
        providers.iter().any(|p| p == "mtplx"),
        "live discovery missing mtplx: {providers:?}"
    );
    assert_eq!(normalize_discovery_provider("mtp_lx"), "mtplx");
}

#[test]
fn factory_builds_mtplx_provider() {
    let p = ProviderFactory::create_llm_provider("mtplx", "test-model").expect("create");
    assert_eq!(p.name(), "mtplx");
    assert_eq!(p.model(), "test-model");
}

#[test]
fn endpoint_registry_includes_mtplx() {
    assert!(PROVIDER_ENDPOINT_SPECS.iter().any(|s| s.id == "mtplx"));
    let s = endpoint_spec("mtplx").unwrap();
    assert!(s.local);
    assert_eq!(s.default_base_url, "http://127.0.0.1:8000");
    assert!(normalize_base_url("http://127.0.0.1:8002/v1").is_ok());
    let mut map = HashMap::new();
    map.insert(
        "mtplx".into(),
        ProviderEndpointConfig {
            base_url: Some("http://127.0.0.1:8002".into()),
        },
    );
    let (url, src) = resolve_endpoint("mtplx", &map).unwrap();
    assert_eq!(url, "http://127.0.0.1:8002");
    assert_eq!(src.label(), "config");
}

#[test]
fn runtime_config_reads_settings_or_default() {
    let cfg = resolve_mtplx_runtime_config();
    assert!(
        cfg.host.starts_with("http://") || cfg.host.starts_with("https://"),
        "host={}",
        cfg.host
    );
    // Settings on this machine often use 8002; default is 8000.
    eprintln!(
        "mtplx resolved host={} model={:?}",
        cfg.host, cfg.default_model
    );
}

#[test]
fn merge_drops_default_when_live_mtplx() {
    let static_cat = vec![("mtplx".into(), vec!["default".into()])];
    let dynamic = [ProviderModels {
        provider: "mtplx".into(),
        models: vec!["Youssofal/Qwen3.6-27B".into()],
        source: DiscoverySource::Live,
    }];
    let merged = merge_grouped_catalog_with_dynamic(&static_cat, &dynamic);
    let models = merged
        .iter()
        .find(|(p, _)| p == "mtplx")
        .map(|(_, m)| m.as_slice())
        .unwrap_or(&[]);
    assert!(!models.iter().any(|m| m == "default"));
    assert!(models.iter().any(|m| m.contains("Qwen")));
}

#[test]
fn fs_cache_list_does_not_panic() {
    let _ = list_cached_model_ids();
}

/// Live/cache discovery (empty when daemon down is OK if cache has models).
#[tokio::test]
async fn live_or_cache_mtplx_lists_models() {
    let cfg = resolve_mtplx_runtime_config();
    let pm = discover_provider_models("mtplx").await;
    eprintln!(
        "mtplx discovery @ {} → {} model(s) source={:?}: {:?}",
        cfg.host,
        pm.models.len(),
        pm.source,
        pm.models
    );
    // Soft: if nothing available, don't fail CI.
    if pm.models.is_empty() {
        eprintln!("no mtplx models (start `mtplx quickstart` or populate ~/.mtplx/models)");
        return;
    }
    assert!(pm.models.iter().all(|m| !m.is_empty()));
}
