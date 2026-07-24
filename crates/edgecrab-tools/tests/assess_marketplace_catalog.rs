//! Assessment of CatalogStore against a live profile cache.
//!
//! ```text
//! EDGECRAB_HOME=~/.edgecrab/profiles/homelab \
//!   cargo test -p edgecrab-tools --test assess_marketplace_catalog -- --nocapture
//! ```

use edgecrab_tools::tools::skills_hub::{
    FilterCatalogStore, catalog_page, marketplace_provider_filters,
};

#[test]
#[ignore = "manual TUI/SoT assessment against a live profile cache"]
fn assess_each_marketplace_filter_against_disk_sot() {
    let home = std::env::var("EDGECRAB_HOME").unwrap_or_else(|_| {
        format!(
            "{}/.edgecrab/profiles/homelab",
            std::env::var("HOME").expect("HOME")
        )
    });
    unsafe {
        std::env::set_var("EDGECRAB_HOME", &home);
    }
    println!("EDGECRAB_HOME={home}");
    println!(
        "{:<16} {:>8} {:>8} {:>8} {:>8} {:>8}  verdict",
        "filter", "total", "complete", "p0", "p80", "uniq+"
    );

    let mut pass = 0usize;
    let mut warn = 0usize;
    let mut fail = 0usize;

    for filter in marketplace_provider_filters() {
        let store = FilterCatalogStore::for_filter(filter);
        let total = store.total();
        let complete = store.complete();
        let page0 = catalog_page(filter, 0, 80);
        let page80 = catalog_page(filter, 80, 80);
        let p0 = page0.row_count();
        let p80 = page80.row_count();
        let page1_ids: std::collections::HashSet<_> = page0
            .groups
            .into_iter()
            .flat_map(|g| g.results)
            .map(|m| m.identifier)
            .collect();
        let uniq: usize = page80
            .groups
            .iter()
            .flat_map(|g| g.results.iter())
            .filter(|m| !page1_ids.contains(&m.identifier))
            .count();
        let total_n = total.unwrap_or(0);
        let can_page = total_n > 80 && uniq > 0;
        let stuck_first = total_n > 80 && p80 == 0;
        let no_sot = total.is_none() && p0 == 0 && *filter != "all";

        let verdict = if can_page {
            pass += 1;
            "PASS page2"
        } else if stuck_first {
            fail += 1;
            "FAIL no page2"
        } else if no_sot {
            fail += 1;
            "FAIL no-SoT"
        } else if total_n > 0 && total_n <= 80 && complete {
            pass += 1;
            "PASS small-complete"
        } else if total_n > 0 && total_n <= 80 && !complete {
            warn += 1;
            "WARN small-incomplete"
        } else if *filter == "all" {
            if p0 > 0 {
                pass += 1;
                "PASS all-has-rows"
            } else {
                warn += 1;
                "WARN all-empty"
            }
        } else {
            warn += 1;
            "WARN check"
        };

        println!(
            "{:<16} {:>8} {:>8} {:>8} {:>8} {:>8}  {}",
            filter,
            total.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
            complete,
            p0,
            p80,
            uniq,
            verdict
        );
    }

    println!("\nSUMMARY pass={pass} warn={warn} fail={fail}");
    // Assessment is diagnostic: FAIL rows document CatalogStore gaps (expected
    // until CatalogBackend uniform wiring lands). Do not gate CI here.
}
