//! # EdgeCrab Python SDK — PyO3 bindings
//!
//! Native Python bindings for the EdgeCrab AI agent runtime.
//!
//! This crate exposes `edgecrab-sdk-core` types to Python via PyO3,
//! providing a high-performance alternative to the HTTP-based SDK.

use pyo3::prelude::*;

mod agent;
mod config;
mod error;
mod types;

/// The native module — imported as `edgecrab._native` by the Python package.
#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Agent
    m.add_class::<agent::PyAgent>()?;
    m.add_class::<agent::PyMemoryManager>()?;

    // Config
    m.add_class::<config::PyConfig>()?;

    // Types
    m.add_class::<types::PyConversationResult>()?;
    m.add_class::<types::PyStreamEvent>()?;
    m.add_class::<types::PyModelCatalog>()?;
    m.add_class::<types::PySessionSummary>()?;
    m.add_class::<types::PySessionSearchHit>()?;
    m.add_class::<types::PySession>()?;
    m.add_class::<types::PySessionStats>()?;

    // Functions
    m.add_function(wrap_pyfunction!(config::py_edgecrab_home, m)?)?;
    m.add_function(wrap_pyfunction!(config::py_ensure_edgecrab_home, m)?)?;

    Ok(())
}
