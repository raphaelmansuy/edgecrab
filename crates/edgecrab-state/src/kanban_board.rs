//! Multi-board path resolution — Hermes `kanban_db_path` / `HERMES_KANBAN_BOARD` parity.

use std::path::{Path, PathBuf};

use edgecrab_types::AgentError;

pub const DEFAULT_BOARD: &str = "default";

const BOARD_SLUG_MAX: usize = 64;

/// Resolve EdgeCrab home for kanban paths.
pub fn kanban_home(home: Option<&Path>) -> PathBuf {
    home.map(Path::to_path_buf)
        .or_else(|| std::env::var("EDGECRAB_HOME").ok().map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|h| h.join(".edgecrab")))
        .unwrap_or_else(|| PathBuf::from(".edgecrab"))
}

/// Pin DB path directly (`EDGECRAB_KANBAN_DB`).
pub fn env_db_override() -> Option<PathBuf> {
    std::env::var("EDGECRAB_KANBAN_DB")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
}

/// Active board from env (`EDGECRAB_KANBAN_BOARD`).
pub fn env_board_override() -> Option<String> {
    std::env::var("EDGECRAB_KANBAN_BOARD")
        .ok()
        .and_then(|s| normalize_board_slug(Some(&s)))
}

fn normalize_board_slug(slug: Option<&str>) -> Option<String> {
    let s = slug?.trim().to_ascii_lowercase();
    if s.is_empty() || s.len() > BOARD_SLUG_MAX {
        return None;
    }
    let mut chars = s.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphanumeric() {
        return None;
    }
    for c in chars {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return None;
        }
    }
    Some(s)
}

/// Read persisted current board slug from `~/.edgecrab/kanban/current`.
pub fn get_current_board(home: &Path) -> String {
    if let Some(slug) = env_board_override() {
        return slug;
    }
    let current = home.join("kanban/current");
    std::fs::read_to_string(&current)
        .ok()
        .and_then(|s| normalize_board_slug(Some(s.trim())))
        .unwrap_or_else(|| DEFAULT_BOARD.to_string())
}

/// Persist active board slug.
pub fn set_current_board(home: &Path, slug: &str) -> Result<(), AgentError> {
    let slug = normalize_board_slug(Some(slug)).ok_or_else(|| {
        AgentError::Validation(format!(
            "invalid board slug '{slug}' (lowercase alphanumerics, hyphens, underscores; 1-64 chars)"
        ))
    })?;
    let dir = home.join("kanban");
    std::fs::create_dir_all(&dir).map_err(AgentError::Io)?;
    std::fs::write(dir.join("current"), format!("{slug}\n")).map_err(AgentError::Io)?;
    Ok(())
}

fn board_dir(home: &Path, slug: &str) -> PathBuf {
    home.join("kanban/boards").join(slug)
}

/// Path to `kanban.db` for a board. Default board uses legacy `~/.edgecrab/kanban.db`.
pub fn kanban_db_path_for_board(home: &Path, board: Option<&str>) -> PathBuf {
    if let Some(path) = env_db_override() {
        return path;
    }
    let slug = board
        .and_then(|b| normalize_board_slug(Some(b)))
        .unwrap_or_else(|| get_current_board(home));
    if slug == DEFAULT_BOARD {
        home.join("kanban.db")
    } else {
        board_dir(home, &slug).join("kanban.db")
    }
}

/// List known board slugs (always includes `default`).
pub fn list_board_slugs(home: &Path) -> Vec<String> {
    let mut boards = vec![DEFAULT_BOARD.to_string()];
    let boards_root = home.join("kanban/boards");
    if let Ok(entries) = std::fs::read_dir(&boards_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.join("kanban.db").exists()
                && let Some(name) = entry.file_name().to_str()
                && normalize_board_slug(Some(name)).is_some()
            {
                boards.push(name.to_ascii_lowercase());
            }
        }
    }
    boards.sort();
    boards.dedup();
    boards
}

/// Ensure a non-default board directory exists.
pub fn ensure_board(home: &Path, slug: &str) -> Result<PathBuf, AgentError> {
    let slug = normalize_board_slug(Some(slug)).ok_or_else(|| {
        AgentError::Validation(format!("invalid board slug '{slug}'"))
    })?;
    if slug == DEFAULT_BOARD {
        return Ok(kanban_db_path_for_board(home, Some(DEFAULT_BOARD)));
    }
    let dir = board_dir(home, &slug);
    std::fs::create_dir_all(&dir).map_err(AgentError::Io)?;
    Ok(dir.join("kanban.db"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_board_uses_legacy_path() {
        let dir = TempDir::new().expect("tmpdir");
        let home = dir.path();
        let path = kanban_db_path_for_board(home, Some(DEFAULT_BOARD));
        assert_eq!(path, home.join("kanban.db"));
    }

    #[test]
    fn named_board_uses_boards_dir() {
        let dir = TempDir::new().expect("tmpdir");
        let home = dir.path();
        let path = kanban_db_path_for_board(home, Some("proj-a"));
        assert_eq!(path, home.join("kanban/boards/proj-a/kanban.db"));
    }

    #[test]
    fn current_board_persistence() {
        let dir = TempDir::new().expect("tmpdir");
        let home = dir.path();
        set_current_board(home, "my-board").expect("set");
        assert_eq!(get_current_board(home), "my-board");
    }
}
