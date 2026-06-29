//! StreamEvent → AgentResponse bridge (Hermes gateway event handler parity).

use std::sync::Arc;

use edgecrab_core::Agent;
use tokio::sync::mpsc;

use super::AgentResponse;

fn format_context_pressure_notice(estimated_tokens: usize, threshold_tokens: usize) -> String {
    let ratio = if threshold_tokens == 0 {
        0.0
    } else {
        (estimated_tokens as f32 / threshold_tokens as f32).clamp(0.0, 1.0)
    };
    let percent = (ratio * 100.0).round() as usize;
    let width = 16usize;
    let filled = ((ratio * width as f32).round() as usize).min(width);
    let bar = format!("{}{}", "▰".repeat(filled), "▱".repeat(width - filled));
    format!(
        "⚠ Context {bar} {percent}% to compression ({estimated_tokens}/{threshold_tokens} tokens)"
    )
}

pub(super) struct RecoveredAssistantTurn {
    pub(super) reasoning: Option<String>,
    pub(super) text: String,
}

pub(super) fn recover_latest_assistant_turn(
    messages: &[edgecrab_types::Message],
) -> Option<RecoveredAssistantTurn> {
    messages.iter().rev().find_map(|message| {
        if message.role != edgecrab_types::Role::Assistant {
            return None;
        }

        let text = message.text_content();
        let reasoning = message
            .reasoning
            .clone()
            .filter(|text| !text.trim().is_empty());
        if text.trim().is_empty() && reasoning.is_none() {
            return None;
        }

        Some(RecoveredAssistantTurn { reasoning, text })
    })
}

