use lsp_types::{
    Diagnostic, Documentation, HoverContents, Location, LocationLink, MarkedString, NumberOrString,
    Position, Range, SymbolKind,
};
use serde_json::{Value, json};

use crate::error::uri_to_path;

pub fn position_json(position: &Position) -> Value {
    json!({
        "line": position.line + 1,
        "column": position.character + 1,
    })
}

pub fn range_json(range: &Range) -> Value {
    json!({
        "start": position_json(&range.start),
        "end": position_json(&range.end),
    })
}

pub fn format_location(location: &Location) -> Value {
    json!({
        "file": uri_to_path(&location.uri).ok().map(|path| path.display().to_string()).unwrap_or_else(|| location.uri.to_string()),
        "range": range_json(&location.range),
    })
}

pub fn format_link(link: &LocationLink) -> Value {
    json!({
        "file": uri_to_path(&link.target_uri).ok().map(|path| path.display().to_string()).unwrap_or_else(|| link.target_uri.to_string()),
        "range": range_json(&link.target_selection_range),
        "origin_range": link.origin_selection_range.as_ref().map(range_json),
    })
}

pub fn format_diagnostic(diagnostic: &Diagnostic) -> Value {
    json!({
        "severity": diagnostic.severity.map(|s| format!("{s:?}")),
        "code": diagnostic.code.as_ref().map(|code| match code {
            NumberOrString::Number(n) => n.to_string(),
            NumberOrString::String(s) => s.clone(),
        }),
        "message": diagnostic.message,
        "range": range_json(&diagnostic.range),
        "source": diagnostic.source,
        "related": diagnostic.related_information.as_ref().map(|items| {
            items.iter().map(|item| json!({
                "message": item.message,
                "location": format_location(&item.location),
            })).collect::<Vec<_>>()
        }).unwrap_or_default(),
    })
}

pub fn hover_to_string(contents: &HoverContents) -> String {
    match contents {
        HoverContents::Scalar(marked) => marked_string(marked),
        HoverContents::Array(items) => items
            .iter()
            .map(marked_string)
            .collect::<Vec<_>>()
            .join("\n\n"),
        HoverContents::Markup(markup) => markup.value.clone(),
    }
}

fn marked_string(value: &MarkedString) -> String {
    match value {
        MarkedString::String(s) => s.clone(),
        MarkedString::LanguageString(ls) => format!("```{}\n{}\n```", ls.language, ls.value),
    }
}

pub fn documentation_to_string(doc: &Documentation) -> String {
    match doc {
        Documentation::String(s) => s.clone(),
        Documentation::MarkupContent(markup) => markup.value.clone(),
    }
}

pub fn symbol_kind_name(kind: SymbolKind) -> String {
    format!("{kind:?}")
}
