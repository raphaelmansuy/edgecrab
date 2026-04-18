//! Python Config bindings.

use pyo3::prelude::*;

use edgecrab_sdk_core::{SdkConfig, edgecrab_home, ensure_edgecrab_home};

use crate::error::sdk_err;

/// EdgeCrab configuration.
#[pyclass(name = "Config")]
#[derive(Clone)]
pub struct PyConfig {
    pub(crate) inner: SdkConfig,
}

#[pymethods]
impl PyConfig {
    /// Load configuration from the default location (~/.edgecrab/config.yaml).
    #[staticmethod]
    fn load() -> PyResult<Self> {
        let inner = SdkConfig::load().map_err(sdk_err)?;
        Ok(Self { inner })
    }

    /// Load configuration from a specific path.
    #[staticmethod]
    fn load_from(path: &str) -> PyResult<Self> {
        let inner = SdkConfig::load_from(path).map_err(sdk_err)?;
        Ok(Self { inner })
    }

    /// Load configuration for a named profile under the current EdgeCrab home.
    #[staticmethod]
    fn load_profile(name: &str) -> PyResult<Self> {
        let inner = SdkConfig::load_profile(name).map_err(sdk_err)?;
        Ok(Self { inner })
    }

    /// Create a default configuration.
    #[staticmethod]
    fn default_config() -> Self {
        Self {
            inner: SdkConfig::default_config(),
        }
    }

    /// Get the default model string (e.g., "anthropic/claude-sonnet-4").
    #[getter]
    fn default_model(&self) -> String {
        self.inner.default_model().to_string()
    }

    /// Set the default model string.
    #[setter]
    fn set_default_model(&mut self, model: String) {
        self.inner.set_default_model(model);
    }

    /// Get the max iterations setting.
    #[getter]
    fn max_iterations(&self) -> u32 {
        self.inner.max_iterations()
    }

    /// Set the max iterations.
    #[setter]
    fn set_max_iterations(&mut self, n: u32) {
        self.inner.set_max_iterations(n);
    }

    /// Get the temperature setting.
    #[getter]
    fn temperature(&self) -> Option<f32> {
        self.inner.temperature()
    }

    /// Set the temperature.
    #[setter]
    fn set_temperature(&mut self, t: Option<f32>) {
        self.inner.set_temperature(t);
    }

    /// Save the configuration to disk (~/.edgecrab/config.yaml).
    fn save(&self) -> PyResult<()> {
        self.inner.save().map_err(sdk_err)
    }

    fn __repr__(&self) -> String {
        format!("Config(default_model='{}')", self.inner.default_model())
    }
}

/// Get the EdgeCrab home directory path.
#[pyfunction]
#[pyo3(name = "edgecrab_home")]
pub fn py_edgecrab_home() -> String {
    edgecrab_home().to_string_lossy().to_string()
}

/// Ensure the EdgeCrab home directory and all subdirectories exist.
#[pyfunction]
#[pyo3(name = "ensure_edgecrab_home")]
pub fn py_ensure_edgecrab_home() -> PyResult<String> {
    let path = ensure_edgecrab_home()
        .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))?;
    Ok(path.to_string_lossy().to_string())
}