async fn forward_stream_event_to_tui(
    event: edgecrab_core::agent::StreamEvent,
    tx: &mpsc::UnboundedSender<AgentResponse>,
    hook_registry: &edgecrab_gateway::hooks::HookRegistry,
    saw_token_event: &mut bool,
    saw_reasoning_event: &mut bool,
    saw_terminal_event: &mut bool,
) -> bool {
    use edgecrab_core::agent::StreamEvent;

    match event {
        StreamEvent::Token(text) => {
            if text.is_empty() {
                tracing::debug!("TUI→agent: dropping empty token delta");
            } else {
                *saw_token_event = true;
                tracing::info!(len = text.len(), "TUI→agent: forwarding token");
                let _ = tx.send(AgentResponse::Token(text));
            }
        }
        StreamEvent::Reasoning(text) => {
            *saw_reasoning_event = true;
            tracing::info!(len = text.len(), "TUI→agent: forwarding reasoning");
            let _ = tx.send(AgentResponse::Reasoning(text));
        }
        StreamEvent::ToolGenerating {
            tool_call_id,
            name,
            partial_args,
        } => {
            tracing::info!(tool = %name, "TUI→agent: forwarding tool generating");
            let _ = tx.send(AgentResponse::ToolGenerating {
                tool_call_id,
                name,
                partial_args,
            });
        }
        StreamEvent::ToolExec {
            tool_call_id,
            name,
            args_json,
        } => {
            tracing::info!(tool = %name, "TUI→agent: forwarding tool exec");
            let _ = tx.send(AgentResponse::ToolExec {
                tool_call_id,
                name,
                args_json,
            });
        }
        StreamEvent::ToolProgress {
            tool_call_id,
            name,
            message,
        } => {
            tracing::info!(tool = %name, "TUI→agent: forwarding tool progress");
            let _ = tx.send(AgentResponse::ToolProgress {
                tool_call_id,
                name,
                message,
            });
        }
        StreamEvent::ToolDone {
            tool_call_id,
            name,
            args_json,
            result_preview,
            duration_ms,
            is_error,
        } => {
            tracing::info!(tool = %name, is_error, "TUI→agent: forwarding tool done");
            let _ = tx.send(AgentResponse::ToolDone {
                tool_call_id,
                name,
                args_json,
                result_preview,
                duration_ms,
                is_error,
            });
        }
        StreamEvent::SubAgentStart {
            task_index,
            task_count,
            goal,
            depth,
            agent_id,
            parent_id,
        } => {
            tracing::info!(
                task_index,
                task_count,
                depth,
                agent_id = %agent_id,
                "TUI→agent: forwarding subagent start"
            );
            let _ = tx.send(AgentResponse::SubAgentStart {
                task_index,
                task_count,
                goal,
                depth,
                agent_id,
                parent_id,
            });
        }
        StreamEvent::SubAgentReasoning {
            task_index,
            task_count,
            text,
        } => {
            tracing::info!(
                task_index,
                task_count,
                len = text.len(),
                "TUI→agent: forwarding subagent reasoning"
            );
            let _ = tx.send(AgentResponse::SubAgentReasoning {
                task_index,
                task_count,
                text,
            });
        }
        StreamEvent::SubAgentToolExec {
            task_index,
            task_count,
            name,
            args_json,
        } => {
            tracing::info!(
                task_index,
                task_count,
                tool = %name,
                "TUI→agent: forwarding subagent tool exec"
            );
            let _ = tx.send(AgentResponse::SubAgentToolExec {
                task_index,
                task_count,
                name,
                args_json,
            });
        }
        StreamEvent::SubAgentFinish {
            task_index,
            task_count,
            status,
            duration_ms,
            summary,
            api_calls,
            model,
        } => {
            tracing::info!(
                task_index,
                task_count,
                status = %status,
                "TUI→agent: forwarding subagent finish"
            );
            let _ = tx.send(AgentResponse::SubAgentFinish {
                task_index,
                task_count,
                status,
                duration_ms,
                summary,
                api_calls,
                model,
            });
        }
        StreamEvent::RunFinished { outcome } => {
            tracing::info!(
                state = outcome.state.as_str(),
                exit_reason = outcome.exit_reason.as_str(),
                "TUI→agent: forwarding run outcome"
            );
            let _ = tx.send(AgentResponse::RunFinished { outcome });
        }
        StreamEvent::Footer(text) => {
            tracing::info!(len = text.len(), "TUI→agent: forwarding mutation footer");
            let _ = tx.send(AgentResponse::Footer(text));
        }
        StreamEvent::Done => {
            *saw_terminal_event = true;
            tracing::info!("TUI→agent: forwarding done");
            let _ = tx.send(AgentResponse::Done);
            return true;
        }
        StreamEvent::Error(error) => {
            *saw_terminal_event = true;
            tracing::warn!(%error, "TUI→agent: forwarding error");
            let _ = tx.send(AgentResponse::Error(error));
            return true;
        }
        StreamEvent::Clarify {
            question,
            choices,
            response_tx,
        } => {
            tracing::info!("TUI→agent: forwarding clarify request");
            let _ = tx.send(AgentResponse::Clarify {
                question,
                choices,
                response_tx,
            });
        }
        StreamEvent::Approval {
            command,
            full_command,
            reasons: _,
            response_tx,
        } => {
            tracing::info!("TUI→agent: forwarding approval request");
            let _ = tx.send(AgentResponse::Approval {
                command,
                full_command,
                response_tx,
            });
        }
        StreamEvent::SecretRequest {
            var_name,
            prompt,
            is_sudo,
            response_tx,
        } => {
            tracing::info!(var = %var_name, "TUI→agent: forwarding secret request");
            let _ = tx.send(AgentResponse::SecretRequest {
                var_name,
                prompt,
                is_sudo,
                response_tx,
            });
        }
        StreamEvent::HookEvent {
            event,
            context_json,
        } => {
            tracing::info!(hook = %event, "TUI→agent: handling hook event");
            if let Ok(ctx) =
                serde_json::from_str::<edgecrab_gateway::hooks::HookContext>(&context_json)
            {
                hook_registry.emit(&event, &ctx).await;
            } else {
                tracing::warn!(hook = %event, "TUI→agent: hook context parse failed");
            }
        }
        StreamEvent::ContextPressure {
            estimated_tokens,
            threshold_tokens,
        } => {
            tracing::info!(
                estimated_tokens,
                threshold_tokens,
                "TUI→agent: forwarding context pressure notice"
            );
            let _ = tx.send(AgentResponse::Notice(format_context_pressure_notice(
                estimated_tokens,
                threshold_tokens,
            )));
        }
        StreamEvent::ActivityNotice(text) => {
            tracing::info!(len = text.len(), "TUI→agent: forwarding activity notice");
            let _ = tx.send(AgentResponse::ActivityFeed(text));
        }
        StreamEvent::LlmWaitProgress {
            provider,
            elapsed_secs,
            has_tools,
            prompt_tokens_estimated,
            context_length,
            prefill_pct,
        } => {
            tracing::info!(
                provider = %provider,
                elapsed_secs,
                has_tools,
                "TUI→agent: forwarding LLM wait progress"
            );
            let _ = tx.send(AgentResponse::LlmWaitProgress {
                provider,
                elapsed_secs,
                has_tools,
                prompt_tokens_estimated,
                context_length,
                prefill_pct,
            });
        }
        StreamEvent::BackgroundProcessTail {
            process_id,
            command_preview,
            tail,
        } => {
            let _ = tx.send(AgentResponse::BackgroundProcessTail {
                process_id,
                command_preview,
                tail,
            });
        }
        StreamEvent::BackgroundProcessFinished {
            process_id,
            exit_code,
        } => {
            let _ = tx.send(AgentResponse::BackgroundProcessFinished {
                process_id,
                exit_code,
            });
        }

        // Steering events — update the TUI's pending steer counter and status bar.
        // These events originate from the agent loop (conversation.rs) and are
        // informational; no AgentResponse forwarding is needed.
        StreamEvent::SteerPending { count } => {
            tracing::debug!(count, "TUI←agent: steer pending notification");
            let _ = tx.send(AgentResponse::SteerPending { count });
        }

        StreamEvent::SteerApplied { message } => {
            tracing::info!(
                len = message.len(),
                "TUI←agent: steer applied — agent received new guidance"
            );
            let _ = tx.send(AgentResponse::Notice(format!(
                "⛵ Steering applied ({})",
                message.chars().take(72).collect::<String>()
            )));
        }

        StreamEvent::ModelTransferComplete { .. } => {
            // Confirmation is surfaced via BackgroundOpResult::ModelChangeDone in the TUI.
        }
    }

    false
}

