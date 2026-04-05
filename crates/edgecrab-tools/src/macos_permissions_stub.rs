//! Non-macOS stub for macOS permission preflight checks.
//!
//! WHY this exists: callers use a uniform API, but the real macOS consent
//! implementation should only be compiled on macOS targets.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacosConsentState {
    Granted,
    Denied,
    WouldPrompt,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MacosPermissionPreflight {
    pub automation_target: Option<String>,
    pub automation_state: Option<MacosConsentState>,
    pub accessibility_state: Option<MacosConsentState>,
}

pub fn preflight_command_permissions(_command: &str) -> MacosPermissionPreflight {
    MacosPermissionPreflight::default()
}
