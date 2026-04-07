use lsp_types::{
    CallHierarchyServerCapability, CodeActionProviderCapability, ImplementationProviderCapability,
    OneOf, ServerCapabilities,
};

#[macro_export]
macro_rules! require_capability {
    ($supported:expr, $op_name:expr) => {
        if !$supported {
            return Ok(serde_json::json!({
                "supported": false,
                "reason": format!("Server does not advertise {} capability", $op_name),
            })
            .to_string());
        }
    };
}

pub fn has_bool_or_registration<T>(value: &Option<OneOf<bool, T>>) -> bool {
    match value {
        Some(OneOf::Left(enabled)) => *enabled,
        Some(OneOf::Right(_)) => true,
        None => false,
    }
}

pub fn supports_implementation(value: &Option<ImplementationProviderCapability>) -> bool {
    match value {
        Some(ImplementationProviderCapability::Simple(enabled)) => *enabled,
        Some(ImplementationProviderCapability::Options(_)) => true,
        None => false,
    }
}

pub fn supports_call_hierarchy(value: &Option<CallHierarchyServerCapability>) -> bool {
    match value {
        Some(CallHierarchyServerCapability::Simple(enabled)) => *enabled,
        Some(CallHierarchyServerCapability::Options(_)) => true,
        None => false,
    }
}

pub fn supports_code_actions(value: &Option<CodeActionProviderCapability>) -> bool {
    match value {
        Some(CodeActionProviderCapability::Simple(enabled)) => *enabled,
        Some(CodeActionProviderCapability::Options(_)) => true,
        None => false,
    }
}

pub fn supports_code_action_resolve(caps: &ServerCapabilities) -> bool {
    match &caps.code_action_provider {
        Some(CodeActionProviderCapability::Simple(enabled)) => *enabled,
        Some(CodeActionProviderCapability::Options(options)) => {
            options.resolve_provider.unwrap_or(false)
        }
        None => false,
    }
}
