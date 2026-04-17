//! Python error types — maps SdkError to Python exceptions.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use edgecrab_sdk_core::SdkError;

/// Convert an SdkError to a PyErr.
pub(crate) fn sdk_err(e: SdkError) -> PyErr {
    match e {
        SdkError::Config(message) => PyValueError::new_err(format!("ConfigError: {message}")),
        SdkError::Provider { model, message } => {
            PyValueError::new_err(format!("ProviderError({model}): {message}"))
        }
        SdkError::Agent(source) => PyRuntimeError::new_err(format!("AgentError: {source}")),
        SdkError::Tool(source) => PyRuntimeError::new_err(format!("ToolError: {source}")),
        SdkError::Serialization(source) => {
            PyValueError::new_err(format!("SerializationError: {source}"))
        }
        SdkError::NotInitialized(component) => {
            PyRuntimeError::new_err(format!("NotInitialized: {component}"))
        }
    }
}
