//! Pi-style `npm:` package install — extract tarball and find SKILL.md dirs.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::read::GzDecoder;
use tar::Archive;

use super::local_bundle::build_local_skill_bundle;
use super::{InstallGate, InstallOutcome, SkillBundle, install_skill};

/// Parse `npm:package[@version]` → (package, version opt).
pub fn parse_npm_spec(identifier: &str) -> Result<(String, Option<String>), String> {
    let rest = identifier
        .strip_prefix("npm:")
        .ok_or_else(|| format!("Not an npm: identifier: {identifier}"))?
        .trim();
    if rest.is_empty() {
        return Err("npm: requires a package name".into());
    }
    // scoped: @scope/name@version
    if let Some(without_at) = rest.strip_prefix('@') {
        if let Some((scoped, ver)) = without_at.rsplit_once('@')
            && scoped.contains('/')
        {
            return Ok((format!("@{scoped}"), Some(ver.to_string())));
        }
        return Ok((rest.to_string(), None));
    }
    if let Some((name, ver)) = rest.split_once('@') {
        return Ok((name.to_string(), Some(ver.to_string())));
    }
    Ok((rest.to_string(), None))
}

/// Fetch npm pack tarball into a temp dir and return skill bundles found.
pub fn fetch_npm_skill_bundles(identifier: &str) -> Result<Vec<SkillBundle>, String> {
    let (package, version) = parse_npm_spec(identifier)?;
    let pack_arg = match &version {
        Some(v) => format!("{package}@{v}"),
        None => package.clone(),
    };

    let tmp = tempfile::tempdir().map_err(|e| format!("tempdir: {e}"))?;
    let status = Command::new("npm")
        .args(["pack", &pack_arg, "--pack-destination"])
        .arg(tmp.path())
        .output()
        .map_err(|e| {
            format!("npm pack failed (is npm installed?): {e}. Tip: use git:owner/repo instead.")
        })?;

    if !status.status.success() {
        return Err(format!(
            "npm pack {pack_arg} failed: {}",
            String::from_utf8_lossy(&status.stderr)
        ));
    }

    let tgz = find_tgz(tmp.path())?;
    let extract_to = tmp.path().join("extracted");
    std::fs::create_dir_all(&extract_to).map_err(|e| e.to_string())?;
    extract_tgz(&tgz, &extract_to)?;

    // npm pack extracts into package/
    let package_root = find_package_root(&extract_to);
    let skill_dirs = find_skill_dirs_in_package(&package_root);
    if skill_dirs.is_empty() {
        return Err(format!(
            "npm package '{pack_arg}' contains no SKILL.md directories"
        ));
    }

    let mut bundles = Vec::new();
    for dir in skill_dirs {
        let mut bundle = build_local_skill_bundle(&dir, None)?;
        bundle.source = "npm".into();
        bundle.identifier = identifier.to_string();
        bundle.trust_level = "community".into();
        bundles.push(bundle);
    }
    Ok(bundles)
}

/// Install all skills from an npm: identifier.
pub fn install_npm_identifier(
    identifier: &str,
    skills_dir: &Path,
    gate: InstallGate,
) -> Result<Vec<InstallOutcome>, String> {
    let bundles = fetch_npm_skill_bundles(identifier)?;
    let mut outcomes = Vec::new();
    for bundle in bundles {
        let name = bundle.name.clone();
        let message = install_skill(&bundle, skills_dir, gate)?;
        outcomes.push(InstallOutcome {
            message,
            skill_name: name,
        });
    }
    Ok(outcomes)
}

fn find_tgz(dir: &Path) -> Result<PathBuf, String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("tgz") {
            return Ok(path);
        }
    }
    Err("npm pack produced no .tgz".into())
}

fn extract_tgz(tgz: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(tgz).map_err(|e| e.to_string())?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive.unpack(dest).map_err(|e| format!("untar: {e}"))?;
    Ok(())
}

fn find_package_root(extract_to: &Path) -> PathBuf {
    let package = extract_to.join("package");
    if package.is_dir() {
        package
    } else {
        extract_to.to_path_buf()
    }
}

fn find_skill_dirs_in_package(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    // Prefer package.json pi.skills / skills/ convention
    let skills_dir = root.join("skills");
    if skills_dir.is_dir() {
        walk_skill_dirs(&skills_dir, &mut out);
    }
    if out.is_empty() {
        walk_skill_dirs(root, &mut out);
    }
    // Also parse package.json "pi"."skills" paths
    if let Ok(pkg) = std::fs::read_to_string(root.join("package.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&pkg)
        && let Some(arr) = v.pointer("/pi/skills").and_then(|x| x.as_array())
    {
        for item in arr {
            if let Some(rel) = item.as_str() {
                let p = root.join(rel);
                if p.join("SKILL.md").is_file() {
                    out.push(p);
                } else if p.is_dir() {
                    walk_skill_dirs(&p, &mut out);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

fn walk_skill_dirs(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.join("SKILL.md").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_skill_dirs(&path, out);
        }
    }
}

/// Build a synthetic bundle from skill dirs (tests / offline).
#[allow(dead_code)]
pub fn bundles_from_skill_dirs(
    dirs: &[PathBuf],
    identifier: &str,
) -> Result<Vec<SkillBundle>, String> {
    let mut bundles = Vec::new();
    for dir in dirs {
        let mut bundle = build_local_skill_bundle(dir, None)?;
        bundle.source = "npm".into();
        bundle.identifier = identifier.to_string();
        bundles.push(bundle);
    }
    Ok(bundles)
}

/// Extract a .tgz from bytes (for e2e without npm).
pub fn extract_tgz_bytes(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let decoder = GzDecoder::new(std::io::Cursor::new(bytes));
    let mut archive = Archive::new(decoder);
    archive.unpack(dest).map_err(|e| format!("untar: {e}"))?;
    Ok(())
}

#[allow(dead_code)]
pub fn read_file_to_map(root: &Path) -> Result<HashMap<String, String>, String> {
    let mut files = HashMap::new();
    fn walk(root: &Path, dir: &Path, files: &mut HashMap<String, String>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(root, &path, files)?;
            } else if path.is_file() {
                let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
                let mut buf = String::new();
                // only text
                if f.read_to_string(&mut buf).is_ok() {
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/");
                    files.insert(rel, buf);
                }
            }
        }
        Ok(())
    }
    walk(root, root, &mut files)?;
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_npm_specs() {
        assert_eq!(
            parse_npm_spec("npm:pi-skills").unwrap(),
            ("pi-skills".into(), None)
        );
        assert_eq!(
            parse_npm_spec("npm:pi-skills@1.0.0").unwrap(),
            ("pi-skills".into(), Some("1.0.0".into()))
        );
        assert_eq!(
            parse_npm_spec("npm:@foo/bar@2").unwrap(),
            ("@foo/bar".into(), Some("2".into()))
        );
    }

    #[test]
    fn finds_skills_under_package() {
        let dir = tempfile::TempDir::new().unwrap();
        let skills = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(skills.join("SKILL.md"), "# Demo\n").unwrap();
        let found = find_skill_dirs_in_package(dir.path());
        assert_eq!(found.len(), 1);
    }
}
