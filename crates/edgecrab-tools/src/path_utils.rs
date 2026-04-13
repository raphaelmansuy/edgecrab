//! Path utilities for tool implementations.
//!
//! WHY wrappers instead of open-coded checks in every tool: the actual path
//! policy lives in `edgecrab-security`, but tools need `ToolError`-shaped
//! failures. Keeping the mapping here preserves one policy with one adapter.

use std::path::{Path, PathBuf};

use edgecrab_security::path_policy::{PathPolicy, PathPolicyError};
use edgecrab_types::ToolError;

fn map_path_policy_error(err: PathPolicyError) -> ToolError {
    match err {
        PathPolicyError::NotFound(path) => {
            ToolError::NotFound(format!("Cannot resolve path '{path}'"))
        }
        PathPolicyError::PermissionDenied(message) => ToolError::PermissionDenied(message),
        PathPolicyError::InvalidRoot(message) => ToolError::Other(message),
    }
}

fn normalize_user_path_input(path: &str) -> String {
    path.trim()
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' {
                std::path::MAIN_SEPARATOR
            } else {
                ch
            }
        })
        .collect()
}

/// Resolve and jail a path for **read** operations, accepting additional trusted roots.
pub fn jail_read_path_multi(
    path: &str,
    policy: &PathPolicy,
    trusted_roots: &[&Path],
) -> Result<PathBuf, ToolError> {
    let normalized = normalize_user_path_input(path);
    policy
        .resolve_read_path(Path::new(&normalized), trusted_roots)
        .map_err(map_path_policy_error)
}

/// Resolve and jail a path for **read** operations.
pub fn jail_read_path(path: &str, policy: &PathPolicy) -> Result<PathBuf, ToolError> {
    jail_read_path_multi(path, policy, &[])
}

/// Resolve and jail a path for **write** operations.
pub fn jail_write_path(path: &str, policy: &PathPolicy) -> Result<PathBuf, ToolError> {
    let normalized = normalize_user_path_input(path);
    policy
        .resolve_write_path(Path::new(&normalized), false)
        .map_err(map_path_policy_error)
}

