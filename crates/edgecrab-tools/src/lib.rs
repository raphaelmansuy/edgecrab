//! # edgecrab-tools
//!
//! Tool registry (`ToolHandler` trait), toolset composition, and all tool
//! implementations. Uses `inventory` crate for compile-time registration.
//!
//! ```text
//!   edgecrab-tools
//!     ├── registry.rs     — ToolHandler trait, ToolRegistry, ToolContext
//!     ├── config_ref.rs   — AppConfigRef (lightweight config for tool context)
//!     ├── toolsets.rs      — CORE_TOOLS, ACP_TOOLS, alias resolution
//!     └── tools/           — individual tool implementations (Phase 2.2+)
//! ```

#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(clippy::result_large_err)]

pub mod approval_runtime;
mod command_interaction;
pub mod config_ref;
pub mod edit_contract;
pub mod execution_fs;
pub mod execution_tmp;
pub mod fuzzy_match;
mod local_pty;
#[cfg(target_os = "macos")]
pub mod macos_permissions;
#[cfg(not(target_os = "macos"))]
#[path = "macos_permissions_stub.rs"]
pub mod macos_permissions;
pub mod path_utils;
pub mod process_table;
pub mod provider_factory;
pub mod read_tracker;
pub mod registry;
mod shell_syntax;
pub mod tools;
pub mod toolsets;
pub mod vision_models;

/// Truncate `s` to at most `max_bytes` bytes, always stopping at a valid UTF-8
/// char boundary so that multi-byte / emoji characters are never split.
#[inline]
pub(crate) fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let boundary = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..boundary]
}

pub use config_ref::AppConfigRef;
pub use execution_fs::{ExecutionFilesystemView, describe_execution_filesystem};
pub use process_table::ProcessTable;
pub use provider_factory::{build_copilot_provider, create_provider_for_model};
pub use registry::{
    SubAgentResult, SubAgentRunner, ToolContext, ToolHandler, ToolProgressUpdate, ToolRegistry,
    to_llm_definitions,
};
pub use tools::todo::TodoStore;
pub use toolsets::{ACP_TOOLS, CORE_TOOLS, resolve_active_toolsets, resolve_alias};

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;
    use std::sync::{Mutex, MutexGuard};

    use tempfile::TempDir;

    static EDGECRAB_HOME_LOCK: Mutex<()> = Mutex::new(());

    pub(crate) struct TestEdgecrabHome {
        _guard: MutexGuard<'static, ()>,
        dir: TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestEdgecrabHome {
        pub(crate) fn new() -> Self {
            let guard = EDGECRAB_HOME_LOCK.lock().expect("lock");
            let dir = TempDir::new().expect("tempdir");
            let previous = std::env::var_os("EDGECRAB_HOME");
            // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
            unsafe { std::env::set_var("EDGECRAB_HOME", dir.path()) };
            Self {
                _guard: guard,
                dir,
                previous,
            }
        }

        pub(crate) fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    impl Drop for TestEdgecrabHome {
        fn drop(&mut self) {
            match &self.previous {
                Some(previous) => {
                    // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
                    unsafe { std::env::set_var("EDGECRAB_HOME", previous) };
                }
                None => {
                    // SAFETY: serialized by EDGECRAB_HOME_LOCK for the guard lifetime.
                    unsafe { std::env::remove_var("EDGECRAB_HOME") };
                }
            }
        }
    }
}
