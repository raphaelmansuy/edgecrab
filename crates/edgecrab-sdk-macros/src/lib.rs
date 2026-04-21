//! # edgecrab-sdk-macros
//!
//! Proc macros for the EdgeCrab SDK.
//!
//! Provides `#[edgecrab_tool]` — an attribute macro that transforms an async
//! function into a full `ToolHandler` implementation.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use edgecrab_sdk::prelude::*;
//!
//! /// Greet someone by name.
//! #[edgecrab_tool(name = "greet", toolset = "demo", emoji = "👋")]
//! async fn greet(name: String) -> Result<String, ToolError> {
//!     Ok(format!("Hello, {name}!"))
//! }
//! ```
//!
//! This generates:
//! - A unit struct (e.g., `GreetTool`)
//! - An `impl ToolHandler for GreetTool` with schema derived from the function
//!   signature
//! - An `inventory::submit!()` call for compile-time registration

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    FnArg, ItemFn, LitStr, Meta, Pat, ReturnType, Type, parse_macro_input, punctuated::Punctuated,
    token::Comma,
};

/// Attribute macro that generates a `ToolHandler` implementation from an async
/// function.
///
/// ## Attributes
///
/// | Key       | Required | Default              | Description                          |
/// |-----------|----------|----------------------|--------------------------------------|
/// | `name`    | No       | function name        | Tool name visible to the LLM         |
/// | `toolset` | No       | `"custom"`           | Toolset for enable/disable filtering |
/// | `emoji`   | No       | `"⚡"`               | Display emoji in TUI                 |
///
/// ## Function Constraints
///
/// - Must be `async fn`
/// - Parameters must be deserializable from JSON (`String`, `i64`, `f64`,
///   `bool`, `Option<T>`, `Vec<T>`)
/// - Return type must be `Result<String, ToolError>` (or just `String`)
/// - The function doc comment becomes the tool schema description
#[proc_macro_attribute]
pub fn edgecrab_tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);
    let attrs = parse_tool_attrs(attr);

    match generate_tool_handler(&input_fn, &attrs) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct ToolAttrs {
    name: Option<String>,
    toolset: Option<String>,
    emoji: Option<String>,
}

fn parse_tool_attrs(attr: TokenStream) -> ToolAttrs {
    let mut result = ToolAttrs {
        name: None,
        toolset: None,
        emoji: None,
    };

    // Parse as comma-separated name=value pairs
    let parser = syn::punctuated::Punctuated::<Meta, Comma>::parse_terminated;
    if let Ok(metas) = syn::parse::Parser::parse(parser, attr) {
        for meta in metas {
            if let Meta::NameValue(nv) = meta {
                let key = nv
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    match key.as_str() {
                        "name" => result.name = Some(s.value()),
                        "toolset" => result.toolset = Some(s.value()),
                        "emoji" => result.emoji = Some(s.value()),
                        _ => {} // Ignore unknown attrs
                    }
                }
            }
        }
    }

    result
}

