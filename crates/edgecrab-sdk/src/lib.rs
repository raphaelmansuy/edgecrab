//! # EdgeCrab SDK
//!
//! Build autonomous AI agents in Rust with a simple, ergonomic API.
//!
//! This crate re-exports everything from `edgecrab-sdk-core` and
//! `edgecrab-sdk-macros`, providing a single dependency for SDK users.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use edgecrab_sdk::prelude::*;
//!
//! # async fn example() -> Result<(), SdkError> {
//! let agent = SdkAgent::new("anthropic/claude-sonnet-4")?;
//! let reply = agent.chat("What is EdgeCrab?").await?;
//! println!("{reply}");
//! # Ok(())
//! # }
//! ```
//!
//! # Custom Tools
//!
//! ```rust,ignore
//! use edgecrab_sdk::prelude::*;
//!
//! /// Greet someone by name.
//! #[edgecrab_tool(name = "greet", toolset = "demo")]
//! async fn greet(name: String) -> Result<String, ToolError> {
//!     Ok(format!("Hello, {name}!"))
//! }
//! ```

/// The prelude — import everything you need with `use edgecrab_sdk::prelude::*`.
pub mod prelude {
    // ── Core types ───────────────────────────────────────────────────
    pub use edgecrab_sdk_core::{
        AgentBuilder,
        ApprovalChoice,
        Content,
        ContentPart,
        // Conversation types
        ConversationResult,
        Cost,
        IsolatedAgentOptions,
        // Message types
        Message,
        OriginChat,
        // Platform
        Platform,
        Role,
        // Agent
        SdkAgent,
        SdkAgentBuilder,
        SdkConfig,
        SdkError,
        SdkToolRegistry,
        SessionExport,
        // Session types
        SessionRecord,
        SessionSearchHit,
        SessionSnapshot,
        SessionSummary,
        StreamEvent,
        ToolCall,
        ToolContext,
        ToolError,
        ToolErrorRecord,
        ToolHandler,
        // Tool types
        ToolSchema,
        Usage,
        // Provider factory
        create_provider_for_model,
        // Profile/home
        edgecrab_home,
        ensure_edgecrab_home,
    };

    // ── Macros ───────────────────────────────────────────────────────
    pub use edgecrab_sdk_macros::edgecrab_tool;

    // ── Re-export commonly needed crates ─────────────────────────────
    pub use async_trait::async_trait;
    pub use inventory;
    pub use serde_json;
}

// ── Top-level re-exports for `use edgecrab_sdk::Agent` style ─────────
pub use edgecrab_sdk_core::*;
pub use edgecrab_sdk_macros::edgecrab_tool;
