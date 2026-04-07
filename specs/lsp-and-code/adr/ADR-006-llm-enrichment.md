# ADR-006 — LLM-Enriched Diagnostics Architecture

**Status**: Proposed  
**Date**: 2025

---

## Context

LSP diagnostics are designed for developer IDEs: they contain precise error codes and short
messages like `E0507: cannot move out of '*self' which is behind a '&' reference`. These are
cryptic to a language model without the surrounding source context and the conceptual frame
for interpreting them.

EdgeCrab can enrich diagnostics by:
1. Extracting source context around the diagnostic location
2. Asking an auxiliary LLM to explain the error and suggest a fix
3. Returning the enriched result to the agent as a structured response

This is **unique to EdgeCrab** — no other agent (including Claude Code) does this.

---

## Design

### Input to enrichment

```
Diagnostic:
  severity: Error
  code: "E0507"
  message: "cannot move out of `*self` which is behind a `&` reference"
  range: { start: {line: 42, col: 8}, end: {line: 42, col: 20} }

Source context (±5 lines, with cursor marker):
  40 | impl Processor {
  41 |     pub fn process(&self) {
  42 |         let data = *self;   // ← ERROR
  43 |                    ^^^^^
  44 |     }
  45 | }

Language: Rust
```

### Prompt to auxiliary LLM

```
You are a Rust expert. Explain this compiler error and suggest the minimal fix.

ERROR [E0507] at line 42:
"cannot move out of `*self` which is behind a `&` reference"

Source context:
```rust
40 | impl Processor {
41 |     pub fn process(&self) {
42 |         let data = *self;   // ← here
43 |     }
44 | }
```

Respond in JSON: { "explanation": "...", "suggested_fix": "..." }
```

### Output structure

```json
{
  "file": "/project/src/main.rs",
  "diagnostics": [
    {
      "original": {
        "severity": "Error",
        "code": "E0507",
        "message": "cannot move out of `*self`...",
        "range": { "start": {"line": 42, "column": 8} }
      },
      "explanation": "You're trying to move the entire struct out of a shared reference. Rust prevents this because other code might also hold a reference to the same data.",
      "suggested_fix": "Change `process(&self)` to `process(self)` if you want ownership, or use `self.clone()` if Processor implements Clone, or restructure to avoid moving."
    }
  ]
}
```

---

## Implementation

```rust
// enrichment.rs

pub struct DiagnosticEnricher {
    aux_client: Arc<dyn AuxiliaryLlmClient>,
}

impl DiagnosticEnricher {
    pub async fn enrich(
        &self,
        file:        &str,
        diagnostics: &[Diagnostic],
        source_text: &str,
    ) -> Vec<EnrichedDiagnostic> {
        // Batch all diagnostics for the file in one LLM request
        let batch_prompt = self.build_batch_prompt(file, diagnostics, source_text);
        
        match self.aux_client.complete(&batch_prompt).await {
            Ok(json_response) => self.parse_enrichments(diagnostics, &json_response),
            Err(_) => {
                // Graceful degradation: return diagnostics without enrichment
                diagnostics.iter().map(|d| EnrichedDiagnostic {
                    original: d.clone(),
                    explanation: None,
                    suggested_fix: None,
                }).collect()
            }
        }
    }

    fn build_batch_prompt(
        &self,
        file:        &str,
        diagnostics: &[Diagnostic],
        source_text: &str,
    ) -> String {
        let lang = detect_language(file);
        let mut prompt = format!(
            "You are a {} expert. Explain these compiler errors and suggest fixes.\n\n",
            lang
        );

        for (i, diag) in diagnostics.iter().enumerate() {
            let context = extract_source_context(source_text, &diag.range, 5);
            prompt.push_str(&format!(
                "ERROR {}: [{}] {}\nContext:\n```{}\n{}\n```\n\n",
                i + 1,
                format_code(&diag.code),
                diag.message,
                lang.to_lowercase(),
                context,
            ));
        }

        prompt.push_str(
            "Respond as a JSON array: [{\"explanation\": \"...\", \"suggested_fix\": \"...\"}, ...]"
        );
        prompt
    }
}

/// Extract ±N lines around a range, with a caret marker on the error line.
fn extract_source_context(source: &str, range: &lsp_types::Range, window: usize) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let error_line = range.start.line as usize;
    let start = error_line.saturating_sub(window);
    let end   = (error_line + window + 1).min(lines.len());

    let mut out = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        let num = start + i;
        if num == error_line {
            out.push_str(&format!("{:4} | {} ← ERROR\n", num, line));
        } else {
            out.push_str(&format!("{:4} | {}\n", num, line));
        }
    }
    out
}
```

---

## Rate Limiting Considerations

Enrichment calls an LLM. In a workspace with 200 errors, this must not generate 200 LLM
calls. Strategy:

1. **Batch per file**: All errors in one file → one LLM request (already in design above)
2. **Cap per invocation**: `max_diagnostics_to_enrich: usize` (default: 10) — beyond which
   diagnostics are returned un-enriched with a note
3. **Caching**: Enrichments are cached per (file_path + diagnostic_code + message + line_number).
   The cache is session-scoped, keyed in `DiagnosticCache`.

---

## Decision

**Implement as an opt-in operation** (`lsp_enrich_diagnostics` tool) rather than automatically
enriching all push diagnostics in the background. Reasons:

1. Enrichment has non-zero LLM cost — agent should invoke it intentionally
2. Background enrichment would introduce nondeterministic LLM calls during file writes
3. Agent can decide when enrichment is worth the cost (e.g., when stuck on an error)

---

## Consequences

- `AuxiliaryLlmClient` trait must be accessible via `ToolContext` (already exists in edgecrab-core as `AuxiliaryClient`)
- Rate limiting is enforced in `DiagnosticEnricher::enrich()`, not in the tool
- Enrichment failures degrade gracefully (raw diagnostics returned)
