use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const ENTRY_DELIMITER: &str = "\n§\n";

pub fn copy_dir_recursive(src: &Path, dst: &Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(dst)?;
    let mut count = 0usize;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            count += copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
            count += 1;
        }
    }

    Ok(count)
}

pub fn ensure_dir(dir: &Path) -> std::io::Result<()> {
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}

pub fn ensure_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    Ok(())
}

pub fn parse_env_file(path: &Path) -> BTreeMap<String, String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };

    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

pub fn save_env_file(path: &Path, data: &BTreeMap<String, String>) -> anyhow::Result<()> {
    ensure_parent(path)?;
    let payload = data
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n");
    let body = if payload.is_empty() {
        String::new()
    } else {
        format!("{payload}\n")
    };
    std::fs::write(path, body)?;
    Ok(())
}

pub fn load_yaml_file(path: &Path) -> anyhow::Result<serde_yml::Value> {
    if !path.exists() {
        return Ok(serde_yml::Value::Mapping(serde_yml::Mapping::new()));
    }
    let raw = std::fs::read_to_string(path)?;
    let value = serde_yml::from_str::<serde_yml::Value>(&raw)?;
    Ok(match value {
        serde_yml::Value::Mapping(_) => value,
        _ => serde_yml::Value::Mapping(serde_yml::Mapping::new()),
    })
}

pub fn save_yaml_file(path: &Path, value: &serde_yml::Value) -> anyhow::Result<()> {
    ensure_parent(path)?;
    let raw = serde_yml::to_string(value)?;
    std::fs::write(path, raw)?;
    Ok(())
}

pub fn parse_existing_memory_entries(path: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    if raw.contains(ENTRY_DELIMITER) {
        return raw
            .split(ENTRY_DELIMITER)
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(ToString::to_string)
            .collect();
    }
    extract_markdown_entries(&raw)
}

pub fn extract_markdown_entries(text: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut headings: Vec<String> = Vec::new();
    let mut paragraph_lines: Vec<String> = Vec::new();
    let mut in_code_block = false;

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        let stripped = line.trim();

        if stripped.starts_with("```") {
            in_code_block = !in_code_block;
            flush_paragraph(&mut entries, &headings, &mut paragraph_lines);
            continue;
        }
        if in_code_block {
            continue;
        }

        if let Some((level, heading)) = parse_heading(stripped) {
            flush_paragraph(&mut entries, &headings, &mut paragraph_lines);
            while headings.len() >= level {
                headings.pop();
            }
            headings.push(heading);
            continue;
        }

        if let Some(item) = parse_bullet(line) {
            flush_paragraph(&mut entries, &headings, &mut paragraph_lines);
            let prefix = context_prefix(&headings);
            if prefix.is_empty() {
                entries.push(item);
            } else {
                entries.push(format!("{prefix}: {item}"));
            }
            continue;
        }

        if stripped.is_empty() {
            flush_paragraph(&mut entries, &headings, &mut paragraph_lines);
            continue;
        }

        if stripped.starts_with('|') && stripped.ends_with('|') {
            flush_paragraph(&mut entries, &headings, &mut paragraph_lines);
            continue;
        }

        paragraph_lines.push(stripped.to_string());
    }

    flush_paragraph(&mut entries, &headings, &mut paragraph_lines);

    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let normalized = normalize_text(&entry);
        if normalized.is_empty() || !seen.insert(normalized) {
            continue;
        }
        deduped.push(entry);
    }
    deduped
}

fn flush_paragraph(
    entries: &mut Vec<String>,
    headings: &[String],
    paragraph_lines: &mut Vec<String>,
) {
    if paragraph_lines.is_empty() {
        return;
    }
    let text_block = paragraph_lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    paragraph_lines.clear();
    if text_block.is_empty() {
        return;
    }
    let prefix = context_prefix(headings);
    if prefix.is_empty() {
        entries.push(text_block);
    } else {
        entries.push(format!("{prefix}: {text_block}"));
    }
}

