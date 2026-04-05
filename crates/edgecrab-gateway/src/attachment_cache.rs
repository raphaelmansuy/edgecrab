use std::path::{Path, PathBuf};

pub(crate) fn platform_media_cache_dir(platform: &str) -> PathBuf {
    edgecrab_core::gateway_media_dir().join(platform)
}

pub(crate) fn sanitize_file_name(file_name: &str) -> Option<String> {
    let sanitized = file_name
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>();
    if sanitized.is_empty() {
        None
    } else {
        Some(sanitized)
    }
}

pub(crate) fn unique_attachment_path(cache_dir: &Path, file_name: &str) -> PathBuf {
    let candidate = cache_dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }

    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("attachment");
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!(".{extension}"))
        .unwrap_or_default();

    for index in 1..1000 {
        let candidate = cache_dir.join(format!("{stem}-{index}{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    cache_dir.join(format!("{stem}-overflow{extension}"))
}

pub(crate) fn persist_bytes(
    platform: &str,
    file_name: Option<&str>,
    fallback_name: &str,
    bytes: &[u8],
) -> anyhow::Result<Option<String>> {
    if bytes.is_empty() {
        return Ok(None);
    }

    let cache_dir = platform_media_cache_dir(platform);
    std::fs::create_dir_all(&cache_dir)?;

    let candidate_name = file_name
        .and_then(sanitize_file_name)
        .unwrap_or_else(|| fallback_name.to_string());
    let file_path = unique_attachment_path(&cache_dir, &candidate_name);
    std::fs::write(&file_path, bytes)?;

    Ok(Some(file_path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_file_name_replaces_reserved_chars() {
        assert_eq!(
            sanitize_file_name("bad:/\\\\name?.png").as_deref(),
            Some("bad____name_.png")
        );
    }

    #[test]
    fn unique_attachment_path_appends_suffix_when_needed() {
        let temp = tempfile::tempdir().expect("tempdir");
        let original = temp.path().join("report.txt");
        std::fs::write(&original, b"hello").expect("write");

        let next = unique_attachment_path(temp.path(), "report.txt");
        assert_eq!(
            next.file_name().and_then(|n| n.to_str()),
            Some("report-1.txt")
        );
    }
}
