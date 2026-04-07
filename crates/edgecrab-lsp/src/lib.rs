#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod capability;
pub mod config;
pub mod diagnostics;
pub mod edit;
pub mod enrichment;
pub mod error;
pub mod manager;
pub mod position;
pub mod protocol;
pub mod render;
pub mod sync;
pub mod tools;

pub use diagnostics::DiagnosticCache;
pub use error::LspError;
pub use manager::{LspRuntime, runtime_for_ctx};
