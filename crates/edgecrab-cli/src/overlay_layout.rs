//! Shared overlay geometry helpers.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// Center a popup of `w`×`h` inside `area`.
pub fn popup_rect(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width).max(1);
    let h = h.min(area.height).max(1);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect::new(x, y, w, h)
}

/// Standard 3-section vertical layout for picker overlays:
/// `[0]` header (3 rows), `[1]` body (min), `[2]` help bar (1 row).
pub fn picker_three_layout(popup: Rect) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(popup)
}

/// Split a body area into `[0]` list pane and `[1]` detail pane.
pub fn picker_two_cols(body: Rect, list_pct: u16) -> std::rc::Rc<[Rect]> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(list_pct),
            Constraint::Percentage(100u16.saturating_sub(list_pct)),
        ])
        .split(body)
}

/// Full-screen browser overlay: header (3) / body (min) / help (1).
pub fn browser_overlay_chunks(area: Rect) -> std::rc::Rc<[Rect]> {
    picker_three_layout(area)
}

/// Split browser body into list (62%) and detail (38%).
pub fn browser_body_chunks(area: Rect) -> std::rc::Rc<[Rect]> {
    picker_two_cols(area, 62)
}

pub fn browser_list_visible_rows(area: Rect, bordered: bool) -> usize {
    let reserved_rows = if bordered { 2 } else { 0 };
    area.height.saturating_sub(reserved_rows).max(1) as usize
}

pub fn browser_scroll_start(selected: usize, max_visible: usize) -> usize {
    browser_virtual_window(selected, usize::MAX, max_visible).0
}

/// Virtual scroll window for large picker lists.
///
/// Returns `(scroll_start, visible_count)` so only a viewport-sized slice is
/// materialized as widgets while the selection stays on-screen.
pub fn browser_virtual_window(selected: usize, total: usize, max_visible: usize) -> (usize, usize) {
    let max_visible = max_visible.max(1);
    if total == 0 {
        return (0, 0);
    }
    let total = total.max(1);
    let selected = selected.min(total.saturating_sub(1));
    let visible = max_visible.min(total);
    let scroll_start = if selected < visible {
        0
    } else {
        selected + 1 - visible
    };
    let max_start = total.saturating_sub(visible);
    (scroll_start.min(max_start), visible)
}

/// Human label for a virtualized window, e.g. `"12–31 of 847"`.
pub fn browser_virtual_range_label(scroll_start: usize, visible: usize, total: usize) -> String {
    if total == 0 {
        return "0 of 0".into();
    }
    let from = scroll_start.saturating_add(1);
    let to = scroll_start
        .saturating_add(visible.max(1))
        .min(total)
        .max(from);
    format!("{from}–{to} of {total}")
}

/// Standard help-bar `Line` for picker overlays: `↑↓ browse  Tab next  Enter apply  Esc cancel`.
pub fn picker_help_line(accent: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled(" ↑↓ ", Style::default().fg(accent)),
        Span::styled("browse  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Tab ", Style::default().fg(accent)),
        Span::styled("next  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter ", Style::default().fg(accent)),
        Span::styled("apply  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc ", Style::default().fg(accent)),
        Span::styled("cancel", Style::default().fg(Color::DarkGray)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_scroll_start_keeps_selection_visible() {
        assert_eq!(browser_scroll_start(0, 10), 0);
        assert_eq!(browser_scroll_start(15, 10), 6);
    }

    #[test]
    fn browser_virtual_window_clamps_and_labels() {
        assert_eq!(browser_virtual_window(0, 100, 10), (0, 10));
        assert_eq!(browser_virtual_window(15, 100, 10), (6, 10));
        assert_eq!(browser_virtual_window(99, 100, 10), (90, 10));
        assert_eq!(browser_virtual_window(0, 3, 10), (0, 3));
        assert_eq!(browser_virtual_range_label(0, 10, 100), "1–10 of 100");
        assert_eq!(browser_virtual_range_label(90, 10, 100), "91–100 of 100");
        assert_eq!(browser_virtual_range_label(0, 0, 0), "0 of 0");
    }

    #[test]
    fn marketplace_scale_catalog_only_materializes_viewport() {
        // skills.sh-scale: 1000 rows, 24-row TUI pane → only 24 widgets.
        let total = 1000usize;
        let max_visible = 24usize;
        let selected = 500usize;
        let (start, visible) = browser_virtual_window(selected, total, max_visible);
        assert_eq!(visible, 24);
        assert_eq!(start, 477); // selected at bottom of window
        let materialized: Vec<usize> = (0..total).skip(start).take(visible).collect();
        assert_eq!(materialized.len(), 24);
        assert!(materialized.contains(&selected));
        assert!(!materialized.contains(&0));
        assert!(!materialized.contains(&(total - 1)) || selected >= total - visible);
    }

    #[test]
    fn popup_fits_inside_area() {
        let area = Rect::new(0, 0, 80, 24);
        let popup = popup_rect(area, 40, 10);
        assert!(popup.width <= area.width);
        assert!(popup.height <= area.height);
        assert!(popup.x + popup.width <= area.x + area.width);
        assert!(popup.y + popup.height <= area.y + area.height);
    }

    #[test]
    fn picker_layout_three_rows() {
        let area = Rect::new(0, 0, 80, 24);
        let chunks = picker_three_layout(area);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].height, 3);
        assert_eq!(chunks[2].height, 1);
    }
}
