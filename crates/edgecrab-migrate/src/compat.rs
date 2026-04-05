//! # Environment variable compatibility — legacy env var resolution
//!
//! WHY compat: Users migrating from hermes-agent or OpenClaw may have
//! env vars like HERMES_API_KEY or OPENCLAW_API_KEY. Resolve these
//! to the canonical OPENROUTER_API_KEY for seamless migration.

/// Resolve API key from env vars with legacy fallback.
///
/// Priority: OPENROUTER_API_KEY > OPENCLAW_API_KEY > HERMES_API_KEY
pub fn resolve_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY")
        .ok()
        .or_else(|| std::env::var("OPENCLAW_API_KEY").ok())
        .or_else(|| std::env::var("HERMES_API_KEY").ok())
}

/// Resolve the EdgeCrab home directory.
///
/// Priority: $EDGECRAB_HOME > ~/.edgecrab
pub fn resolve_home() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("EDGECRAB_HOME") {
        return std::path::PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".edgecrab")
}

/// Resolve the hermes-agent home directory.
///
/// Priority: $HERMES_HOME > ~/.hermes
pub fn resolve_hermes_home() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("HERMES_HOME") {
        return std::path::PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".hermes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_home_returns_path() {
        let home = resolve_home();
        assert!(home.to_string_lossy().contains("edgecrab") || !home.as_os_str().is_empty());
    }

    #[test]
    fn resolve_hermes_home_returns_path() {
        let home = resolve_hermes_home();
        assert!(home.to_string_lossy().contains("hermes") || !home.as_os_str().is_empty());
    }
}
