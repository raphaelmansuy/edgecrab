//! Accumulate [`edgequake_llm::StreamChunk`] into a final [`edgequake_llm::LLMResponse`].
//!
//! Mirrors `edgecrab-core` `conversation.rs` streaming aggregation (DRY within proxy;
//! future: extract to shared crate).

use std::collections::BTreeMap;

use edgequake_llm::traits::{StreamChunk, StreamUsage};
use edgequake_llm::{LLMResponse, ToolCall};

#[derive(Default)]
struct PartialToolCall {
    id: Option<String>,
    function_name: Option<String>,
    arguments: String,
    thought_signature: Option<String>,
}

#[derive(Default)]
pub struct StreamAccumulator {
    content: String,
    thinking: String,
    tool_calls: BTreeMap<usize, PartialToolCall>,
    finish_reason: Option<String>,
    usage: Option<StreamUsage>,
}

impl StreamAccumulator {
    pub fn push(&mut self, chunk: StreamChunk) -> edgequake_llm::Result<()> {
        match chunk {
            StreamChunk::Content(delta) => self.content.push_str(&delta),
            StreamChunk::ThinkingContent { text, .. } => self.thinking.push_str(&text),
            StreamChunk::ToolCallDelta {
                index,
                id,
                function_name,
                function_arguments,
                thought_signature,
            } => {
                let entry = self.tool_calls.entry(index).or_default();
                if let Some(id) = id {
                    entry.id = Some(id);
                }
                if let Some(name) = function_name {
                    entry.function_name = Some(name);
                }
                if let Some(args) = function_arguments {
                    entry.arguments.push_str(&args);
                }
                if thought_signature.is_some() {
                    entry.thought_signature = thought_signature;
                }
            }
            StreamChunk::PrefillProgress { .. } => {}
            StreamChunk::Finished { reason, usage, .. } => {
                self.finish_reason = Some(reason);
                if usage.is_some() {
                    self.usage = usage;
                }
            }
        }
        Ok(())
    }

    pub fn into_response(self, model: &str) -> edgequake_llm::Result<LLMResponse> {
        let tool_calls = finalize_streamed_tool_calls(self.tool_calls)?;
        let prompt_tokens = self.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
        let completion_tokens = self
            .usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        let mut resp = LLMResponse::new(self.content, model);
        resp.prompt_tokens = prompt_tokens;
        resp.completion_tokens = completion_tokens;
        resp.total_tokens = prompt_tokens + completion_tokens;
        resp.finish_reason = self.finish_reason;
        resp.tool_calls = tool_calls;
        if !self.thinking.is_empty() {
            resp.thinking_content = Some(self.thinking);
        }
        Ok(resp)
    }
}

fn finalize_streamed_tool_calls(
    partials: BTreeMap<usize, PartialToolCall>,
) -> edgequake_llm::Result<Vec<ToolCall>> {
    partials
        .into_iter()
        .map(|(index, partial)| {
            let id = partial.id.unwrap_or_else(|| format!("stream_call_{index}"));
            let function_name = partial.function_name.ok_or_else(|| {
                edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} finished without a function name"
                ))
            })?;
            let arguments = partial.arguments.trim();
            if arguments.is_empty() {
                return Err(edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) finished without arguments"
                )));
            }
            let parsed: serde_json::Value = serde_json::from_str(arguments).map_err(|err| {
                edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) invalid JSON arguments: {err}"
                ))
            })?;
            if !parsed.is_object() {
                return Err(edgequake_llm::LlmError::ApiError(format!(
                    "streamed tool call {id} ({function_name}) arguments must be a JSON object"
                )));
            }
            Ok(ToolCall {
                id,
                call_type: "function".to_string(),
                function: edgequake_llm::FunctionCall {
                    name: function_name,
                    arguments: arguments.to_string(),
                },
                thought_signature: partial.thought_signature,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_tool_deltas() {
        let mut acc = StreamAccumulator::default();
        acc.push(StreamChunk::ToolCallDelta {
            index: 0,
            id: Some("call_1".into()),
            function_name: Some("foo".into()),
            function_arguments: Some("{\"a\":".into()),
            thought_signature: None,
        })
        .expect("push");
        acc.push(StreamChunk::ToolCallDelta {
            index: 0,
            id: None,
            function_name: None,
            function_arguments: Some("1}".into()),
            thought_signature: None,
        })
        .expect("push");
        acc.push(StreamChunk::Finished {
            reason: "tool_calls".into(),
            ttft_ms: None,
            usage: None,
        })
        .expect("push");
        let resp = acc.into_response("m").expect("resp");
        let calls = &resp.tool_calls;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.arguments, "{\"a\":1}");
    }
}
