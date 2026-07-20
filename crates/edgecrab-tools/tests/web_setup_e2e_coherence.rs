#![allow(clippy::await_holding_lock)]
#![allow(dead_code)]
//! E2E: web setup chain coherence — chain vs extract, overlay subcommands.

mod common;

use common::registry_guard;
use edgecrab_tools::{
    WebSectionUpdate, default_chain_for_backend, ensure_web_search_config_coherence_at,
    persist_search_backend_as_chain, persist_search_chain_order, persist_web_section_in_config,
    search_section_override_from_path,
    tools::web::search::{load_web_search_config_from_path, load_web_tools_config_from_path},
    web_command_overlay,
};
use tempfile::TempDir;

#[test]
fn e2e_split_search_chain_and_extract_backend() {
    let _lock = registry_guard();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");

    persist_search_backend_as_chain(&path, "brave").expect("search chain");
    persist_web_section_in_config(
        &path,
        &WebSectionUpdate {
            backend: Some(String::new()),
            search_backend: Some(String::new()),
            extract_backend: Some("exa".into()),
        },
    )
    .expect("extract");

    let web = load_web_tools_config_from_path(&path).expect("web");
    assert!(web.search_backend.is_empty());
    assert_eq!(web.extract_backend, "exa");

    let search = load_web_search_config_from_path(&path).expect("search");
    assert_eq!(search.primary, "brave");
    assert_eq!(search.fallbacks, vec!["ddgs".to_string()]);
}

#[test]
fn e2e_persist_chain_does_not_wipe_extract() {
    let _lock = registry_guard();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");

    persist_web_section_in_config(
        &path,
        &WebSectionUpdate {
            backend: Some(String::new()),
            search_backend: Some(String::new()),
            extract_backend: Some("parallel".into()),
        },
    )
    .expect("extract seed");

    persist_search_chain_order(&path, &["searxng".into(), "ddgs".into()]).expect("chain");

    let web = load_web_tools_config_from_path(&path).expect("web");
    assert_eq!(web.extract_backend, "parallel");
}

#[test]
fn e2e_migrate_legacy_search_backend_to_chain() {
    let _lock = registry_guard();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    edgecrab_tools::persist_web_section_in_config(
        &path,
        &edgecrab_tools::WebSectionUpdate {
            backend: Some("brave".into()),
            search_backend: Some(String::new()),
            extract_backend: None,
        },
    )
    .expect("legacy");
    assert!(edgecrab_tools::migrate_legacy_search_override(&path).expect("migrate"));
    assert!(edgecrab_tools::search_section_override_from_path(&path).is_none());
    let search = load_web_search_config_from_path(&path).expect("search");
    assert_eq!(search.primary, "brave");
    assert_eq!(search.fallbacks, vec!["ddgs".to_string()]);
}

#[test]
fn e2e_startup_coherence_migrates_legacy_override() {
    let _lock = registry_guard();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    persist_web_section_in_config(
        &path,
        &WebSectionUpdate {
            backend: Some(String::new()),
            search_backend: Some("tavily".into()),
            extract_backend: None,
        },
    )
    .expect("legacy");
    ensure_web_search_config_coherence_at(&path);
    assert!(search_section_override_from_path(&path).is_none());
    let search = load_web_search_config_from_path(&path).expect("search");
    assert_eq!(search.primary, "tavily");
}

#[test]
fn e2e_app_config_load_from_migrates_legacy_search_override() {
    let _lock = registry_guard();
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("config.yaml");
    persist_web_section_in_config(
        &path,
        &WebSectionUpdate {
            backend: Some(String::new()),
            search_backend: Some("brave".into()),
            extract_backend: None,
        },
    )
    .expect("legacy");
    // Same migration AppConfig::load_from invokes — keep this crate free of
    // edgecrab-core so crates.io publish order (tools → core) stays acyclic.
    ensure_web_search_config_coherence_at(&path);
    assert!(search_section_override_from_path(&path).is_none());
    let search = load_web_search_config_from_path(&path).expect("search");
    assert_eq!(search.primary, "brave");
}

#[test]
fn e2e_gateway_web_command_reply_non_empty() {
    let _lock = registry_guard();
    for sub in ["status", "chain", "doctor", "help"] {
        let reply = edgecrab_tools::gateway_web_command_reply(sub);
        assert!(!reply.is_empty(), "reply for {sub}");
    }
}

#[test]
fn e2e_default_chain_for_backend_matches_wizard() {
    assert_eq!(
        default_chain_for_backend("tavily"),
        vec!["tavily".to_string(), "ddgs".to_string()]
    );
}

#[test]
fn e2e_web_command_overlay_subcommands_non_empty() {
    let _lock = registry_guard();
    for sub in ["status", "chain", "doctor", "providers", "help"] {
        let (title, _subtitle, body) = web_command_overlay(sub);
        assert!(!title.is_empty(), "title for {sub}");
        assert!(!body.is_empty(), "body for {sub}");
    }
    let (_, _, chain_body) = web_command_overlay("chain");
    assert!(chain_body.contains("FALLBACK FLOW"));
    let (_, _, help_body) = web_command_overlay("help");
    assert!(help_body.contains("/web status"));
}
