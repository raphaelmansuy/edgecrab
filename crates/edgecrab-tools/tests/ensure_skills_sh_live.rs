//! Live skills.sh sitemap ensure (network). Ignored in default CI.

#[tokio::test]
#[ignore]
async fn ensure_skills_sh_sitemap_catalog_live() {
    let dir = tempfile::tempdir().unwrap();
    unsafe {
        std::env::set_var("EDGECRAB_HOME", dir.path());
    }
    let result = edgecrab_tools::tools::skills_hub::ensure_skills_sh_sitemap_catalog().await;
    println!("result={result:?}");
    let n = edgecrab_tools::tools::skills_hub::skills_sh_sitemap_cache_len();
    let complete = edgecrab_tools::tools::skills_hub::skills_sh_sitemap_catalog_complete();
    println!("len={n:?} complete={complete}");
    assert!(result.is_ok(), "ensure failed: {result:?}");
    assert!(n.is_some_and(|x| x > 80), "expected >80 cached, got {n:?}");
    assert!(complete);
    unsafe {
        std::env::remove_var("EDGECRAB_HOME");
    }
}
