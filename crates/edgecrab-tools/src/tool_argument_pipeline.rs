//! Tool-argument pipeline — single DRY entry for wire repair + schema prepare.
//!
//! First principle: every tool argument (stream assembly, dispatch, suppression keys)
//! passes through the same deterministic stages:
//!   1. Syntax repair (`repair_tool_arguments`)
//!   2. JSON object parse
//!   3. Schema normalize (`ToolRegistry::prepare_tool_arguments`)

use std::sync::LazyLock;

use edgecrab_types::ToolError;
use regex::Regex;

use crate::registry::ToolRegistry;

static TRAILING_COMMA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r",\s*([}\]])").expect("valid trailing-comma regex"));

/// Repair malformed tool-call argument JSON (Hermes `_repair_tool_call_arguments` parity).
///
/// Deterministic passes only — no failure-count logic. When parsing still fails and
/// `EDGECRAB_TOOL_ARGS_EMPTY_FALLBACK=1`, returns `"{}"` (Hermes last-resort survival).
pub fn repair_tool_arguments(raw: &str) -> String {
    repair_tool_arguments_inner(raw, empty_fallback_enabled())
}

fn repair_tool_arguments_inner(raw: &str, allow_empty_fallback: bool) -> String {
    let raw_stripped = raw.trim();
    if raw_stripped.is_empty() || raw_stripped == "null" || raw_stripped == "None" {
        tracing::warn!("tool_argument_pipeline: empty tool args normalized to {{}}");
        return "{}".to_string();
    }

    // Pass 0: already valid JSON — canonicalize (compact wire form).
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw_stripped) {
        return canonical_tool_args_json(&value);
    }

    let mut fixed = normalize_python_json_literals(raw_stripped);
    fixed = TRAILING_COMMA.replace_all(&fixed, "$1").into_owned();
    fixed = close_unclosed_delimiters(&fixed);
    fixed = trim_excess_closing_delimiters(fixed);

    if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
        tracing::warn!(
            original = %truncate_for_log(raw_stripped),
            repaired = %truncate_for_log(&fixed),
            "tool_argument_pipeline: repaired malformed tool arguments"
        );
        return fixed;
    }

    let escaped = escape_invalid_chars_in_json_strings(&fixed);
    if escaped != fixed && serde_json::from_str::<serde_json::Value>(&escaped).is_ok() {
        tracing::warn!(
            original = %truncate_for_log(raw_stripped),
            repaired = %truncate_for_log(&escaped),
            "tool_argument_pipeline: repaired control-char-laced tool arguments"
        );
        return escaped;
    }

    if allow_empty_fallback {
        tracing::warn!(
            tool_args = %truncate_for_log(raw_stripped),
            "tool_argument_pipeline: unrepairable args replaced with {{}} (EDGECRAB_TOOL_ARGS_EMPTY_FALLBACK)"
        );
        return "{}".to_string();
    }

    fixed
}

/// Parse tool arguments into a JSON object, applying repair when strict parse fails.
pub fn parse_tool_arguments_json(raw: &str) -> Result<serde_json::Value, ToolError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(serde_json::json!({}));
    }

    match serde_json::from_str(trimmed) {
        Ok(value) => ensure_object(value),
        Err(first_err) => {
            let repaired = repair_tool_arguments(trimmed);
            serde_json::from_str(&repaired)
                .map_err(|e| ToolError::InvalidArgs {
                    tool: "tool_call".into(),
                    message: format!("invalid JSON arguments after repair: {e} (initial: {first_err})"),
                })
                .and_then(ensure_object)
        }
    }
}

/// Canonical JSON string for fingerprints and suppression keys.
pub fn canonical_tool_args_json(args: &serde_json::Value) -> String {
    serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
}

/// Full pipeline: repair → parse → schema alias/coerce via registry.
pub fn prepare_parsed_tool_arguments(
    registry: &ToolRegistry,
    tool_name: &str,
    raw: &str,
) -> Result<serde_json::Value, ToolError> {
    let mut args = parse_tool_arguments_json(raw).map_err(|e| match e {
        ToolError::InvalidArgs { tool, message } if tool == "tool_call" => ToolError::InvalidArgs {
            tool: tool_name.to_string(),
            message,
        },
        other => other,
    })?;
    registry.prepare_tool_arguments(tool_name, &mut args);
    validate_required_tool_fields(tool_name, &args)?;
    Ok(args)
}

