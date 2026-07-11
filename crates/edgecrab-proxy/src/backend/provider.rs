//! Mode B — OpenAI wire ↔ [`LLMProvider`].

use std::sync::Arc;

use crate::error::ProxyError;
use crate::resolve::ResolvedBackend;
use crate::wire::messages::openai_messages_to_chat;
use crate::wire::openai::{
    ChatChoiceOut, ChatCompletionRequest, ChatCompletionResponse, ChatMessageOut, FunctionCallOut,
    ToolCallOut, UsageOut, unix_now,
};
use crate::wire::sse::stream_chunks_to_sse;
use axum::Json;
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use edgequake_llm::error::LlmError;
use edgequake_llm::traits::StreamChunk;
use edgequake_llm::{CompletionOptions, LLMProvider};

pub async fn handle_chat_completion(
    provider: Arc<dyn LLMProvider>,
    backend: &ResolvedBackend,
    req: ChatCompletionRequest,
) -> Result<Response, ProxyError> {
    let (messages, tools, tool_choice) = openai_messages_to_chat(&req)?;
    let options = CompletionOptions {
        temperature: req.temperature,
        max_tokens: req.max_tokens,
        ..Default::default()
    };
    let options_ref = Some(&options);

    if req.stream {
        return handle_stream(provider, backend, messages, tools, tool_choice, options_ref).await;
    }

    let response = if tools.is_empty() {
        provider.chat(&messages, options_ref).await?
    } else {
        provider
            .chat_with_tools(&messages, &tools, tool_choice, options_ref)
            .await?
    };

    Ok(Json(completion_json(backend, &response)).into_response())
}

async fn handle_stream(
    provider: Arc<dyn LLMProvider>,
    backend: &ResolvedBackend,
    messages: Vec<edgequake_llm::ChatMessage>,
    tools: Vec<edgequake_llm::ToolDefinition>,
    tool_choice: Option<edgequake_llm::ToolChoice>,
    options: Option<&CompletionOptions>,
) -> Result<Response, ProxyError> {
    let stream_result = if tools.is_empty() {
        provider
            .chat_with_tools_stream(&messages, &[], None, options)
            .await
    } else {
        provider
            .chat_with_tools_stream(&messages, &tools, tool_choice.clone(), options)
            .await
    };

    let stream = match stream_result {
        Ok(stream) => stream,
        Err(LlmError::NotSupported(_)) => {
            let response = if tools.is_empty() {
                provider.chat(&messages, options).await?
            } else {
                provider
                    .chat_with_tools(&messages, &tools, tool_choice, options)
                    .await?
            };
            return Ok(
                synthesize_sse_from_response(backend.display_model.clone(), response)
                    .into_response(),
            );
        }
        Err(err) => return Err(err.into()),
    };

    let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let model_label = backend.display_model.clone();
    let sse = Sse::new(stream_chunks_to_sse(chat_id, model_label, stream))
        .keep_alive(KeepAlive::default());
    Ok(sse.into_response())
}

/// Fallback when the provider has no native SSE: build [`StreamChunk`]s and reuse the encoder.
fn synthesize_sse_from_response(
    display_model: String,
    response: edgequake_llm::LLMResponse,
) -> Sse<
    impl futures::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send,
> {
    let finish = response.finish_reason.clone().unwrap_or_else(|| {
        if response.tool_calls.is_empty() {
            "stop".into()
        } else {
            "tool_calls".into()
        }
    });
    let mut chunks = Vec::new();
    if !response.content.is_empty() {
        chunks.push(Ok(StreamChunk::Content(response.content.clone())));
    }
    for (index, tc) in response.tool_calls.iter().enumerate() {
        chunks.push(Ok(StreamChunk::ToolCallDelta {
            index,
            id: Some(tc.id.clone()),
            function_name: Some(tc.function.name.clone()),
            function_arguments: Some(tc.function.arguments.clone()),
            thought_signature: tc.thought_signature.clone(),
        }));
    }
    chunks.push(Ok(StreamChunk::Finished {
        reason: finish,
        ttft_ms: None,
        usage: None,
    }));
    let chat_id = format!("chatcmpl-{}", uuid::Uuid::new_v4());
    let inner = futures::stream::iter(chunks);
    Sse::new(stream_chunks_to_sse(chat_id, display_model, inner)).keep_alive(KeepAlive::default())
}

fn completion_json(
    backend: &ResolvedBackend,
    response: &edgequake_llm::LLMResponse,
) -> ChatCompletionResponse {
    let tool_calls = if response.tool_calls.is_empty() {
        None
    } else {
        Some(
            response
                .tool_calls
                .iter()
                .map(|tc| ToolCallOut {
                    id: tc.id.clone(),
                    call_type: "function",
                    function: FunctionCallOut {
                        name: tc.function.name.clone(),
                        arguments: tc.function.arguments.clone(),
                    },
                })
                .collect(),
        )
    };
    let finish = response.finish_reason.clone().unwrap_or_else(|| {
        if tool_calls.is_some() {
            "tool_calls".into()
        } else {
            "stop".into()
        }
    });
    ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion",
        created: Some(unix_now()),
        model: backend.display_model.clone(),
        choices: vec![ChatChoiceOut {
            index: 0,
            message: ChatMessageOut {
                role: "assistant",
                content: response.content.clone(),
                tool_calls,
            },
            finish_reason: finish,
        }],
        usage: UsageOut {
            prompt_tokens: response.prompt_tokens as u32,
            completion_tokens: response.completion_tokens as u32,
            total_tokens: response.total_tokens as u32,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::ResolvedBackend;

    #[test]
    fn completion_json_maps_tool_calls() {
        let backend = ResolvedBackend {
            display_model: "mock/m".into(),
            runtime_provider: "mock".into(),
            model_name: "m".into(),
        };
        let mut resp = edgequake_llm::LLMResponse::new("", "m");
        resp.tool_calls = vec![edgequake_llm::ToolCall {
            id: "c1".into(),
            call_type: "function".into(),
            function: edgequake_llm::FunctionCall {
                name: "fn".into(),
                arguments: "{}".into(),
            },
            thought_signature: None,
        }];
        resp.finish_reason = Some("tool_calls".into());
        let json = completion_json(&backend, &resp);
        assert_eq!(json.choices[0].finish_reason, "tool_calls");
        assert!(json.choices[0].message.tool_calls.is_some());
    }
}
