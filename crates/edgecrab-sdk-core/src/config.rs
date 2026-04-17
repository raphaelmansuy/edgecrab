//! SDK configuration.
//!
//! [`SdkConfig`] wraps [`AppConfig`](edgecrab_core::config::AppConfig) with a
//! stable, builder-style API. It also handles provider auto-creation from the
//! model string.

use std::path::{Path, PathBuf};

use crate::error::SdkError;

/// SDK configuration loaded from `~/.edgecrab/config.yaml` or built programmatically.
///
/// # Examples
///
/// ```rust,no_run
/// # use edgecrab_sdk_core::SdkConfig;
/// // Load from default path
/// let config = SdkConfig::load().unwrap();
///
/// // Load from explicit path
/// let config = SdkConfig::load_from("./my-config.yaml").unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct SdkConfig {
    pub(crate) inner: edgecrab_core::config::AppConfig,
}

impl SdkConfig {
    /// Load configuration from the default path (`~/.edgecrab/config.yaml`),
    /// with environment variable overrides applied.
    pub fn load() -> Result<Self, SdkError> {
        let inner = edgecrab_core::config::AppConfig::load()
            .map_err(|e| SdkError::Config(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Load from an explicit path.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, SdkError> {
        let inner = edgecrab_core::config::AppConfig::load_from(path.as_ref())
            .map_err(|e| SdkError::Config(e.to_string()))?;
        Ok(Self { inner })
    }

    /// Load the configuration for a named profile under the current EdgeCrab home.
    pub fn load_profile(name: impl AsRef<str>) -> Result<Self, SdkError> {
        let path = Self::profile_config_path(name);
        Self::load_from(path)
    }

    /// Resolve the config path for a named profile.
    pub fn profile_config_path(name: impl AsRef<str>) -> PathBuf {
        edgecrab_core::config::edgecrab_home()
            .join("profiles")
            .join(name.as_ref())
            .join("config.yaml")
    }

    /// Create a config with all defaults.
    pub fn default_config() -> Self {
        Self {
            inner: edgecrab_core::config::AppConfig::default(),
        }
    }

    /// Save the configuration to `~/.edgecrab/config.yaml`.
    pub fn save(&self) -> Result<(), SdkError> {
        self.inner
            .save()
            .map_err(|e| SdkError::Config(e.to_string()))
    }

    /// Get the default model string (e.g. `"anthropic/claude-sonnet-4"`).
    pub fn default_model(&self) -> &str {
        &self.inner.model.default_model
    }

    /// Set the default model.
    pub fn set_default_model(&mut self, model: impl Into<String>) {
        self.inner.model.default_model = model.into();
    }

    /// Get the max iterations for the agent loop.
    pub fn max_iterations(&self) -> u32 {
        self.inner.model.max_iterations
    }

    /// Set the max iterations.
    pub fn set_max_iterations(&mut self, n: u32) {
        self.inner.model.max_iterations = n;
    }

    /// Get the temperature setting.
    pub fn temperature(&self) -> Option<f32> {
        self.inner.model.temperature
    }

    /// Set the temperature.
    pub fn set_temperature(&mut self, t: Option<f32>) {
        self.inner.model.temperature = t;
    }

    /// Get a reference to the underlying `AppConfig`.
    pub fn as_inner(&self) -> &edgecrab_core::config::AppConfig {
        &self.inner
    }

    /// Consume and return the underlying `AppConfig`.
    pub fn into_inner(self) -> edgecrab_core::config::AppConfig {
        self.inner
    }
}

impl From<edgecrab_core::config::AppConfig> for SdkConfig {
    fn from(inner: edgecrab_core::config::AppConfig) -> Self {
        Self { inner }
    }
}

impl From<SdkConfig> for edgecrab_core::config::AppConfig {
    fn from(sdk: SdkConfig) -> Self {
        sdk.inner
    }
}