fn validate_required_tool_fields(
    tool_name: &str,
    args: &serde_json::Value,
) -> Result<(), ToolError> {
    if tool_name != "write_file" {
        return Ok(());
    }
    let Some(obj) = args.as_object() else {
        return Ok(());
    };
    let path_ok = obj
        .get("path")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|s| !s.is_empty());
    if !path_ok {
        return Err(crate::recovery_catalog::write_file_missing_path());
    }
    if !obj.contains_key("content") {
        return Err(crate::recovery_catalog::write_file_missing_content(None));
    }
    Ok(())
}

/// Repair streamed tool-call argument JSON before assembly validation.
pub fn repair_stream_tool_arguments(raw: &str) -> String {
    repair_tool_arguments(raw)
}

fn ensure_object(value: serde_json::Value) -> Result<serde_json::Value, ToolError> {
    if value.is_object() {
        Ok(value)
    } else {
        Err(ToolError::InvalidArgs {
            tool: "tool_call".into(),
            message: format!(
                "tool arguments must be a JSON object, got {}",
                value_type_name(&value)
            ),
        })
    }
}

fn value_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn normalize_python_json_literals(raw: &str) -> String {
    raw.replace(": True", ": true")
        .replace(":True", ":true")
        .replace(": False", ": false")
        .replace(":False", ":false")
        .replace(": None", ": null")
        .replace(":None", ":null")
}

fn close_unclosed_delimiters(fixed: &str) -> String {
    let mut out = fixed.to_string();
    let open_curly = out.chars().filter(|c| *c == '{').count();
    let close_curly = out.chars().filter(|c| *c == '}').count();
    if open_curly > close_curly {
        out.push_str(&"}".repeat(open_curly - close_curly));
    }
    let open_sq = out.chars().filter(|c| *c == '[').count();
    let close_sq = out.chars().filter(|c| *c == ']').count();
    if open_sq > close_sq {
        out.push_str(&"]".repeat(open_sq - close_sq));
    }
    out
}

fn trim_excess_closing_delimiters(mut fixed: String) -> String {
    for _ in 0..50 {
        if serde_json::from_str::<serde_json::Value>(&fixed).is_ok() {
            break;
        }
        if fixed.ends_with('}')
            && fixed.chars().filter(|c| *c == '}').count()
                > fixed.chars().filter(|c| *c == '{').count()
        {
            fixed.pop();
            continue;
        }
        if fixed.ends_with(']')
            && fixed.chars().filter(|c| *c == ']').count()
                > fixed.chars().filter(|c| *c == '[').count()
        {
            fixed.pop();
            continue;
        }
        break;
    }
    fixed
}

/// Escape literal control characters inside JSON string values (Hermes #12093 / #12068).
fn escape_invalid_chars_in_json_strings(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 16);
    let mut in_string = false;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_string {
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push('\\');
                    out.push(next);
                }
                continue;
            }
            if ch == '"' {
                in_string = false;
                out.push(ch);
                continue;
            }
            if ch.is_ascii_control() {
                out.push_str(&format!("\\u{:04x}", ch as u32));
                continue;
            }
            out.push(ch);
        } else {
            if ch == '"' {
                in_string = true;
            }
            out.push(ch);
        }
    }
    out
}

fn empty_fallback_enabled() -> bool {
    match std::env::var("EDGECRAB_TOOL_ARGS_EMPTY_FALLBACK")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("0") | Some("false") | Some("FALSE") | Some("off") | Some("OFF") => false,
        Some("1") | Some("true") | Some("TRUE") | Some("on") | Some("ON") => true,
        // Default ON — Hermes survival parity. Opt out with EDGECRAB_TOOL_ARGS_EMPTY_FALLBACK=0.
        None | Some(_) => true,
    }
}

