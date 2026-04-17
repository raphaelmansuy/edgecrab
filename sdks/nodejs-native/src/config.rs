//! Config bindings — edgecrab home utilities.
//!
//! These exports are consumed from the generated JS wrapper at runtime; lib-test
//! builds used by clippy cannot see those JS references, so we suppress
//! dead-code noise for this binding module.
#![allow(dead_code)]

use edgecrab_sdk_core::SdkConfig;
use napi::Result;

fn sdk_err(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// EdgeCrab configuration.
#[napi(js_name = "Config")]
pub struct JsConfig {
    pub(crate) inner: SdkConfig,
}

#[napi]
impl JsConfig {
    /// Load configuration from the default location.
    #[napi(factory)]
    pub fn load() -> Result<Self> {
        Ok(Self {
            inner: SdkConfig::load().map_err(sdk_err)?,
        })
    }

    /// Load configuration from a specific path.
    #[napi(factory)]
    pub fn load_from(path: String) -> Result<Self> {
        Ok(Self {
            inner: SdkConfig::load_from(path).map_err(sdk_err)?,
        })
    }

    /// Load configuration for a named profile.
    #[napi(factory)]
    pub fn load_profile(name: String) -> Result<Self> {
        Ok(Self {
            inner: SdkConfig::load_profile(name).map_err(sdk_err)?,
        })
    }

    /// Create a default configuration.
    #[napi(factory)]
    pub fn default_config() -> Self {
        Self {
            inner: SdkConfig::default_config(),
        }
    }

    /// Get the default model string.
    #[napi(getter)]
    pub fn default_model(&self) -> String {
        self.inner.default_model().to_string()
    }

    /// Set the default model string.
    #[napi(setter)]
    pub fn set_default_model(&mut self, model: String) {
        self.inner.set_default_model(model);
    }

    /// Get the max iterations setting.
    #[napi(getter)]
    pub fn max_iterations(&self) -> u32 {
        self.inner.max_iterations()
    }

    /// Set the max iterations.
    #[napi(setter)]
    pub fn set_max_iterations(&mut self, n: u32) {
        self.inner.set_max_iterations(n);
    }

    /// Get the temperature setting.
    #[napi(getter)]
    pub fn temperature(&self) -> Option<f64> {
        self.inner.temperature().map(|t| t as f64)
    }

    /// Set the temperature.
    #[napi(setter)]
    pub fn set_temperature(&mut self, t: Option<f64>) {
        self.inner.set_temperature(t.map(|v| v as f32));
    }

    /// Save the configuration to disk (~/.edgecrab/config.yaml).
    #[napi]
    pub fn save(&self) -> Result<()> {
        self.inner.save().map_err(sdk_err)
    }
}

/// Get the EdgeCrab home directory path (usually ~/.edgecrab).
///
/// This is exported for the Node.js public API and is referenced from JS/TS,
/// which Rust's dead-code analysis for test targets cannot see.
#[allow(dead_code)]
#[napi]
pub fn edgecrab_home() -> String {
    edgecrab_sdk_core::edgecrab_home()
        .to_string_lossy()
        .into_owned()
}

/// Ensure the EdgeCrab home directory exists, creating it if needed.
///
/// This is exported for the Node.js public API and is referenced from JS/TS,
/// which Rust's dead-code analysis for test targets cannot see.
#[allow(dead_code)]
#[napi]
pub fn ensure_edgecrab_home() -> Result<String> {
    let path = edgecrab_sdk_core::ensure_edgecrab_home().map_err(sdk_err)?;
    Ok(path.to_string_lossy().into_owned())
}
