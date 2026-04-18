//! SDK tool registry.
//!
//! [`SdkToolRegistry`] wraps the internal [`ToolRegistry`] with a stable API
//! for registering custom tools at runtime.

use std::sync::Arc;

use edgecrab_tools::registry::{ToolContext, ToolHandler, ToolRegistry};
use edgecrab_types::ToolSchema;

/// Stable SDK wrapper around the internal [`ToolRegistry`].
///
/// Provides methods to register custom tools and query available tools.
///
/// # Example
///
/// ```rust,no_run
/// use edgecrab_sdk_core::SdkToolRegistry;
///
/// let mut registry = SdkToolRegistry::new();
/// let tool_names = registry.tool_names();
/// ```
pub struct SdkToolRegistry {
    inner: ToolRegistry,
}

impl SdkToolRegistry {
    /// Create a new registry with all built-in tools auto-discovered.
    pub fn new() -> Self {
        Self {
            inner: ToolRegistry::new(),
        }
    }

    /// Register a custom tool at runtime.
    ///
    /// This is the primary way SDK users add tools — it calls
    /// [`ToolRegistry::register_dynamic`] internally.
    pub fn register(&mut self, handler: Box<dyn ToolHandler>) {
        self.inner.register_dynamic(handler);
    }

    /// List all available tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        self.inner.tool_names()
    }

    /// List all available toolset names.
    pub fn toolset_names(&self) -> Vec<&str> {
        self.inner.toolset_names()
    }

    /// List tools belonging to a specific toolset.
    pub fn tools_in_toolset(&self, toolset: &str) -> Vec<&str> {
        self.inner.tools_in_toolset(toolset)
    }

    /// Get a summary of toolsets and their tool counts.
    pub fn toolset_summary(&self) -> Vec<(String, usize)> {
        self.inner.toolset_summary()
    }

    /// Get the tool schemas for a set of enabled/disabled toolset filters.
    pub fn get_definitions(
        &self,
        enabled: Option<&[String]>,
        disabled: Option<&[String]>,
        ctx: &ToolContext,
    ) -> Vec<ToolSchema> {
        self.inner.get_definitions(enabled, disabled, ctx)
    }

    /// Consume into the inner [`ToolRegistry`].
    pub fn into_inner(self) -> ToolRegistry {
        self.inner
    }

    /// Get a shared reference to the inner registry.
    pub fn as_inner(&self) -> &ToolRegistry {
        &self.inner
    }

    /// Wrap into an `Arc` for use with [`SdkAgent`](crate::SdkAgent).
    pub fn into_arc(self) -> Arc<ToolRegistry> {
        Arc::new(self.inner)
    }
}

impl Default for SdkToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ToolRegistry> for SdkToolRegistry {
    fn from(inner: ToolRegistry) -> Self {
        Self { inner }
    }
}

// Re-export the ToolHandler trait so SDK users can implement it
pub use edgecrab_tools::registry::{ToolContext as SdkToolContext, ToolHandler as SdkToolHandler};