fn truncate_for_log(s: &str) -> String {
    crate::safe_truncate(s, 80).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ta01_empty_and_none_normalize_to_empty_object() {
        assert_eq!(repair_tool_arguments(""), "{}");
        assert_eq!(repair_tool_arguments("   "), "{}");
        assert_eq!(repair_tool_arguments("null"), "{}");
        assert_eq!(repair_tool_arguments("None"), "{}");
    }

    #[test]
    fn ta02_valid_json_canonicalizes() {
        let raw = r#"{"path": "foo.rs", "line": 42}"#;
        let repaired = repair_tool_arguments(raw);
        let v: serde_json::Value = serde_json::from_str(&repaired).expect("parse");
        assert_eq!(v["path"], "foo.rs");
        assert_eq!(v["line"], 42);
    }

    #[test]
    fn ta03_trailing_comma_object_and_array() {
        let obj = repair_tool_arguments(r#"{"a": 1, "b": 2, }"#);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&obj).expect("obj"),
            serde_json::json!({"a": 1, "b": 2})
        );
        let arr = repair_tool_arguments(r#"{"items": [1, 2, 3, ]}"#);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&arr).expect("arr")["items"]
                .as_array()
                .expect("array")
                .len(),
            3
        );
    }

    #[test]
    fn ta04_python_literals_and_unclosed_braces() {
        let py = repair_tool_arguments(r#"{"flag": True, "other": False, "val": None}"#);
        let v: serde_json::Value = serde_json::from_str(&py).expect("py");
        assert_eq!(v["flag"], serde_json::json!(true));
        assert!(v["val"].is_null());

        let nested = repair_tool_arguments(r#"{"a": {"b": 1}"#);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&nested).expect("nested")["a"]["b"],
            1
        );
    }

    #[test]
    fn ta05_trim_excess_closing_brace() {
        let fixed = repair_tool_arguments(r#"{"key": "value"}}"#);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fixed).expect("fixed"),
            serde_json::json!({"key": "value"})
        );
    }

    #[test]
    fn ta06_control_chars_with_trailing_comma() {
        let raw = r#"{"msg": "line
one",}"#;
        let fixed = repair_tool_arguments(raw);
        let v: serde_json::Value = serde_json::from_str(&fixed).expect("fixed");
        assert!(v["msg"].as_str().expect("str").contains("line"));
    }

    #[test]
    fn ta07_unrepairable_without_opt_out_still_attempts_structure() {
        let garbage = repair_tool_arguments_inner("totally not json", false);
        assert!(serde_json::from_str::<serde_json::Value>(&garbage).is_err());
    }

    #[test]
    fn ta08_empty_fallback_default_on() {
        let fixed = repair_tool_arguments("totally not json");
        assert_eq!(fixed, "{}");
    }

    #[test]
    fn ta08b_empty_fallback_opt_out() {
        let fixed = repair_tool_arguments_inner("totally not json", false);
        assert!(serde_json::from_str::<serde_json::Value>(&fixed).is_err());
    }

    #[test]
    fn ta09_prepare_renames_file_path_via_registry() {
        let registry = ToolRegistry::new();
        let prepared = prepare_parsed_tool_arguments(
            &registry,
            "write_file",
            r#"{"file_path":"x.py","text":"ok"}"#,
        )
        .expect("prepare");
        assert_eq!(prepared["path"], "x.py");
        assert_eq!(prepared["content"], "ok");
    }

    #[test]
    fn ta10_write_file_missing_path_after_alias_normalize() {
        let registry = ToolRegistry::new();
        let err = prepare_parsed_tool_arguments(&registry, "write_file", r#"{"text":"ok"}"#)
            .expect_err("missing path");
        assert!(err.to_llm_response().contains("missing required field 'path'"));
    }

    #[test]
    fn ta11_write_file_missing_content() {
        let registry = ToolRegistry::new();
        let err =
            prepare_parsed_tool_arguments(&registry, "write_file", r#"{"path":"/tmp/x.md"}"#)
                .expect_err("missing content");
        assert!(err.to_llm_response().contains("missing required field 'content'"));
    }
}
