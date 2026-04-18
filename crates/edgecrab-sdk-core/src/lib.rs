#![allow(clippy::result_large_err)]

//! # edgecrab-sdk-core
//!
//! Stable facade between the EdgeCrab agent runtime and language-specific SDKs.
//!
//! This crate provides a **semver-stable** API surface that wraps the internal
//! `edgecrab-core`, `edgecrab-tools`, and `edgecrab-state` crates. All foreign
//! bindings (PyO3, napi-rs) depend on this crate rather than on internals directly,
//! so internal refactors do not break downstream SDKs.
//!
//! # Quick Start (Rust)
//!
//! ```rust,no_run
//! use edgecrab_sdk_core::{SdkAgent, SdkConfig, SdkError};
//!
//! # async fn example() -> Result<(), SdkError> {
//! let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
//! let reply = agent.chat("What is EdgeCrab?").await?;
//! println!("{reply}");
//! # Ok(())
//! # }
//! ```

pub mod agent;
pub mod config;
pub mod convert;
pub mod error;
pub mod memory;
pub mod session;
pub mod tools;
pub mod types;

// ── Re-exports (public SDK surface) ──────────────────────────────────

pub use agent::SdkAgent;
pub use agent::SdkAgentBuilder;
pub use agent::SessionExport;
pub use config::SdkConfig;
pub use error::SdkError;
pub use memory::MemoryManager;
pub use session::SdkSession;
pub use tools::SdkToolRegistry;

// Re-export core types that are part of the public API
pub use edgecrab_core::agent::{
    AgentBuilder, ApprovalChoice, ConversationResult, IsolatedAgentOptions, SessionSnapshot,
    StreamEvent,
};
pub use edgecrab_types::{
    AgentError, Content, ContentPart, Cost, Message, OriginChat, Platform, Role, ToolCall,
    ToolError, ToolErrorRecord, ToolSchema, Usage,
};

// Re-export session types
pub use edgecrab_state::{SessionRecord, SessionSearchHit, SessionStats, SessionSummary};

// Re-export profile/home directory utilities
pub use edgecrab_core::config::{edgecrab_home, ensure_edgecrab_home};

// Re-export tool handler trait and context for custom tools
pub use edgecrab_tools::registry::{ToolContext, ToolHandler};

// Re-export the provider factory for advanced usage
pub use edgecrab_tools::create_provider_for_model;