fn generate_tool_handler(
    input_fn: &ItemFn,
    attrs: &ToolAttrs,
) -> Result<proc_macro2::TokenStream, syn::Error> {
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;

    // Validate: must be async
    if input_fn.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            input_fn.sig.fn_token,
            "#[edgecrab_tool] functions must be async",
        ));
    }

    // Derive tool name from attr or function name
    let tool_name = attrs.name.clone().unwrap_or_else(|| fn_name.to_string());
    let toolset = attrs
        .toolset
        .clone()
        .unwrap_or_else(|| "custom".to_string());
    let emoji = attrs.emoji.clone().unwrap_or_else(|| "⚡".to_string());

    // Generate struct name: snake_case fn → PascalCase + "Tool"
    let struct_name = format_ident!("{}Tool", to_pascal_case(&fn_name.to_string()));

    // Extract doc comment for schema description
    let description =
        extract_doc_comment(&input_fn.attrs).unwrap_or_else(|| format!("Tool: {tool_name}"));

    // Parse function parameters (skip &self if present, skip ctx: &ToolContext)
    let params = extract_params(&input_fn.sig.inputs)?;

    // Build JSON schema properties
    let schema_properties = build_schema_properties(&params);
    let required_fields = build_required_fields(&params);

    // Build argument extraction code
    let arg_extractions = build_arg_extractions(&params);
    let arg_names: Vec<_> = params.iter().map(|p| &p.name).collect();

    // Determine if the function takes a context parameter
    let has_ctx = has_context_param(&input_fn.sig.inputs);

    let fn_call = if has_ctx {
        quote! { #fn_name(#(#arg_names,)* _ctx) }
    } else {
        quote! { #fn_name(#(#arg_names),*) }
    };

    // Check return type — if it returns Result<String, _> we use ?, otherwise wrap
    let returns_result = is_result_return(&input_fn.sig.output);

    let execute_body = if returns_result {
        quote! {
            #(#arg_extractions)*
            #fn_call.await
        }
    } else {
        quote! {
            #(#arg_extractions)*
            Ok(#fn_call.await)
        }
    };

    let tool_name_lit = LitStr::new(&tool_name, Span::call_site());
    let toolset_lit = LitStr::new(&toolset, Span::call_site());
    let emoji_lit = LitStr::new(&emoji, Span::call_site());
    let desc_lit = LitStr::new(&description, Span::call_site());

    let output = quote! {
        // Keep the original function
        #input_fn

        /// Auto-generated tool handler for [`#fn_name`].
        #fn_vis struct #struct_name;

        #[async_trait::async_trait]
        impl edgecrab_sdk_core::ToolHandler for #struct_name {
            fn name(&self) -> &'static str { #tool_name_lit }
            fn toolset(&self) -> &'static str { #toolset_lit }
            fn emoji(&self) -> &'static str { #emoji_lit }

            fn schema(&self) -> edgecrab_sdk_core::ToolSchema {
                edgecrab_sdk_core::ToolSchema {
                    name: #tool_name_lit.into(),
                    description: #desc_lit.into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": { #schema_properties },
                        "required": [#(#required_fields),*]
                    }),
                    strict: None,
                }
            }

            async fn execute(
                &self,
                _args: serde_json::Value,
                _ctx: &edgecrab_sdk_core::ToolContext,
            ) -> Result<String, edgecrab_sdk_core::ToolError> {
                #execute_body
            }
        }

        inventory::submit!(&(#struct_name) as &dyn edgecrab_sdk_core::ToolHandler);
    };

    Ok(output)
}

// ── Parameter parsing helpers ────────────────────────────────────────

struct ToolParam {
    name: proc_macro2::Ident,
    ty: Type,
    is_optional: bool,
    json_type: String,
}

fn extract_params(inputs: &Punctuated<FnArg, Comma>) -> Result<Vec<ToolParam>, syn::Error> {
    let mut params = Vec::new();

    for arg in inputs {
        match arg {
            FnArg::Receiver(_) => continue,
            FnArg::Typed(pat_ty) => {
                // Skip context parameters
                if is_tool_context_type(&pat_ty.ty) {
                    continue;
                }

                let name = match pat_ty.pat.as_ref() {
                    Pat::Ident(pi) => pi.ident.clone(),
                    _ => {
                        return Err(syn::Error::new_spanned(
                            &pat_ty.pat,
                            "expected simple identifier pattern",
                        ));
                    }
                };

                let is_optional = is_option_type(&pat_ty.ty);
                let json_type = rust_type_to_json_type(&pat_ty.ty);

                params.push(ToolParam {
                    name,
                    ty: (*pat_ty.ty).clone(),
                    is_optional,
                    json_type,
                });
            }
        }
    }

    Ok(params)
}

fn is_tool_context_type(ty: &Type) -> bool {
    let type_str = quote!(#ty).to_string();
    type_str.contains("ToolContext")
}

fn has_context_param(inputs: &Punctuated<FnArg, Comma>) -> bool {
    inputs.iter().any(|arg| {
        if let FnArg::Typed(pat_ty) = arg {
            is_tool_context_type(&pat_ty.ty)
        } else {
            false
        }
    })
}

fn is_option_type(ty: &Type) -> bool {
    let type_str = quote!(#ty).to_string();
    type_str.starts_with("Option <") || type_str.starts_with("Option<")
}

fn is_result_return(ret: &ReturnType) -> bool {
    match ret {
        ReturnType::Default => false,
        ReturnType::Type(_, ty) => {
            let type_str = quote!(#ty).to_string();
            type_str.starts_with("Result <") || type_str.starts_with("Result<")
        }
    }
}

fn rust_type_to_json_type(ty: &Type) -> String {
    let type_str = quote!(#ty).to_string().replace(' ', "");
    match type_str.as_str() {
        "String" | "&str" => "string".to_string(),
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize" => {
            "integer".to_string()
        }
        "f32" | "f64" => "number".to_string(),
        "bool" => "boolean".to_string(),
        s if s.starts_with("Vec<") => "array".to_string(),
        s if s.starts_with("Option<") => {
            // Extract inner type
            let inner = &s[7..s.len() - 1];
            match inner {
                "String" | "&str" => "string".to_string(),
                "i64" | "i32" | "u64" | "u32" | "usize" => "integer".to_string(),
                "f64" | "f32" => "number".to_string(),
                "bool" => "boolean".to_string(),
                _ => "string".to_string(),
            }
        }
        _ => "string".to_string(),
    }
}

fn build_schema_properties(params: &[ToolParam]) -> proc_macro2::TokenStream {
    let entries: Vec<proc_macro2::TokenStream> = params
        .iter()
        .map(|p| {
            let name_str = p.name.to_string();
            let json_type = &p.json_type;
            quote! {
                #name_str: { "type": #json_type }
            }
        })
        .collect();

    if entries.is_empty() {
        quote! {}
    } else {
        quote! { #(#entries),* }
    }
}

fn build_required_fields(params: &[ToolParam]) -> Vec<proc_macro2::TokenStream> {
    params
        .iter()
        .filter(|p| !p.is_optional)
        .map(|p| {
            let name_str = p.name.to_string();
            quote! { #name_str }
        })
        .collect()
}

fn build_arg_extractions(params: &[ToolParam]) -> Vec<proc_macro2::TokenStream> {
    params
        .iter()
        .map(|p| {
            let name = &p.name;
            let name_str = p.name.to_string();
            let ty = &p.ty;

            if p.is_optional {
                quote! {
                    let #name: #ty = _args.get(#name_str)
                        .and_then(|v| serde_json::from_value(v.clone()).ok());
                }
            } else {
                quote! {
                    let #name: #ty = serde_json::from_value(
                        _args.get(#name_str)
                            .cloned()
                            .unwrap_or(serde_json::Value::Null)
                    ).map_err(|e| edgecrab_sdk_core::ToolError::InvalidArgs {
                        tool: #name_str.into(),
                        message: format!("parameter '{}': {}", #name_str, e),
                    })?;
                }
            }
        })
        .collect()
}

fn extract_doc_comment(attrs: &[syn::Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc")
            && let Meta::NameValue(nv) = &attr.meta
            && let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) = &nv.value
        {
            lines.push(s.value().trim().to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" "))
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => c.to_uppercase().to_string() + chars.as_str(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pascal_case() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("greet"), "Greet");
        assert_eq!(to_pascal_case("get_git_diff"), "GetGitDiff");
    }

    #[test]
    fn test_rust_type_to_json() {
        let string_ty: Type = syn::parse_str("String").unwrap();
        assert_eq!(rust_type_to_json_type(&string_ty), "string");

        let int_ty: Type = syn::parse_str("i64").unwrap();
        assert_eq!(rust_type_to_json_type(&int_ty), "integer");

        let bool_ty: Type = syn::parse_str("bool").unwrap();
        assert_eq!(rust_type_to_json_type(&bool_ty), "boolean");

        let float_ty: Type = syn::parse_str("f64").unwrap();
        assert_eq!(rust_type_to_json_type(&float_ty), "number");
    }
}
