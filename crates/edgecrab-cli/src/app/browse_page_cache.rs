//! Sliding browse page cache for Skills Marketplace virtual scroll.
//!
//! Progressive search fills the loaded window; scrolling near the end extends
//! from per-source [`SkillCatalogStore`] slices. `fetch_cursor` is a SoT offset
//! (not a Vec index) — merge-by-identifier dedup can make `loaded < cursor`.

use edgecrab_tools::tools::skills_hub::MARKETPLACE_BROWSE_PAGE_SIZE;

/// Default retain cap after Ready (≈ 7–8 pages).
pub const BROWSE_RETAIN_CAP: usize = 600;

/// Max SoT pages to pull in one extend call when merges are all duplicates.
const DEDUP_SKIP_MAX_PAGES: usize = 8;

#[derive(Debug, Clone)]
pub struct BrowsePageCache {
    pub page_size: usize,
    /// Next offset into the authoritative per-source catalog SoT.
    pub fetch_cursor: usize,
    pub exhausted: bool,
    /// True while an extend request is in flight (avoid pile-up).
    pub extending: bool,
    pub retain_cap: usize,
    /// Set after `RemoteSkillSearchReady` — enables trim.
    pub stream_complete: bool,
    /// Background [`ensure_catalog`] running for the active filter.
    pub catalog_ensure_inflight: bool,
    /// Filter id the in-flight ensure was started for (ignore stale Done).
    pub catalog_ensure_filter: Option<String>,
}

impl Default for BrowsePageCache {
    fn default() -> Self {
        Self {
            page_size: MARKETPLACE_BROWSE_PAGE_SIZE,
            fetch_cursor: 0,
            exhausted: false,
            extending: false,
            retain_cap: BROWSE_RETAIN_CAP,
            stream_complete: false,
            catalog_ensure_inflight: false,
            catalog_ensure_filter: None,
        }
    }
}

impl BrowsePageCache {
    pub fn reset_for_browse(&mut self) {
        *self = Self::default();
    }

    pub fn clear_catalog_ensure(&mut self) {
        self.catalog_ensure_inflight = false;
        self.catalog_ensure_filter = None;
    }

    pub fn mark_stream_complete(&mut self, loaded_len: usize) {
        self.stream_complete = true;
        self.extending = false;
        // Cursor tracks SoT offset; never shrink below loaded unique count.
        self.fetch_cursor = self.fetch_cursor.max(loaded_len);
    }

    /// True when selection is near the end of the loaded (deduped) window.
    pub fn should_extend(&self, selected: usize, loaded_len: usize) -> bool {
        if self.exhausted || self.extending || loaded_len == 0 {
            return false;
        }
        let threshold = self.page_size / 2;
        selected + threshold >= loaded_len.saturating_sub(1)
    }

    /// Advance SoT cursor after a page pull, accounting for identifier dedup.
    ///
    /// Always advances by the SoT page size when the slice was non-empty, even
    /// if merge added zero unique rows — otherwise virtual scroll stalls on
    /// duplicate pages (index ∪ skills.sh / multi-root GitHub).
    pub fn advance_after_page(&mut self, sot_rows: usize, unique_added: usize) {
        if sot_rows == 0 {
            return;
        }
        self.fetch_cursor = self.fetch_cursor.saturating_add(self.page_size);
        if unique_added > 0 {
            self.exhausted = false;
        }
    }

    pub fn dedup_skip_budget(&self) -> usize {
        DEDUP_SKIP_MAX_PAGES
    }
}

/// Footer range while browsing — never `"0 of 0"` during an in-flight load.
///
/// When `catalog_total` is known and larger than the loaded (deduped) window,
/// the "of N" uses that SoT size so `60–78 of 78` is not mistaken for
/// end-of-catalog. `ensure_inflight` / `!complete` keeps `loading…` visible
/// even when loaded == catalog_total hint from a partial cache.
pub fn marketplace_browse_range_label_with_catalog(
    scroll_start: usize,
    visible: usize,
    loaded: usize,
    inflight: bool,
    catalog_total: Option<usize>,
) -> String {
    marketplace_browse_range_label_full(
        scroll_start,
        visible,
        loaded,
        inflight,
        catalog_total,
        false,
    )
}

pub fn marketplace_browse_range_label_full(
    scroll_start: usize,
    visible: usize,
    loaded: usize,
    search_inflight: bool,
    catalog_total: Option<usize>,
    catalog_loading: bool,
) -> String {
    let inflight = search_inflight || catalog_loading;
    if inflight && loaded == 0 {
        return "Browsing…".into();
    }
    if loaded == 0 {
        return "0 of 0".into();
    }
    let display_total = match catalog_total {
        Some(n) if n > loaded => n,
        _ => loaded,
    };
    let base =
        crate::overlay_layout::browser_virtual_range_label(scroll_start, visible, display_total);
    if inflight {
        format!("{base} · loading…")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_never_zero_of_zero_while_inflight() {
        assert_eq!(
            marketplace_browse_range_label_with_catalog(0, 0, 0, true, None),
            "Browsing…"
        );
        let mid = marketplace_browse_range_label_with_catalog(0, 10, 40, true, None);
        assert!(mid.contains("loading"), "{mid}");
        assert!(!mid.starts_with("0 of 0"), "{mid}");
    }

    #[test]
    fn footer_uses_catalog_total_when_larger_than_loaded() {
        let label = marketplace_browse_range_label_with_catalog(59, 19, 78, false, Some(12_400));
        assert!(
            label.contains("of 12400"),
            "expected SoT catalog total, got {label}"
        );
        assert!(label.starts_with("60–"), "{label}");
    }

    #[test]
    fn footer_loading_while_catalog_ensure_even_if_loaded_equals_hint() {
        let label =
            marketplace_browse_range_label_full(59, 19, 78, false, Some(78), true);
        assert!(label.contains("loading"), "{label}");
        assert!(label.contains("of 78"), "{label}");
    }

    #[test]
    fn should_extend_near_end() {
        let cache = BrowsePageCache {
            page_size: 80,
            ..Default::default()
        };
        assert!(!cache.should_extend(0, 100));
        assert!(cache.should_extend(70, 100));
    }

    #[test]
    fn advance_after_page_moves_cursor_even_when_all_dups() {
        let mut cache = BrowsePageCache {
            page_size: 80,
            fetch_cursor: 80,
            ..Default::default()
        };
        cache.advance_after_page(80, 0);
        assert_eq!(cache.fetch_cursor, 160);
        assert!(!cache.exhausted);
    }
}
