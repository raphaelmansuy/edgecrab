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

mod approval_runtime;
mod command_interaction;
pub mod config_ref;
pub mod execution_fs;
pub mod execution_tmp;
pub mod fuzzy_match;
#[cfg(target_os = "macos")]
pub mod macos_permissions;
#[cfg(not(target_os = "macos"))]
#[path = "macos_permissions_stub.rs"]
pub mod macos_permissions;
pub mod path_utils;
pub mod process_table;
pub mod read_tracker;
pub mod registry;
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
pub use registry::{
    SubAgentResult, SubAgentRunner, ToolContext, ToolHandler, ToolRegistry, to_llm_definitions,
};
pub use tools::todo::TodoStore;
pub use toolsets::{ACP_TOOLS, CORE_TOOLS, resolve_active_toolsets, resolve_alias};
