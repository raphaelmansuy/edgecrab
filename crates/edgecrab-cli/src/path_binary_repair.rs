//! Launch-time repair for stale `edgecrab` binaries on `PATH`.
//!
//! Older installs (commonly `~/.cargo/bin/edgecrab`) can shadow a newer npm /
//! Homebrew / cargo install and surface errors like
//! `Unknown LLM provider: omlx`. When a newer binary starts, it renames
//! strictly older native siblings to `edgecrab.<ver>.bak` (never deletes).
//!
//! Opt out: `EDGECRAB_NO_PATH_REPAIR=1`.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Running binary version from the Cargo package (same as `--version` first token).
pub fn running_package_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// True when auto-repair is disabled via env.
pub fn path_repair_disabled() -> bool {
    match std::env::var("EDGECRAB_NO_PATH_REPAIR") {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

/// Parse `edgecrab X.Y.Z` (or `edgecrab-cli X.Y.Z`) from `--version` stdout.
pub fn parse_edgecrab_version_line(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let line = line.trim();
        let mut parts = line.split_whitespace();
        let name = parts.next().unwrap_or("");
        if name == "edgecrab" || name == "edgecrab-cli" || name.ends_with("/edgecrab") {
            if let Some(ver) = parts.next() {
                let ver = ver.trim_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-');
                if !ver.is_empty() {
                    return Some(ver.to_string());
                }
            }
        }
    }
    None
}

pub fn probe_edgecrab_version(path: &Path) -> Option<String> {
    let output = std::process::Command::new(path)
        .arg("--version")
        .env_remove("EDGECRAB_HOME")
        // Avoid recursive repair while probing other binaries.
        .env("EDGECRAB_NO_PATH_REPAIR", "1")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_edgecrab_version_line(&stdout)
}

pub fn same_executable(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// True for ELF / Mach-O / PE native executables (not shell/Node/Python wrappers).
pub fn is_native_binary(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut magic = [0u8; 4];
    if file.read_exact(&mut magic).is_err() {
        return false;
    }
    matches!(
        &magic,
        b"\x7fELF" // Linux ELF
            | b"\xcf\xfa\xed\xfe" // Mach-O 64-bit LE
            | b"\xce\xfa\xed\xfe" // Mach-O 32-bit LE
            | b"\xca\xfe\xba\xbe" // Mach-O fat
            | [b'M', b'Z', _, _] // PE / DOS stub
    )
}

/// Parse `MAJOR.MINOR.PATCH` (optional `-pre` suffix ignored for ordering).
fn parse_semver_triplet(ver: &str) -> Option<(u64, u64, u64)> {
    let core = ver.split('-').next().unwrap_or(ver);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// True when `other` is a strictly older semver than `running`.
pub fn version_is_strictly_older(other: &str, running: &str) -> bool {
    match (parse_semver_triplet(other), parse_semver_triplet(running)) {
        (Some(a), Some(b)) => a < b,
        _ => false,
    }
}

fn bin_name() -> &'static str {
    if cfg!(windows) {
        "edgecrab.exe"
    } else {
        "edgecrab"
    }
}

fn bak_path_for(candidate: &Path, version: &str) -> PathBuf {
    let parent = candidate.parent().unwrap_or_else(|| Path::new("."));
    let safe_ver: String = version
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
            c
        } else {
            '_'
        })
        .collect();
    let base = format!("edgecrab.{safe_ver}.bak");
    let mut dest = parent.join(&base);
    if !dest.exists() {
        return dest;
    }
    for n in 2..100 {
        dest = parent.join(format!("edgecrab.{safe_ver}.bak.{n}"));
        if !dest.exists() {
            return dest;
        }
    }
    parent.join(format!(
        "edgecrab.{safe_ver}.bak.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ))
}

/// Rename strictly older native `edgecrab` entries on `PATH` to `.bak`.
///
/// Returns the list of `(from, to)` renames performed.
pub fn repair_stale_path_binaries() -> Vec<(PathBuf, PathBuf)> {
    repair_stale_path_binaries_with_running(
        running_package_version(),
        std::env::current_exe().ok().as_deref(),
    )
}

pub fn repair_stale_path_binaries_with_running(
    running_ver: &str,
    self_exe: Option<&Path>,
) -> Vec<(PathBuf, PathBuf)> {
    if path_repair_disabled() {
        return Vec::new();
    }

    let path_env = std::env::var_os("PATH").unwrap_or_default();
    let name = bin_name();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();

    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(name);
        if !candidate.is_file() {
            continue;
        }
        // Never rename npm/pipx/shell wrappers — only native binaries.
        if !is_native_binary(&candidate) {
            continue;
        }
        if seen.iter().any(|p| same_executable(p, &candidate)) {
            continue;
        }
        if let Some(self_path) = self_exe
            && same_executable(self_path, &candidate)
        {
            seen.push(candidate);
            continue;
        }
        seen.push(candidate.clone());

        let Some(other_ver) = probe_edgecrab_version(&candidate) else {
            // Unknown / SDK wrappers — do not rename.
            continue;
        };
        if !version_is_strictly_older(&other_ver, running_ver) {
            continue;
        }

        let dest = bak_path_for(&candidate, &other_ver);
        match std::fs::rename(&candidate, &dest) {
            Ok(()) => {
                renamed.push((candidate, dest));
            }
            Err(err) => {
                let _ = writeln!(
                    std::io::stderr(),
                    "[edgecrab] PATH repair: could not rename {}: {err}",
                    candidate.display()
                );
            }
        }
    }

    if !renamed.is_empty() {
        let summary: Vec<String> = renamed
            .iter()
            .map(|(from, to)| format!("{} → {}", from.display(), to.display()))
            .collect();
        let _ = writeln!(
            std::io::stderr(),
            "[edgecrab] PATH repair: renamed older edgecrab install(s) so {running_ver} is not shadowed — {}. Opt out: EDGECRAB_NO_PATH_REPAIR=1",
            summary.join("; ")
        );
    }

    renamed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    #[test]
    fn parse_edgecrab_version_line_reads_cli_output() {
        assert_eq!(
            parse_edgecrab_version_line("edgecrab 0.12.0\n").as_deref(),
            Some("0.12.0")
        );
        assert_eq!(
            parse_edgecrab_version_line("edgecrab 0.9.0").as_deref(),
            Some("0.9.0")
        );
        assert!(parse_edgecrab_version_line("edgecrab-sdk 0.4.1").is_none());
    }

    #[test]
    fn version_is_strictly_older_compares_triplets() {
        assert!(version_is_strictly_older("0.9.0", "0.12.1"));
        assert!(version_is_strictly_older("0.12.0", "0.12.1"));
        assert!(!version_is_strictly_older("0.12.1", "0.12.1"));
        assert!(!version_is_strictly_older("0.13.0", "0.12.1"));
        assert!(!version_is_strictly_older("unknown", "0.12.1"));
    }

    #[cfg(unix)]
    fn plant_native_fake(dir: &Path, version_line: &str) -> PathBuf {
        let src = dir.join("fake.c");
        let bin = dir.join("edgecrab");
        std::fs::write(
            &src,
            format!(
                "#include <stdio.h>\nint main(void) {{ puts(\"{version_line}\"); return 0; }}\n"
            ),
        )
        .expect("write c");
        // Absolute compiler path — tests isolate PATH to `dir` only.
        let cc = if Path::new("/usr/bin/cc").exists() {
            "/usr/bin/cc"
        } else if Path::new("/usr/bin/clang").exists() {
            "/usr/bin/clang"
        } else {
            "cc"
        };
        let status = std::process::Command::new(cc)
            .args(["-O0", "-o"])
            .arg(&bin)
            .arg(&src)
            .status()
            .unwrap_or_else(|e| panic!("spawn {cc}: {e}"));
        assert!(status.success(), "{cc} failed to build fake edgecrab");
        bin
    }

    #[cfg(unix)]
    #[test]
    fn repair_renames_older_shadow_on_path() {
        let tmp = TempDir::new().expect("tmp");
        let fake = plant_native_fake(tmp.path(), "edgecrab 0.9.0");

        let old_path = std::env::var_os("PATH");
        let old_opt_out = std::env::var_os("EDGECRAB_NO_PATH_REPAIR");
        // SAFETY: test-only env — PATH is ONLY the temp dir (never real PATH).
        unsafe {
            std::env::set_var(
                "PATH",
                std::env::join_paths([tmp.path()]).expect("join"),
            );
            std::env::remove_var("EDGECRAB_NO_PATH_REPAIR");
        }

        let renamed = repair_stale_path_binaries_with_running("0.12.1", None);
        assert_eq!(renamed.len(), 1, "expected one rename, got {renamed:?}");
        assert!(!fake.exists(), "old path should be gone");
        assert!(renamed[0].1.exists(), "bak should exist");
        assert!(
            renamed[0]
                .1
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.contains("0.9.0") && n.ends_with(".bak")),
            "bak name = {:?}",
            renamed[0].1
        );

        // SAFETY: restore env.
        unsafe {
            match old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
            match old_opt_out {
                Some(v) => std::env::set_var("EDGECRAB_NO_PATH_REPAIR", v),
                None => std::env::remove_var("EDGECRAB_NO_PATH_REPAIR"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn repair_skips_equal_or_newer_and_opt_out() {
        let tmp = TempDir::new().expect("tmp");
        let fake = plant_native_fake(tmp.path(), "edgecrab 0.12.1");

        let old_path = std::env::var_os("PATH");
        let old_opt_out = std::env::var_os("EDGECRAB_NO_PATH_REPAIR");
        // SAFETY: test-only env — PATH is ONLY the temp dir.
        unsafe {
            std::env::set_var(
                "PATH",
                std::env::join_paths([tmp.path()]).expect("join"),
            );
            std::env::remove_var("EDGECRAB_NO_PATH_REPAIR");
        }

        let renamed = repair_stale_path_binaries_with_running("0.12.1", None);
        assert!(renamed.is_empty());
        assert!(fake.exists());

        // Rebuild as older, then opt out.
        let _ = plant_native_fake(tmp.path(), "edgecrab 0.9.0");
        // SAFETY: opt-out.
        unsafe {
            std::env::set_var("EDGECRAB_NO_PATH_REPAIR", "1");
        }
        let renamed = repair_stale_path_binaries_with_running("0.12.1", None);
        assert!(renamed.is_empty());
        assert!(fake.exists());

        // SAFETY: restore.
        unsafe {
            match old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
            match old_opt_out {
                Some(v) => std::env::set_var("EDGECRAB_NO_PATH_REPAIR", v),
                None => std::env::remove_var("EDGECRAB_NO_PATH_REPAIR"),
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn repair_skips_shell_script_wrappers() {
        let tmp = TempDir::new().expect("tmp");
        let fake = tmp.path().join("edgecrab");
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(&fake, "#!/bin/sh\necho 'edgecrab 0.9.0'\n").expect("write");
        let mut perms = std::fs::metadata(&fake).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake, perms).expect("chmod");

        let old_path = std::env::var_os("PATH");
        let old_opt_out = std::env::var_os("EDGECRAB_NO_PATH_REPAIR");
        // SAFETY: isolated PATH.
        unsafe {
            std::env::set_var(
                "PATH",
                std::env::join_paths([tmp.path()]).expect("join"),
            );
            std::env::remove_var("EDGECRAB_NO_PATH_REPAIR");
        }

        let renamed = repair_stale_path_binaries_with_running("0.12.1", None);
        assert!(renamed.is_empty(), "must not rename script wrappers");
        assert!(fake.exists());

        // SAFETY: restore.
        unsafe {
            match old_path {
                Some(p) => std::env::set_var("PATH", p),
                None => std::env::remove_var("PATH"),
            }
            match old_opt_out {
                Some(v) => std::env::set_var("EDGECRAB_NO_PATH_REPAIR", v),
                None => std::env::remove_var("EDGECRAB_NO_PATH_REPAIR"),
            }
        }
    }
}