pub fn merge_entries(
    existing: &[String],
    incoming: &[String],
    limit: usize,
) -> (Vec<String>, MergeStats, Vec<String>) {
    let mut merged = existing.to_vec();
    let mut seen = existing
        .iter()
        .map(|entry| normalize_text(entry))
        .filter(|entry| !entry.is_empty())
        .collect::<std::collections::HashSet<_>>();
    let mut overflowed = Vec::new();
    let mut stats = MergeStats {
        existing: existing.len(),
        added: 0,
        duplicates: 0,
        overflowed: 0,
    };
    let mut current_len = if merged.is_empty() {
        0
    } else {
        merged.join(ENTRY_DELIMITER).chars().count()
    };

    for entry in incoming {
        let normalized = normalize_text(entry);
        if normalized.is_empty() {
            continue;
        }
        if seen.contains(&normalized) {
            stats.duplicates += 1;
            continue;
        }

        let entry_len = entry.chars().count();
        let candidate_len = if merged.is_empty() {
            entry_len
        } else {
            current_len + ENTRY_DELIMITER.chars().count() + entry_len
        };
        if candidate_len > limit {
            stats.overflowed += 1;
            overflowed.push(entry.clone());
            continue;
        }

        merged.push(entry.clone());
        seen.insert(normalized);
        current_len = candidate_len;
        stats.added += 1;
    }

    (merged, stats, overflowed)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeStats {
    pub existing: usize,
    pub added: usize,
    pub duplicates: usize,
    pub overflowed: usize,
}

pub fn relative_label(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|_| path.display().to_string())
}

pub fn copy_path(source: &Path, destination: &Path) -> anyhow::Result<()> {
    ensure_parent(destination)?;
    if source.is_dir() {
        copy_dir_recursive(source, destination)?;
    } else {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

pub fn backup_existing(path: &Path, backup_root: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let relative = if path.is_absolute() && path.components().count() > 1 {
        path.components()
            .skip(1)
            .map(|component| component.as_os_str())
            .collect::<PathBuf>()
    } else {
        path.to_path_buf()
    };
    let destination = backup_root.join(relative);
    copy_path(path, &destination)?;
    Ok(Some(destination))
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 {
        return None;
    }
    let rest = line.get(hashes..)?.trim();
    if rest.is_empty() {
        return None;
    }
    Some((hashes, rest.to_string()))
}

fn parse_bullet(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return Some(trimmed[2..].trim().to_string());
    }

    let mut idx = 0usize;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            idx += ch.len_utf8();
            continue;
        }
        if ch == '.' && idx > 0 {
            let rest = trimmed.get(idx + 1..)?.trim();
            if rest.is_empty() {
                return None;
            }
            return Some(rest.to_string());
        }
        break;
    }
    None
}

fn context_prefix(headings: &[String]) -> String {
    headings
        .iter()
        .filter(|heading| {
            let upper = heading.to_ascii_uppercase();
            let leading = upper
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches([':', '-']);
            !matches!(
                leading,
                "MEMORY.MD" | "USER.MD" | "SOUL.MD" | "AGENTS.MD" | "TOOLS.MD" | "IDENTITY.MD"
            )
        })
        .cloned()
        .collect::<Vec<_>>()
        .join(" > ")
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_markdown_entries_promotes_heading_context() {
        let text = "# MEMORY.md - Long-Term Memory\n\n## Tyler Williams\n\n- Founder of VANTA Research\n- Timezone: America/Los_Angeles\n\n### Active Projects\n\n- Hermes Agent\n";
        let entries = extract_markdown_entries(text);
        assert!(entries.contains(&"Tyler Williams: Founder of VANTA Research".to_string()));
        assert!(entries.contains(&"Tyler Williams: Timezone: America/Los_Angeles".to_string()));
        assert!(entries.contains(&"Tyler Williams > Active Projects: Hermes Agent".to_string()));
    }

    #[test]
    fn merge_entries_respects_limit_and_reports_overflow() {
        let existing = vec!["alpha".to_string()];
        let incoming = vec!["beta".to_string(), "gamma is too long".to_string()];
        let (merged, stats, overflowed) = merge_entries(&existing, &incoming, 12);
        assert_eq!(merged, vec!["alpha".to_string(), "beta".to_string()]);
        assert_eq!(stats.added, 1);
        assert_eq!(stats.overflowed, 1);
        assert_eq!(overflowed, vec!["gamma is too long".to_string()]);
    }
}
