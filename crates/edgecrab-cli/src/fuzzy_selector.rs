//! Generic fuzzy-search overlay state.
//!
//! Both the model selector and the skill browser (and future overlays such as
//! the session browser) share the same navigation logic:
//!
//!  * A `Vec<T>` of items, each implementing [`FuzzyItem`].
//!  * A free-text `query` that filters `items` into `filtered` (indices).
//!  * A `selected` cursor within `filtered`.
//!  * An `active` flag.
//!
//! By centralising the state + methods here we eliminate the ~250 LOC of
//! duplicated boilerplate that used to live in `ModelSelectorState` /
//! `SkillSelectorState` while keeping the rendering completely separate (since
//! each overlay has its own colour scheme and column layout).

/// A trait for items that can be displayed in a fuzzy-search overlay.
pub trait FuzzyItem {
    /// Primary text — used for display and for filter matching.
    fn primary(&self) -> &str;

    /// Secondary text — also matched by the filter (e.g. description).
    /// Default implementation returns an empty string.
    fn secondary(&self) -> &str {
        ""
    }

    /// Short tag / label shown in a separate column (e.g. provider name).
    /// Default implementation returns an empty string.
    #[allow(dead_code)]
    fn tag(&self) -> &str {
        ""
    }
}

/// Generic fuzzy-selector overlay state shared by model, skill, and session
/// browsers.
pub struct FuzzySelector<T: Clone + FuzzyItem> {
    /// All items (full collection, unfiltered).
    pub items: Vec<T>,
    /// Indices into `items` that match the current `query`.
    pub filtered: Vec<usize>,
    /// Current filter text typed by the user.
    pub query: String,
    /// Currently highlighted index within `filtered`.
    pub selected: usize,
    /// Whether the overlay is visible.
    pub active: bool,
}

impl<T: Clone + FuzzyItem> FuzzySelector<T> {
    /// Create a new, empty, inactive selector.
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            filtered: Vec::new(),
            query: String::new(),
            selected: 0,
            active: false,
        }
    }

    /// Replace the item list and recompute filters.
    pub fn set_items(&mut self, items: Vec<T>) {
        self.items = items;
        self.update_filter();
    }

    /// Replace the item list while preserving the current query and best-effort
    /// focus on the previously selected item.
    pub fn replace_items_preserving_state(&mut self, items: Vec<T>) {
        let selected_primary = self.current().map(|item| item.primary().to_string());
        let query = self.query.clone();
        let was_active = self.active;

        self.items = items;
        self.query = query;
        self.update_filter();
        self.active = was_active;

        if let Some(primary) = selected_primary {
            if let Some(pos) = self.filtered.iter().position(|&idx| {
                self.items
                    .get(idx)
                    .is_some_and(|item| item.primary() == primary)
            }) {
                self.selected = pos;
            }
        }
    }

    /// Activate the selector with an empty query, pre-selecting `primary`.
    /// If `primary` is empty the first filtered item is highlighted.
    pub fn activate_with_primary(&mut self, primary: &str) {
        self.query.clear();
        self.selected = 0;
        self.active = true;
        self.update_filter();
        if !primary.is_empty() {
            if let Some(pos) = self
                .filtered
                .iter()
                .position(|&idx| self.items.get(idx).is_some_and(|i| i.primary() == primary))
            {
                self.selected = pos;
            }
        }
    }

    /// Activate the selector with an empty query and no pre-selection.
    #[allow(dead_code)]
    pub fn activate(&mut self) {
        self.activate_with_primary("");
    }

    /// Recompute `filtered` based on the current `query`.
    pub fn update_filter(&mut self) {
        let q = self.query.to_lowercase();
        let tokens: Vec<&str> = q
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .collect();
        self.filtered = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if q.is_empty() {
                    return true;
                }
                let primary = item.primary().to_lowercase();
                let secondary = item.secondary().to_lowercase();
                let tag = item.tag().to_lowercase();
                if primary.contains(&q) || secondary.contains(&q) || tag.contains(&q) {
                    return true;
                }
                if tokens.is_empty() {
                    return false;
                }
                let haystack = format!("{primary} {secondary} {tag}");
                tokens.iter().all(|token| haystack.contains(token))
            })
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = 0;
        }
    }

    /// Append a character to the query and refresh.
    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.update_filter();
    }

    /// Remove the last character from the query and refresh.
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.update_filter();
    }

    /// Move selection up by one row.
    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// Move selection down by one row.
    pub fn move_down(&mut self) {
        if self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    /// Move selection up by one page (8 rows).
    pub fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(8);
    }

    /// Move selection down by one page (8 rows).
    pub fn page_down(&mut self) {
        let last = self.filtered.len().saturating_sub(1);
        self.selected = (self.selected + 8).min(last);
    }

    /// Return the currently highlighted item, if any.
    pub fn current(&self) -> Option<&T> {
        self.filtered
            .get(self.selected)
            .and_then(|&idx| self.items.get(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestItem {
        primary: &'static str,
        secondary: &'static str,
        tag: &'static str,
    }

    impl FuzzyItem for TestItem {
        fn primary(&self) -> &str {
            self.primary
        }

        fn secondary(&self) -> &str {
            self.secondary
        }

        fn tag(&self) -> &str {
            self.tag
        }
    }

    #[test]
    fn update_filter_matches_tag_column() {
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![
            TestItem {
                primary: "filesystem",
                secondary: "local files",
                tag: "official-ref",
            },
            TestItem {
                primary: "github",
                secondary: "repo operations",
                tag: "configured",
            },
        ]);

        selector.query = "configured".into();
        selector.update_filter();

        assert_eq!(selector.filtered.len(), 1);
        assert_eq!(
            selector.current().map(|item| item.primary()),
            Some("github")
        );
    }

    #[test]
    fn replace_items_preserves_query_and_focus() {
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![
            TestItem {
                primary: "bedrock/amazon.nova-lite-v1:0",
                secondary: "static",
                tag: "bedrock",
            },
            TestItem {
                primary: "bedrock/anthropic.claude-4-sonnet-20250514-v1:0",
                secondary: "static",
                tag: "bedrock",
            },
        ]);
        selector.active = true;
        selector.query = "claude".into();
        selector.update_filter();

        selector.replace_items_preserving_state(vec![
            TestItem {
                primary: "bedrock/amazon.nova-lite-v1:0",
                secondary: "live",
                tag: "bedrock",
            },
            TestItem {
                primary: "bedrock/anthropic.claude-4-sonnet-20250514-v1:0",
                secondary: "live",
                tag: "bedrock",
            },
            TestItem {
                primary: "bedrock/deepseek.r1-v1:0",
                secondary: "live",
                tag: "bedrock",
            },
        ]);

        assert!(selector.active);
        assert_eq!(selector.query, "claude");
        assert_eq!(
            selector.current().map(|item| item.primary()),
            Some("bedrock/anthropic.claude-4-sonnet-20250514-v1:0")
        );
    }

    #[test]
    fn update_filter_matches_multi_word_queries_by_token() {
        let mut selector = FuzzySelector::new();
        selector.set_items(vec![TestItem {
            primary: "support triage",
            secondary: "trace websocket reconnect jitter",
            tag: "cli",
        }]);

        selector.query = "websocket jitter".into();
        selector.update_filter();

        assert_eq!(selector.filtered.len(), 1);
        assert_eq!(
            selector.current().map(|item| item.primary()),
            Some("support triage")
        );
    }
}