pub(super) async fn forward_agent_stream_to_tui(
    agent: Arc<Agent>,
    mut chunk_rx: mpsc::UnboundedReceiver<edgecrab_core::agent::StreamEvent>,
    mut agent_task: tokio::task::JoinHandle<Result<(), edgecrab_types::AgentError>>,
    tx: mpsc::UnboundedSender<AgentResponse>,
    hook_registry: Arc<edgecrab_gateway::hooks::HookRegistry>,
) {
    let mut saw_terminal_event = false;
    let mut saw_token_event = false;
    let mut saw_reasoning_event = false;
    let mut agent_result: Option<
        Result<Result<(), edgecrab_types::AgentError>, tokio::task::JoinError>,
    > = None;

    loop {
        if let Some(join_result) = agent_result.take() {
            while let Ok(event) = chunk_rx.try_recv() {
                if forward_stream_event_to_tui(
                    event,
                    &tx,
                    &hook_registry,
                    &mut saw_token_event,
                    &mut saw_reasoning_event,
                    &mut saw_terminal_event,
                )
                .await
                {
                    return;
                }
            }

            tracing::info!(
                saw_terminal_event,
                saw_token_event,
                saw_reasoning_event,
                ?join_result,
                "TUI→agent: stream pump observed inner task completion"
            );

            if saw_terminal_event {
                return;
            }

            match join_result {
                Ok(Ok(())) => {
                    let messages = agent.messages().await;
                    if let Some(turn) = recover_latest_assistant_turn(&messages) {
                        tracing::warn!(
                            recovered_text_len = turn.text.len(),
                            recovered_reasoning = turn.reasoning.is_some(),
                            "TUI→agent: recovered assistant reply after missing terminal stream event"
                        );
                        if !saw_reasoning_event && let Some(reasoning) = turn.reasoning {
                            let _ = tx.send(AgentResponse::Reasoning(reasoning));
                        }
                        if !saw_token_event && !turn.text.is_empty() {
                            let _ = tx.send(AgentResponse::Token(turn.text));
                        }
                        let _ = tx.send(AgentResponse::Done);
                    } else {
                        let _ = tx.send(AgentResponse::Error(
                            "Agent completed, but no assistant reply could be recovered for the TUI."
                                .to_string(),
                        ));
                    }
                }
                Ok(Err(err)) => {
                    let _ = tx.send(AgentResponse::Error(err.to_string()));
                }
                Err(err) => {
                    let _ = tx.send(AgentResponse::Error(format!("Agent task failed: {err}")));
                }
            }
            return;
        }

        tokio::select! {
            join_result = &mut agent_task => {
                agent_result = Some(join_result);
            }
            event = chunk_rx.recv() => {
                match event {
                    Some(event) => {
                        if forward_stream_event_to_tui(
                            event,
                            &tx,
                            &hook_registry,
                            &mut saw_token_event,
                            &mut saw_reasoning_event,
                            &mut saw_terminal_event,
                        )
                        .await
                        {
                            return;
                        }
                    }
                    None => {
                        tracing::warn!(
                            saw_terminal_event,
                            saw_token_event,
                            saw_reasoning_event,
                            "TUI→agent: stream channel closed before terminal event"
                        );
                        if saw_terminal_event {
                            return;
                        }
                        agent_result = Some((&mut agent_task).await);
                    }
                }
            }
        }
    }
}
