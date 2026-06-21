//! Accumulate [`edgequake_llm::StreamChunk`] into a final [`edgequake_llm::LLMResponse`].
//!
//! Delegates tool-call finalization to [`edgequake_llm::stream_tool_calls`] (DRY with edgecrab-core).

use edgequake_llm::LLMResponse;
use edgequake_llm::stream_tool_calls::{FinalizeStreamToolCallsOptions, StreamToolCallAccumulator};
use edgequake_llm::traits::{StreamChunk, StreamUsage};

pub struct StreamAccumulator {
    content: String,
    thinking: String,
    tool_calls: StreamToolCallAccumulator,
    finish_reason: Option<String>,
    usage: Option<StreamUsage>,
}

impl Default for StreamAccumulator {
    fn default() -> Self {
        Self {
            content: String::new(),
            thinking: String::new(),
            tool_calls: StreamToolCallAccumulator::new(),
            finish_reason: None,
            usage: None,
        }
    }
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
                self.tool_calls.apply_delta(
                    index,
                    id,
                    function_name,
                    function_arguments,
                    thought_signature,
                );
            }
            StreamChunk::PrefillProgress { .. } => {}
            StreamChunk::Connected { .. } => {}
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
        let tool_calls = self
            .tool_calls
            .finalize(FinalizeStreamToolCallsOptions::default())?;
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
