//! Slash-invocation footers — setup notes + supporting-file hints (Hermes parity).

use std::path::Path;

use crate::config_ref::resolve_edgecrab_home;
use crate::tools::skills::{skill_missing_credential_files, skill_missing_env_specs};

const GATEWAY_SETUP_HINT: &str = "Secure secret entry is not available on this messaging surface. \
Load this skill in the local CLI to be prompted, or add the key to ~/.edgecrab/.env manually.";

/// List relative paths under `references/`, `templates/`, `scripts/`, `assets/`.
pub fn list_supporting_files(skill_dir: &Path) -> Vec<String> {
    let mut supporting_files = Vec::new();
    for subdir in &["references", "templates", "scripts", "assets"] {
        let sub_path = skill_dir.join(subdir);
        let entries = match std::fs::read_dir(&sub_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                supporting_files.push(format!("{subdir}/{name}"));
            }
        }
    }
    supporting_files.sort();
    supporting_files
}

fn normalize_dir_display(skill_dir: &Path) -> String {
    let display = skill_dir.to_string_lossy().to_string();
    if cfg!(windows) {
        display.replace('\\', "/")
    } else {
        display
    }
}

/// `[Skill setup note: ...]` when required env vars or credential files are missing.
pub fn format_slash_setup_note(skill_dir: &Path, interactive: bool) -> Option<String> {
    let missing_env = skill_missing_env_specs(skill_dir);
    let missing_cred = skill_missing_credential_files(skill_dir);
    if missing_env.is_empty() && missing_cred.is_empty() {
        return None;
    }
    if !interactive {
        let home = resolve_edgecrab_home();
        let hint = GATEWAY_SETUP_HINT.replace("~/.edgecrab", &home.display().to_string());
        let mut parts = vec![hint];
        if !missing_env.is_empty() {
            let names: Vec<_> = missing_env.iter().map(|s| s.name.as_str()).collect();
            parts.push(format!("missing env: {}", names.join(", ")));
        }
        if !missing_cred.is_empty() {
            let paths: Vec<_> = missing_cred.iter().map(|s| s.path.as_str()).collect();
            parts.push(format!("missing credential files: {}", paths.join(", ")));
        }
        return Some(format!("[Skill setup note: {}]", parts.join("; ")));
    }

    let home = resolve_edgecrab_home();
    let mut items = Vec::new();
    for spec in &missing_env {
        items.push(format!("env `{}`", spec.name));
    }
    for spec in &missing_cred {
        items.push(format!(
            "credential file `{}` ({}/{})",
            spec.path,
            home.display(),
            spec.path
        ));
    }
    let missing_str = items.join(", ");
    let mut note =
        format!("[Skill setup note: Setup needed before using this skill: missing {missing_str}.");
    if let Some(first) = missing_env.first() {
        if let Some(prompt) = &first.prompt {
            note.push(' ');
            note.push_str(prompt);
        }
        note.push_str(&format!(" Set with: export {}=<value>", first.name));
    } else if let Some(first) = missing_cred.first() {
        if let Some(desc) = &first.description {
            note.push(' ');
            note.push_str(desc);
        }
        note.push_str(&format!(
            " Place file at {}/{}.",
            home.display(),
            first.path
        ));
    }
    note.push(']');
    Some(note)
}

/// Hermes-style supporting-files block with absolute paths + `skill_view` hint.
pub fn format_slash_supporting_files(skill_dir: &Path, skill_lookup_name: &str) -> Option<String> {
    let files = list_supporting_files(skill_dir);
    if files.is_empty() {
        return None;
    }
    let dir_display = normalize_dir_display(skill_dir);
    let mut lines = vec!["[This skill has supporting files:]".to_string()];
    for rel in &files {
        lines.push(format!("- {rel}  ->  {dir_display}/{rel}"));
    }
    lines.push(String::new());
    lines.push(format!(
        "Load any of these with skill_view(name=\"{skill_lookup_name}\", file_path=\"<path>\"), \
or run scripts directly by absolute path (e.g. `node {dir_display}/scripts/foo.js`)."
    ));
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn supporting_block_includes_absolute_paths() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("demo");
        let scripts = skill_dir.join("scripts");
        std::fs::create_dir_all(&scripts).expect("mkdir");
        std::fs::write(scripts.join("run.js"), "console.log('hi')").expect("write");
        let block = format_slash_supporting_files(&skill_dir, "demo").expect("block");
        assert!(block.contains("scripts/run.js"));
        assert!(block.contains(&format!("{}/scripts/run.js", skill_dir.display())));
        assert!(block.contains("skill_view"));
    }

    #[test]
    fn gateway_setup_note_when_env_missing() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("needs-key");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: needs-key\nrequired_environment_variables:\n  - name: API_TOKEN\n    prompt: Get a token\n---\n\nBody\n",
        )
        .expect("write");
        let note = format_slash_setup_note(&skill_dir, false).expect("note");
        assert!(note.contains("Skill setup note"));
        assert!(note.contains("API_TOKEN") || note.contains("Secure secret"));
    }

    #[test]
    fn gateway_setup_note_when_credential_file_missing() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("needs-creds");
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: needs-creds\nrequired_credential_files:\n  - path: oauth_token.json\n    description: OAuth token\n---\n\nBody\n",
        )
        .expect("write");
        let note = format_slash_setup_note(&skill_dir, false).expect("note");
        assert!(note.contains("oauth_token.json"));
        assert!(note.contains("missing credential files"));
    }
}