/// Same as `jail_write_path` but creates intermediate directories first.
pub fn jail_write_path_create_dirs(path: &str, policy: &PathPolicy) -> Result<PathBuf, ToolError> {
    let normalized = normalize_user_path_input(path);
    policy
        .resolve_write_path(Path::new(&normalized), true)
        .map_err(map_path_policy_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn policy_for(dir: &Path) -> PathPolicy {
        PathPolicy::new(dir.to_path_buf())
    }

    fn policy_with_virtual_tmp(dir: &Path, virtual_tmp: &Path) -> PathPolicy {
        PathPolicy::new(dir.to_path_buf()).with_virtual_tmp_root(virtual_tmp.to_path_buf())
    }

    #[test]
    fn jail_read_allows_existing_file() {
        let dir = TempDir::new().expect("tmpdir");
        std::fs::write(dir.path().join("hello.txt"), "hi").expect("write");

        let result = jail_read_path("hello.txt", &policy_for(dir.path()));
        assert!(result.is_ok());
    }

    #[test]
    fn jail_read_blocks_traversal() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_read_path("../../../etc/passwd", &policy_for(dir.path()));
        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn jail_read_rejects_nonexistent() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_read_path("no_such_file.txt", &policy_for(dir.path()));
        assert!(matches!(result, Err(ToolError::NotFound(_))));
    }

    #[test]
    fn jail_write_allows_valid_path() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_write_path("new_file.txt", &policy_for(dir.path()));
        assert!(result.is_ok());
    }

    #[test]
    fn jail_write_blocks_traversal() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_write_path("../escaped.txt", &policy_for(dir.path()));
        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[cfg(unix)]
    #[test]
    fn jail_read_normalizes_windows_separators() {
        let dir = TempDir::new().expect("tmpdir");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(nested.join("hello.txt"), "hi").expect("write");

        let result = jail_read_path("nested\\hello.txt", &policy_for(dir.path()));
        assert!(result.is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn jail_read_normalizes_unix_separators() {
        let dir = TempDir::new().expect("tmpdir");
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(nested.join("hello.txt"), "hi").expect("write");

        let result = jail_read_path("nested/hello.txt", &policy_for(dir.path()));
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn jail_write_blocks_windows_style_traversal() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_write_path("..\\escaped.txt", &policy_for(dir.path()));
        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[cfg(windows)]
    #[test]
    fn jail_write_blocks_unix_style_traversal() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_write_path("../escaped.txt", &policy_for(dir.path()));
        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn jail_write_create_dirs_works() {
        let dir = TempDir::new().expect("tmpdir");
        let result = jail_write_path_create_dirs("sub/dir/file.txt", &policy_for(dir.path()));
        assert!(result.is_ok());
        assert!(dir.path().join("sub/dir").exists());
    }

    #[test]
    fn multi_allows_file_under_extra_root() {
        let dir = TempDir::new().expect("tmpdir");
        let extra = TempDir::new().expect("tmpdir");
        let extra_file = extra.path().join("img.png");
        std::fs::write(&extra_file, b"\x89PNG").expect("write");

        let result = jail_read_path_multi(
            &extra_file.to_string_lossy(),
            &policy_for(dir.path()),
            &[extra.path()],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn multi_blocks_file_outside_all_roots() {
        let dir = TempDir::new().expect("tmpdir");
        let extra = TempDir::new().expect("tmpdir");
        let outside = TempDir::new().expect("tmpdir");
        let outside_file = outside.path().join("outside.txt");
        std::fs::write(&outside_file, "nope").expect("write");

        let result = jail_read_path_multi(
            &outside_file.to_string_lossy(),
            &policy_for(dir.path()),
            &[extra.path()],
        );

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn configured_allowed_roots_permit_absolute_paths() {
        let dir = TempDir::new().expect("tmpdir");
        let extra = TempDir::new().expect("tmpdir");
        let extra_file = extra.path().join("shared.txt");
        std::fs::write(&extra_file, "shared").expect("write");

        let policy =
            PathPolicy::new(dir.path().to_path_buf()).with_allowed_roots(vec![extra.path().into()]);
        let result = jail_read_path(&extra_file.to_string_lossy(), &policy);

        assert!(result.is_ok());
    }

    #[test]
    fn path_restrictions_override_workspace_root() {
        let dir = TempDir::new().expect("tmpdir");
        let blocked_dir = dir.path().join("blocked");
        std::fs::create_dir_all(&blocked_dir).expect("create blocked");
        std::fs::write(blocked_dir.join("secret.txt"), "secret").expect("write");

        let policy = PathPolicy::new(dir.path().to_path_buf())
            .with_denied_roots(vec![PathBuf::from("blocked")]);
        let result = jail_read_path("blocked/secret.txt", &policy);

        assert!(matches!(result, Err(ToolError::PermissionDenied(_))));
    }

    #[test]
    fn vision_whatsapp_image_cache_is_trusted() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let image_cache = edgecrab_home.path().join("image_cache");
        std::fs::create_dir_all(&image_cache).expect("create image_cache");
        let image_path = image_cache.join("img_deadbeef.jpg");
        std::fs::write(&image_path, b"\xff\xd8\xff").expect("write jpg");

        let result = jail_read_path_multi(
            &image_path.to_string_lossy(),
            &policy_for(cwd.path()),
            &[image_cache.as_path()],
        );

        assert!(result.is_ok(), "WhatsApp image cache path must be trusted");
    }

    #[test]
    fn vision_telegram_gateway_media_is_trusted() {
        let cwd = TempDir::new().expect("cwd");
        let edgecrab_home = TempDir::new().expect("edgecrab_home");
        let gateway_media = edgecrab_home.path().join("gateway_media");
        let telegram_dir = gateway_media.join("telegram");
        std::fs::create_dir_all(&telegram_dir).expect("create gateway_media/telegram");
        let image_path = telegram_dir.join("photo.png");
        std::fs::write(&image_path, b"\x89PNG").expect("write png");

        let result = jail_read_path_multi(
            &image_path.to_string_lossy(),
            &policy_for(cwd.path()),
            &[gateway_media.as_path()],
        );

        assert!(
            result.is_ok(),
            "Telegram gateway_media path must be trusted"
        );
    }

    #[test]
    fn jail_write_maps_absolute_tmp_into_virtual_tmp_root() {
        let dir = TempDir::new().expect("workspace");
        let virtual_tmp = TempDir::new().expect("virtual tmp");

        let resolved = jail_write_path(
            "/tmp/out.txt",
            &policy_with_virtual_tmp(dir.path(), virtual_tmp.path()),
        )
        .expect("map /tmp write");

        assert_eq!(
            resolved,
            virtual_tmp
                .path()
                .canonicalize()
                .expect("canon virtual tmp")
                .join("out.txt")
        );
    }

    #[test]
    fn jail_read_maps_absolute_tmp_into_virtual_tmp_root() {
        let dir = TempDir::new().expect("workspace");
        let virtual_tmp = TempDir::new().expect("virtual tmp");
        let mapped = virtual_tmp.path().join("summary.md");
        std::fs::write(&mapped, "hello").expect("write mapped tmp file");

        let resolved = jail_read_path(
            "/tmp/summary.md",
            &policy_with_virtual_tmp(dir.path(), virtual_tmp.path()),
        )
        .expect("map /tmp read");

        assert_eq!(resolved, mapped.canonicalize().expect("canon mapped"));
    }

    /// Non-existent extra_roots must be silently skipped — do NOT cause an
    /// `InvalidRoot` error.  This is the regression test for the Gateway
    /// `vision_analyze` failure: image_cache, gateway_media, and images dirs
    /// are created lazily and may not exist on a first run.
    #[test]
    fn non_existent_extra_root_is_skipped_not_fatal() {
        let cwd = TempDir::new().expect("cwd");
        let ghost_dir = cwd.path().join("does_not_exist");
        // ghost_dir is intentionally NOT created
        let real_file = cwd.path().join("hello.txt");
        std::fs::write(&real_file, "hi").expect("write");

        // Passing a non-existent extra root must not cause an error
        let result = jail_read_path_multi(
            &real_file.to_string_lossy(),
            &policy_for(cwd.path()),
            &[ghost_dir.as_path()],
        );
        assert!(result.is_ok(), "non-existent extra root must not be fatal");
    }

    /// A file that is ONLY under a non-existent extra_root should still be
    /// blocked — once skipped, the root grants no access.
    #[test]
    fn file_under_non_existent_extra_root_is_blocked() {
        let cwd = TempDir::new().expect("cwd");
        let other = TempDir::new().expect("other");
        let ghost_dir = PathBuf::from("/tmp/__edgecrab_ghost_test_dir_that_does_not_exist__");
        let outside_file = other.path().join("outside.txt");
        std::fs::write(&outside_file, "secret").expect("write");

        // ghost_dir doesn't exist → it's skipped → outside_file is NOT trusted
        let result = jail_read_path_multi(
            &outside_file.to_string_lossy(),
            &policy_for(cwd.path()),
            &[ghost_dir.as_path()],
        );
        assert!(
            matches!(result, Err(ToolError::PermissionDenied(_))),
            "file outside all real roots must be blocked"
        );
    }
}
