//! Agent response dispatch — extracted from `app.rs` (Hermes `turnController` consumer).

use super::*;

impl App {
    /// Check for agent responses from background tasks.
    pub fn check_responses(&mut self) {
        while let Ok(resp) = self.response_rx.try_recv() {
            match resp {
                AgentResponse::Token(text) => {
                    // Accumulate per-turn token count regardless of streaming mode.
                    self.turn_stream_tokens += 1;
                    // Record TTFB (Time To First Token) on the very first token that
                    // arrives out of the AwaitingFirstToken phase. This is the wall-clock
                    // latency from "submit sent" to "first model token received" — a
                    // useful calibration metric for model-selection decisions.
                    if matches!(self.display_state, DisplayState::AwaitingFirstToken { .. })
                        && self.last_ttfb_secs.is_none()
                        && let DisplayState::AwaitingFirstToken { ref started, .. } =
                            self.display_state
                    {
                        self.last_ttfb_secs = Some(started.elapsed().as_secs_f32());
                    }
                    // Transition to streaming state on first token of a new phase.
                    if self.streaming_enabled
                        && matches!(
                            self.display_state,
                            DisplayState::AwaitingFirstToken { .. }
                                | DisplayState::Thinking { .. }
                                | DisplayState::ToolExec { .. }
                        )
                    {
                        // WHY turn_stream_tokens: initialise from the running total so
                        // the status bar shows cumulative tokens even after tool-call
                        // interruptions, rather than resetting to 0 each streaming phase.
                        self.display_state = DisplayState::Streaming {
                            token_count: self.turn_stream_tokens,
                            chars_written: 0,
                            current_section: None,
                            started: Instant::now(),
                        };
                    }
                    // Keep the Streaming state's token_count, chars_written, and current_section
                    // in sync as new tokens arrive.
                    let new_chars = text.len() as u64;
                    if let DisplayState::Streaming {
                        ref mut token_count,
                        ref mut chars_written,
                        ref mut current_section,
                        ..
                    } = self.display_state
                    {
                        *token_count = self.turn_stream_tokens;
                        *chars_written += new_chars;
                        // Detect new markdown headings in the accumulated streaming text.
                        // We check for `\n# ` and `\n## ` patterns in the current token
                        // plus a small look-behind (the text ends with `\n#...`).
                        stream_bridge::extract_streaming_section(&text, current_section);
                    }

                    if self.live_token_display_enabled {
                        if let Some(idx) = self.streaming_line {
                            if idx < self.output.len() {
                                self.output[idx].text.push_str(&text);
                                self.output[idx].invalidate_render_cache();
                            }
                        } else {
                            self.output
                                .push(OutputLine::new_text(text.clone(), OutputRole::Assistant));
                            self.streaming_line = Some(self.output.len() - 1);
                            // Only auto-scroll to bottom if the user is already there
                            if self.at_bottom {
                                self.scroll_offset = 0;
                            }
                        }
                        self.needs_redraw = true;
                    } else {
                        self.buffered_assistant_output.push_str(&text);
                    }
                    // Accumulate response text for voice mode TTS readback.
                    self.last_agent_response_text.push_str(&text);
                    self.turn_activity.on_model_resuming();
                    if matches!(self.display_state, DisplayState::Streaming { .. })
                        || !self.turn_activity.tools.is_empty()
                        || matches!(
                            self.turn_activity.phase,
                            ShelfPhase::AwaitingFirstToken
                        )
                    {
                        self.turn_activity.set_phase(ShelfPhase::Streaming);
                    }
                    self.note_shelf_activity();
                }
                AgentResponse::Footer(text) => {
                    if !text.trim().is_empty() {
                        self.last_agent_response_text.push_str("\n\n");
                        self.last_agent_response_text.push_str(&text);
                        self.push_output(text, OutputRole::System);
                        self.needs_redraw = true;
                    }
                }
                AgentResponse::Notice(text) => {
                    // Detect steering-applied notices to update steer state.
                    if text.starts_with("⛵ Steering applied") {
                        self.steer_applied_at = Some(Instant::now());
                        self.pending_steer_count = 0;
                    }
                    self.push_output(text, OutputRole::System);
                    self.needs_redraw = true;
                }
                AgentResponse::ActivityFeed(text) => {
                    if text.contains("Tool draft aborted")
                        || text.contains("non-streaming")
                        || text.contains("Stream interrupted")
                        || text.contains("stalled drafting")
                    {
                        stream_bridge::clear_tool_generating(&mut self.turn_activity);
                        self.display_state = DisplayState::AwaitingFirstToken {
                            frame: 0,
                            started: Instant::now(),
                        };
                    }
                    stream_bridge::apply_activity_notice(
                        &mut self.turn_activity,
                        text,
                        crate::turn_activity::ActivityTone::Info,
                    );
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::LlmWaitProgress {
                    provider,
                    elapsed_secs,
                    has_tools,
                    prompt_tokens_estimated,
                    context_length,
                    prefill_pct,
                } => {
                    stream_bridge::apply_llm_wait_progress(
                        &mut self.turn_activity,
                        &provider,
                        elapsed_secs,
                        has_tools,
                        edgecrab_tools::tool_progress_tail::LlmWaitContext {
                            prompt_tokens_estimated,
                            context_length,
                            prefill_pct,
                        },
                    );
                    if !matches!(
                        self.display_state,
                        DisplayState::Streaming { .. } | DisplayState::Thinking { .. }
                    ) {
                        match &mut self.display_state {
                            DisplayState::AwaitingFirstToken { started: _, .. } => {
                                // Keep wall-clock anchor — heartbeats must not reset elapsed.
                            }
                            _ => {
                                self.display_state = DisplayState::AwaitingFirstToken {
                                    frame: 0,
                                    started: Instant::now(),
                                };
                            }
                        }
                    }
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::SteerPending { count } => {
                    self.pending_steer_count = count;
                    self.needs_redraw = true;
                }
                AgentResponse::BackgroundProcessTail {
                    process_id,
                    command_preview,
                    tail,
                } => {
                    self.turn_activity.on_bg_tail(
                        process_id.clone(),
                        command_preview.clone(),
                        tail.clone(),
                    );
                    self.upsert_bg_process_line(&process_id, &command_preview, &tail);
                    self.note_shelf_activity();
                }
                AgentResponse::BackgroundProcessFinished {
                    process_id,
                    exit_code,
                } => {
                    self.turn_activity.on_bg_finished(&process_id);
                    self.finish_bg_process_line(&process_id, exit_code);
                    self.note_shelf_activity();
                }
                AgentResponse::DirectToolOutput(text) => {
                    self.push_output(text, OutputRole::System);
                    self.needs_redraw = true;
                }
                AgentResponse::Reasoning(text) => {
                    if matches!(self.display_state, DisplayState::AwaitingFirstToken { .. }) {
                        self.display_state = DisplayState::Thinking {
                            frame: 0,
                            started: Instant::now(),
                        };
                    }
                    if self.show_reasoning && !text.trim().is_empty() {
                        if let Some(idx) = self.reasoning_line {
                            if idx < self.output.len() {
                                self.output[idx].text.push_str(&text);
                                self.output[idx].invalidate_render_cache();
                            }
                        } else {
                            let line = OutputLine::new_text(
                                format!("Thinking\n{text}"),
                                OutputRole::Reasoning,
                            );
                            if let Some(idx) = self.streaming_line {
                                let insert_idx = idx.min(self.output.len());
                                self.output.insert(insert_idx, line);
                                self.reasoning_line = Some(insert_idx);
                                self.streaming_line = Some(insert_idx + 1);
                            } else {
                                self.output.push(line);
                                self.reasoning_line = Some(self.output.len() - 1);
                            }
                            if self.at_bottom {
                                self.scroll_offset = 0;
                            }
                        }
                        self.needs_redraw = true;
                    }
                    stream_bridge::apply_reasoning_delta(&mut self.turn_activity, &text);
                    self.note_shelf_activity();
                }
                AgentResponse::ToolGenerating {
                    tool_call_id,
                    name,
                    partial_args,
                } => {
                    let preview = extract_streaming_tool_preview(&name, &partial_args);
                    stream_bridge::apply_tool_generating(
                        &mut self.turn_activity,
                        tool_call_id.clone(),
                        name.clone(),
                        partial_args.clone(),
                    );
                    self.display_state = DisplayState::ToolExec {
                        tool_call_id,
                        name,
                        args_json: partial_args,
                        detail: if preview.is_empty() {
                            None
                        } else {
                            Some(preview)
                        },
                        frame: 0,
                        started: Instant::now(),
                    };
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::ToolExec {
                    tool_call_id,
                    name,
                    args_json,
                } => {
                    if name == "shadow_judge" {
                        self.handle_shadow_judge_intervention_notice(&args_json);
                        self.needs_redraw = true;
                        continue;
                    }
                    self.flush_buffered_assistant_output();
                    // CRITICAL: Break the streaming buffer at the tool boundary.
                    // Without this, tokens arriving after the tool call append to
                    // the pre-tool text, visually merging text before and after
                    // the tool call into a single garbled line.
                    self.streaming_line = None;
                    // Track parallel in-flight tools — multiple ToolExec events
                    // may arrive before any ToolDone (parallel tool dispatch).
                    self.in_flight_tool_count = self.in_flight_tool_count.saturating_add(1);
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    let started_at = Instant::now();
                    let preview = extract_tool_preview(&name, &args_json);
                    stream_bridge::apply_tool_exec(
                        &mut self.turn_activity,
                        tool_call_id.clone(),
                        name.clone(),
                        args_json.clone(),
                        preview,
                        self.progress_seq,
                    );
                    self.display_state = DisplayState::ToolExec {
                        tool_call_id: tool_call_id.clone(),
                        name: name.clone(),
                        args_json: args_json.clone(),
                        detail: None,
                        frame: 0,
                        started: started_at,
                    };
                    if self.should_render_in_flight_tool_in_transcript(&name, &args_json) {
                        // Push a live "in-flight" placeholder line to the output area.
                        let edit_snapshot = capture_local_edit_snapshot(&name, &args_json);
                        let running_spans = build_tool_running_line_width(
                            &name,
                            &args_json,
                            None,
                            &self.theme.tool_emojis,
                            &DisplayWidths::from_terminal_width(self.last_terminal_width as usize),
                        );
                        let line_idx = self.output.len();
                        self.output
                            .push(OutputLine::new_spans(running_spans, OutputRole::Tool));
                        self.pending_tool_lines.insert(
                            tool_call_id,
                            PendingToolLine {
                                tool_name: name,
                                args_json,
                                line_idx,
                                edit_snapshot,
                                minimal_indicator: false,
                            },
                        );
                    } else if self.tool_progress_mode == ToolProgressMode::Off
                        && !self.turn_activity.enabled
                    {
                        let preview = extract_tool_preview(&name, &args_json);
                        let line_idx = self.output.len();
                        self.push_output(format!("⏳ {name}  {preview}"), OutputRole::System);
                        self.pending_tool_lines.insert(
                            tool_call_id,
                            PendingToolLine {
                                tool_name: name,
                                args_json,
                                line_idx,
                                edit_snapshot: None,
                                minimal_indicator: true,
                            },
                        );
                    } else {
                        self.hidden_tool_calls.insert(tool_call_id);
                    }
                    self.maybe_shelf_onboarding();
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::ToolProgress {
                    tool_call_id,
                    name,
                    message,
                } => {
                    let detail = message.trim().to_string();
                    if detail.is_empty() {
                        continue;
                    }
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    stream_bridge::apply_tool_progress(
                        &mut self.turn_activity,
                        &tool_call_id,
                        detail.clone(),
                        self.progress_seq,
                        Instant::now(),
                    );
                    self.maybe_shelf_onboarding();
                    if let Some(row) = self.turn_activity.tool_row(&tool_call_id) {
                        if matches!(self.display_state, DisplayState::ToolExec { .. }) {
                            self.display_state = DisplayState::ToolExec {
                                tool_call_id: tool_call_id.clone(),
                                name: row.name.clone(),
                                args_json: row.args_json.clone(),
                                detail: Some(detail.clone()),
                                frame: 0,
                                started: row.started_at,
                            };
                        }
                    } else if let DisplayState::ToolExec {
                        tool_call_id: active_tool_call_id,
                        detail: active_detail,
                        ..
                    } = &mut self.display_state
                        && active_tool_call_id == &tool_call_id
                    {
                        *active_detail = Some(detail.clone());
                    }
                    if self.turn_activity.enabled {
                        if self.at_bottom {
                            self.scroll_offset = 0;
                        }
                        self.note_shelf_activity();
                        self.needs_redraw = true;
                        continue;
                    }
                    if self.hidden_tool_calls.contains(&tool_call_id) {
                        if self.at_bottom {
                            self.scroll_offset = 0;
                        }
                        self.needs_redraw = true;
                        continue;
                    }
                    if let Some(PendingToolLine {
                        minimal_indicator: true,
                        line_idx,
                        tool_name,
                        args_json,
                        ..
                    }) = self
                        .pending_tool_lines
                        .get(&tool_call_id)
                        .cloned()
                        .or_else(|| self.ensure_tool_progress_placeholder(&tool_call_id))
                        .filter(|pending| pending.minimal_indicator)
                    {
                        if line_idx < self.output.len() {
                            let preview = extract_tool_preview(&tool_name, &args_json);
                            let elapsed = self.turn_activity.tool_elapsed_secs(&tool_call_id);
                            self.output[line_idx].text =
                                edgecrab_tools::tool_progress_tail::format_minimal_tool_indicator(
                                    &tool_name, &preview, elapsed, &detail,
                                );
                            self.output[line_idx].invalidate_render_cache();
                        }
                        if self.at_bottom {
                            self.scroll_offset = 0;
                        }
                        self.needs_redraw = true;
                        continue;
                    }
                    if let Some(PendingToolLine {
                        line_idx,
                        tool_name,
                        args_json,
                        ..
                    }) = self
                        .pending_tool_lines
                        .get(&tool_call_id)
                        .cloned()
                        .or_else(|| self.ensure_tool_progress_placeholder(&tool_call_id))
                        .filter(|pending| !pending.minimal_indicator)
                    {
                        if line_idx < self.output.len() {
                            let elapsed = self.turn_activity.tool_elapsed_secs(&tool_call_id);
                            self.output[line_idx].prebuilt_spans =
                                Some(build_tool_running_line_width_elapsed(
                                    &tool_name,
                                    &args_json,
                                    Some(detail.as_str()),
                                    elapsed,
                                    &self.theme.tool_emojis,
                                    &DisplayWidths::from_terminal_width(
                                        self.last_terminal_width as usize,
                                    ),
                                ));
                            self.output[line_idx].invalidate_render_cache();
                        }
                    } else if self.turn_activity.contains_tool(&tool_call_id) {
                        // Active tool but transcript filtered — status bar already updated.
                    } else {
                        self.push_output(
                            format!("{}: {detail}", name.replace('_', " ")),
                            OutputRole::System,
                        );
                    }
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::ToolDone {
                    tool_call_id,
                    name,
                    args_json,
                    result_preview,
                    duration_ms,
                    is_error,
                } => {
                    let hidden = self.hidden_tool_calls.remove(&tool_call_id);
                    // Build the final styled completion spans.
                    let pending = self.pending_tool_lines.remove(&tool_call_id);
                    let minimal_done = pending
                        .as_ref()
                        .is_some_and(|entry| entry.minimal_indicator);
                    if minimal_done
                        && let Some(line_idx) = pending.as_ref().map(|entry| entry.line_idx)
                    {
                        self.remove_output_line(line_idx);
                    }
                    let show_done = self.should_show_tool_done_in_transcript(hidden, minimal_done);
                    if show_done {
                        let widths =
                            DisplayWidths::from_terminal_width(self.last_terminal_width as usize);
                        let spans = build_tool_done_line_width(
                            &name,
                            &args_json,
                            result_preview.as_deref(),
                            duration_ms,
                            is_error,
                            &self.theme.tool_emojis,
                            &widths,
                        );
                        // Upgrade the in-flight placeholder in-place (if present).
                        //
                        // WHY in-place: replacing the placeholder avoids appending a
                        // second line for the same tool call — the layout stays stable
                        // (no shift), and the cyan "···" naturally becomes the gold
                        // timing string without any visual flash.
                        if let Some(PendingToolLine { line_idx, .. }) = pending.as_ref() {
                            if *line_idx < self.output.len() {
                                self.output[*line_idx].prebuilt_spans = Some(spans);
                                if matches!(
                                    name.as_str(),
                                    "terminal" | "execute_code" | "browser_snapshot"
                                ) && let Some(body) = result_preview
                                    .clone()
                                    .filter(|text| !text.trim().is_empty())
                                {
                                    let body =
                                        crate::transcript_heights::truncate_verbose_trail(&body);
                                    self.output[*line_idx].attach_expandable_body(body);
                                }
                                self.output[*line_idx].invalidate_render_cache();
                            } else {
                                // Index out of range — fall back to append (shouldn't happen).
                                self.push_output_spans(spans, OutputRole::Tool);
                            }
                        } else {
                            // No pending placeholder (e.g. streaming disabled, or the
                            // tool fired before the feature was introduced) — append.
                            self.push_output_spans(spans, OutputRole::Tool);
                        }
                        if self.tool_progress_mode == ToolProgressMode::Verbose
                            && !self.turn_activity.enabled
                        {
                            for line in build_tool_verbose_lines_width(
                                &name,
                                &args_json,
                                result_preview.as_deref(),
                                is_error,
                                widths.verbose_content,
                            ) {
                                self.push_output_spans(line, OutputRole::Tool);
                            }
                        }
                        if let Some(diff_lines) = render_edit_diff_lines(
                            &name,
                            &args_json,
                            is_error,
                            pending
                                .as_ref()
                                .and_then(|entry| entry.edit_snapshot.as_ref()),
                        ) {
                            for line in diff_lines {
                                self.push_output_spans(line, OutputRole::Tool);
                            }
                        }
                        if name == "report_task_status"
                            && !is_error
                            && let Some(preview) = result_preview
                                .as_deref()
                                .filter(|text| !text.trim().is_empty())
                        {
                            self.push_output(
                                format_task_status_progress_notice(preview),
                                OutputRole::System,
                            );
                        }
                        if name == "run_process"
                            && !is_error
                            && let Some(process_id) =
                                Self::parse_run_process_id(result_preview.as_deref())
                            && !self.bg_process_lines.contains_key(&process_id)
                        {
                            let command_preview = extract_tool_preview(&name, &args_json);
                            self.upsert_bg_process_line(&process_id, &command_preview, "starting…");
                        }
                    }
                    if name == "clarify" && self.clarify_pending_tx.is_some() {
                        self.flush_abandoned_clarify("timed out");
                    }
                    // Decrement the in-flight counter. Only transition back to
                    // Thinking when ALL parallel tools have completed; otherwise
                    // stay in ToolExec state so the status bar stays accurate.
                    self.in_flight_tool_count = self.in_flight_tool_count.saturating_sub(1);
                    stream_bridge::apply_tool_done(&mut self.turn_activity, &tool_call_id);
                    if is_error {
                        stream_bridge::apply_activity_notice(
                            &mut self.turn_activity,
                            format!("{} failed", name.replace('_', " ")),
                            crate::turn_activity::ActivityTone::Error,
                        );
                    }
                    if self.in_flight_tool_count == 0 {
                        self.display_state = DisplayState::AwaitingFirstToken {
                            frame: 0,
                            started: Instant::now(),
                        };
                        self.turn_activity
                            .set_phase(crate::turn_activity::ShelfPhase::AwaitingFirstToken);
                    } else if let Some((active_tool_call_id, row)) =
                        self.turn_activity.latest_active_tool()
                    {
                        self.display_state = DisplayState::ToolExec {
                            tool_call_id: active_tool_call_id.to_string(),
                            name: row.name.clone(),
                            args_json: row.args_json.clone(),
                            detail: row.detail.clone(),
                            frame: 0,
                            started: row.started_at,
                        };
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentStart {
                    task_index,
                    task_count,
                    goal,
                    depth,
                    agent_id,
                    parent_id,
                } => {
                    self.flush_buffered_assistant_output();
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    self.active_subagents.insert(
                        task_index,
                        ActiveSubagentStatus {
                            task_index,
                            task_count,
                            goal: goal.clone(),
                            last_detail: None,
                            last_seq: self.progress_seq,
                        },
                    );
                    self.streaming_line = None;
                    // Push a running placeholder and record its index so that
                    // SubAgentReasoning / SubAgentToolExec can update it in-place,
                    // and SubAgentFinish can replace it — exactly like ToolExec/ToolDone.
                    let widths =
                        DisplayWidths::from_terminal_width(self.last_terminal_width as usize);
                    let running_spans = build_subagent_running_line_width(
                        task_index, task_count, &goal, None, 0, &widths,
                    );
                    let line_idx = self.output.len();
                    self.output
                        .push(OutputLine::new_spans(running_spans, OutputRole::Tool));
                    self.pending_subagent_lines.insert(
                        task_index,
                        PendingSubagentLine {
                            line_idx,
                            started_at: Instant::now(),
                            goal: goal.clone(),
                            task_count,
                        },
                    );
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    stream_bridge::apply_subagent_start(
                        &mut self.turn_activity,
                        task_index,
                        task_count,
                        goal.clone(),
                        depth,
                        agent_id,
                        parent_id,
                    );
                    self.maybe_agents_nudge();
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentReasoning {
                    task_index,
                    task_count: _task_count,
                    text,
                } => {
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    let detail = format!(
                        "thinking: {}",
                        edgecrab_core::safe_truncate(text.trim(), 72)
                    );
                    if let Some(status) = self.active_subagents.get_mut(&task_index) {
                        status.last_detail = Some(detail.clone());
                        status.last_seq = self.progress_seq;
                    }
                    // Update running placeholder in-place — no new lines pushed.
                    if let Some(pending) = self.pending_subagent_lines.get(&task_index) {
                        let line_idx = pending.line_idx;
                        let goal = pending.goal.clone();
                        let task_count = pending.task_count;
                        let elapsed = pending.started_at.elapsed().as_secs();
                        if line_idx < self.output.len() {
                            let widths = DisplayWidths::from_terminal_width(
                                self.last_terminal_width as usize,
                            );
                            self.output[line_idx].prebuilt_spans =
                                Some(build_subagent_running_line_width(
                                    task_index,
                                    task_count,
                                    &goal,
                                    Some(&detail),
                                    elapsed,
                                    &widths,
                                ));
                            self.output[line_idx].invalidate_render_cache();
                        }
                    }
                    stream_bridge::apply_subagent_detail(
                        &mut self.turn_activity,
                        task_index,
                        detail,
                    );
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentToolExec {
                    task_index,
                    task_count: _task_count,
                    name,
                    args_json,
                } => {
                    self.progress_seq = self.progress_seq.saturating_add(1);
                    let preview = crate::tool_display::extract_tool_preview(&name, &args_json);
                    let detail = if preview.is_empty() {
                        name.clone()
                    } else {
                        format!("{name}  {preview}")
                    };
                    if let Some(status) = self.active_subagents.get_mut(&task_index) {
                        status.last_detail = Some(detail.clone());
                        status.last_seq = self.progress_seq;
                    }
                    // Update the running placeholder in-place — do NOT push a new line.
                    // Showing every sub-agent tool call as a permanent line creates
                    // O(tool_calls × subagents) output noise.
                    if let Some(pending) = self.pending_subagent_lines.get(&task_index) {
                        let line_idx = pending.line_idx;
                        let goal = pending.goal.clone();
                        let task_count = pending.task_count;
                        let elapsed = pending.started_at.elapsed().as_secs();
                        if line_idx < self.output.len() {
                            let widths = DisplayWidths::from_terminal_width(
                                self.last_terminal_width as usize,
                            );
                            self.output[line_idx].prebuilt_spans =
                                Some(build_subagent_running_line_width(
                                    task_index,
                                    task_count,
                                    &goal,
                                    Some(&detail),
                                    elapsed,
                                    &widths,
                                ));
                            self.output[line_idx].invalidate_render_cache();
                        }
                    }
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    stream_bridge::apply_subagent_tool(
                        &mut self.turn_activity,
                        task_index,
                        &name,
                        detail,
                    );
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::SubAgentFinish {
                    task_index,
                    task_count,
                    status,
                    duration_ms,
                    summary,
                    api_calls,
                    model,
                } => {
                    self.active_subagents.remove(&task_index);
                    self.streaming_line = None;
                    let is_error = status != "completed";
                    let widths =
                        DisplayWidths::from_terminal_width(self.last_terminal_width as usize);
                    let done_spans = build_subagent_done_line_width(
                        task_index,
                        task_count,
                        is_error,
                        duration_ms,
                        api_calls,
                        model.as_deref().filter(|m| !m.is_empty()),
                        summary.trim(),
                        &widths,
                    );
                    // Replace the running placeholder in-place; fall back to append when
                    // the placeholder is missing (e.g. session restore, race condition).
                    if let Some(pending) = self.pending_subagent_lines.remove(&task_index) {
                        if pending.line_idx < self.output.len() {
                            self.output[pending.line_idx].prebuilt_spans = Some(done_spans);
                            self.output[pending.line_idx].invalidate_render_cache();
                        } else {
                            self.push_output_spans(done_spans, OutputRole::Tool);
                        }
                    } else {
                        self.push_output_spans(done_spans, OutputRole::Tool);
                    }
                    if self.at_bottom {
                        self.scroll_offset = 0;
                    }
                    if let Some(row) = self.turn_activity.subagents.get(&task_index) {
                        self.spawn_history
                            .record_finish(row, duration_ms / 1000, &status);
                    }
                    stream_bridge::apply_subagent_finish(&mut self.turn_activity, task_index);
                    self.note_shelf_activity();
                    self.needs_redraw = true;
                }
                AgentResponse::RunFinished { outcome } => {
                    self.flush_buffered_assistant_output();
                    let outcome = self.maybe_apply_stop_hooks(outcome);
                    self.last_run_outcome = Some(outcome.clone());
                    self.push_output(
                        format_run_outcome_notice(&outcome),
                        run_outcome_role(&outcome),
                    );
                    self.needs_redraw = true;
                }
                AgentResponse::Done => {
                    self.flush_buffered_assistant_output();
                    self.auto_update_status();
                    let turn_metrics = TurnCommitMetrics {
                        token_est: self
                            .turn_activity
                            .thinking_token_est
                            .saturating_add(self.turn_activity.tool_token_acc),
                        cost_usd: (self.session_cost - self.turn_baseline_cost).max(0.0),
                    };
                    self.spawn_history.commit_turn(turn_metrics);
                    if let Some(session_id) = self.current_session_key()
                        && let Some(snapshot) = self.spawn_history.turns().next().cloned()
                        && let Err(err) =
                            crate::spawn_tree_store::save_turn_snapshot(&session_id, &snapshot)
                    {
                        tracing::debug!(error = %err, "spawn_tree.save skipped");
                    }
                    let goal_last_response = self.last_agent_response_text.clone();
                    let goal_interrupted = self.last_run_outcome.as_ref().is_some_and(|o| {
                        matches!(o.state, edgecrab_types::CompletionDecision::Interrupted)
                    });
                    self.clear_active_request_state();
                    self.last_response_time = Some(Instant::now());
                    self.turn_count += 1;
                    self.needs_redraw = true;

                    // Show TTFB calibration hint when the wait was noticeable (>1s).
                    // This surfaces the model's latency characteristics to the user
                    // without requiring any external tooling.
                    if let Some(ttfb) = self.last_ttfb_secs.take()
                        && ttfb >= 1.0
                    {
                        self.push_output(
                            format!("  \u{21b3} ttfb: {ttfb:.1}s"),
                            OutputRole::System,
                        );
                    }

                    // Voice mode: speak the response via direct TTS after each
                    // turn. This avoids routing a deterministic action back
                    // through the model and removes a major source of flakiness.
                    let response_text = std::mem::take(&mut self.last_agent_response_text);
                    if self.voice_mode_enabled && !response_text.is_empty() {
                        self.spawn_direct_tts(response_text, false);
                    }
                    if self.voice_continuous_active && !self.voice_playback_active {
                        self.maybe_restart_continuous_voice_session(
                            "Response finished. Listening again for continuous voice...",
                        );
                    }

                    if edgecrab_core::prompt_queue_has_real_user_message(&self.prompt_queue) {
                        if let Some(next) = self.prompt_queue.first().cloned() {
                            self.prompt_queue.remove(0);
                            self.process_input(&next);
                        }
                    } else {
                        self.drain_queued_slash_commands();
                        self.maybe_continue_goal_loop(&goal_last_response, goal_interrupted);
                    }
                }
                AgentResponse::Error(err) => {
                    self.flush_buffered_assistant_output();
                    self.clear_active_request_state();
                    self.push_output(format!("⚠ Run failed\n{}", err.trim()), OutputRole::Error);
                    if self.voice_continuous_active {
                        self.stop_continuous_voice_session(false);
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::Clarify {
                    question,
                    choices,
                    response_tx,
                } => {
                    // Display the question prominently and wait for the user.
                    // The agent is paused — it will resume once the oneshot sender
                    // is fulfilled. We store the sender and route the user's next
                    // Enter key press to it instead of treating it as a new prompt.
                    self.display_state = DisplayState::WaitingForClarify;
                    self.turn_activity.set_phase(ShelfPhase::WaitingForClarify);
                    self.note_shelf_activity();
                    self.push_output(format!("❓ {question}"), OutputRole::System);
                    // Render predefined choices as a numbered list so the user can
                    // type a number or their own answer. A 5th "Other" option is
                    // implied; the user may also type free-form text.
                    if let Some(ref list) = choices {
                        for (i, choice) in list.iter().enumerate() {
                            self.push_output(
                                format!("  {}. {}", i + 1, choice),
                                OutputRole::System,
                            );
                        }
                        self.push_output(
                            format!("  {}. Other (type your answer)", list.len() + 1),
                            OutputRole::System,
                        );
                    }
                    self.clarify_pending_tx = Some(response_tx);
                    self.clarify_pending_question = Some(question.clone());
                    self.clarify_pending_choices = choices.clone();
                    self.textarea.set_block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(Color::Rgb(255, 220, 80)))
                            .title(" ❓ Reply: "),
                    );
                    self.needs_redraw = true;
                }
                AgentResponse::Approval {
                    command,
                    full_command,
                    response_tx,
                } => {
                    // Check the session-level approval cache first.
                    // SHA-256 key is the exact full_command string so permission is
                    // tight — "rm -rf /tmp/a" and "rm -rf /tmp/b" are distinct keys.
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    full_command.hash(&mut h);
                    let cache_key = format!("{:x}", h.finish());

                    if self.session_approvals.contains(&cache_key) {
                        // Already approved for this session — auto-accept.
                        let _ = response_tx.send(edgecrab_core::ApprovalChoice::Once);
                        self.needs_redraw = true;
                    } else {
                        // Surface the approval overlay.
                        self.display_state = DisplayState::WaitingForApproval {
                            command: command.clone(),
                            full_command,
                            selected: 0,
                            show_full: false,
                            scroll_offset: 0,
                        };
                        self.turn_activity.set_phase(ShelfPhase::WaitingForApproval);
                        self.note_shelf_activity();
                        self.approval_pending_tx = Some(response_tx);
                        self.needs_redraw = true;
                    }
                }
                AgentResponse::SecretRequest {
                    var_name,
                    prompt,
                    is_sudo,
                    response_tx,
                } => {
                    // Surface the masked-input overlay.
                    self.display_state = DisplayState::SecretCapture {
                        var_name,
                        prompt,
                        is_sudo,
                        buffer: String::new(),
                    };
                    self.secret_pending_tx = Some(response_tx);
                    self.needs_redraw = true;
                }
                AgentResponse::BgOp(result) => {
                    if matches!(self.display_state, DisplayState::BgOp { .. }) {
                        self.display_state = DisplayState::Idle;
                        self.needs_redraw = true;
                    }
                    match result {
                        BackgroundOpResult::ModelCatalogReady {
                            models,
                            current_model,
                        } => {
                            self.model_selector_refresh_in_flight = false;
                            self.apply_model_selector_catalog(
                                models,
                                &current_model,
                                true,
                                self.model_selector_target,
                            );
                        }
                        BackgroundOpResult::SystemMsg(text) => {
                            self.push_output(text, OutputRole::System);
                        }
                        BackgroundOpResult::GatewayCommandDone { report } => {
                            self.push_output(report, OutputRole::System);
                            self.refresh_gateway_browser();
                        }
                        BackgroundOpResult::DiagnoseReady { report } => {
                            // Count lines before storing for scroll-clamping.
                            let total = report.lines().count();
                            self.diagnose_panel.report = report;
                            self.diagnose_panel.total_lines = total;
                            self.diagnose_panel.scroll = 0;
                            self.diagnose_panel.active = true;
                            self.diagnose_panel.refresh_in_flight = false;
                            self.needs_redraw = true;
                        }
                        BackgroundOpResult::CompressDone { msg } => {
                            self.turn_activity.push_activity(
                                edgecrab_core::safe_truncate(msg.trim(), 72).to_string(),
                                crate::turn_activity::ActivityTone::Info,
                            );
                            self.note_shelf_activity();
                            self.push_output(msg, OutputRole::System);
                        }
                        BackgroundOpResult::ModelChangeDone { outcome } => {
                            self.model_name = outcome.to_model().to_string();
                            self.update_context_window();
                            let confirmation =
                                edgecrab_core::format_model_change_confirmation(&outcome);
                            match persist_model_to_config(outcome.to_model()) {
                                Ok(()) => self.push_output(
                                    format!(
                                        "{confirmation}\n\nSaved as default model for next run."
                                    ),
                                    OutputRole::System,
                                ),
                                Err(e) => self.push_output(
                                    format!(
                                        "{confirmation}\n\n(warning: failed to save default: {e})"
                                    ),
                                    OutputRole::System,
                                ),
                            }
                        }
                        BackgroundOpResult::SessionHandoffDone { message } => {
                            self.push_output(message, OutputRole::System);
                            self.should_exit = true;
                        }
                    }
                }
                AgentResponse::RemoteSkillSearchReady {
                    request_id,
                    query,
                    report,
                } => {
                    self.apply_remote_skill_search_result(request_id, query, report);
                }
                AgentResponse::RemotePluginSearchReady {
                    request_id,
                    query,
                    report,
                } => {
                    self.apply_remote_plugin_search_result(request_id, query, report);
                }
                AgentResponse::RemoteMcpSearchReady {
                    request_id,
                    query,
                    report,
                } => {
                    self.apply_remote_mcp_search_result(request_id, query, report);
                }
                AgentResponse::RemotePluginActionComplete {
                    message,
                    plugin_name,
                } => {
                    self.remote_plugin_browser.action_in_flight = None;
                    if let Err(error) = self.refresh_agent_plugin_runtime() {
                        self.push_output(
                            format!("Plugin runtime refresh failed: {error}"),
                            OutputRole::Error,
                        );
                    }
                    self.push_output(
                        format!("{message}\nInspect with: /plugins info {plugin_name}"),
                        OutputRole::System,
                    );
                    if self.remote_plugin_browser.selector.active {
                        self.schedule_remote_plugin_search(true);
                    }
                }
                AgentResponse::RemotePluginActionFailed {
                    action_label,
                    identifier,
                    error,
                } => {
                    self.remote_plugin_browser.action_in_flight = None;
                    self.push_output(
                        format!("Remote {action_label} failed for '{identifier}': {error}"),
                        OutputRole::Error,
                    );
                    self.needs_redraw = true;
                }
                AgentResponse::RemoteSkillGuardPrompt {
                    entry,
                    preview,
                    review_only,
                } => {
                    self.remote_skill_browser.action_in_flight = None;
                    self.skill_trust_prompt = Some(App::build_skill_trust_prompt_state(
                        entry,
                        *preview,
                        review_only,
                    ));
                    self.needs_redraw = true;
                }
                AgentResponse::RemoteSkillGuardPreviewReady {
                    identifier,
                    preview,
                } => {
                    self.apply_remote_skill_guard_preview_ready(identifier, *preview);
                }
                AgentResponse::RemoteSkillGuardPreviewFailed { identifier, error } => {
                    self.apply_remote_skill_guard_preview_failed(identifier, error);
                }
                AgentResponse::RemoteSkillActionComplete {
                    message,
                    skill_name,
                } => {
                    self.remote_skill_browser.action_in_flight = None;
                    self.refresh_skills_list();
                    self.push_output(
                        format!("{message}\nActivate with: /skills view {skill_name}"),
                        OutputRole::System,
                    );
                    if self.remote_skill_browser.selector.active {
                        self.schedule_remote_skill_search(true);
                    }
                }
                AgentResponse::RemoteSkillActionFailed {
                    action_label,
                    identifier,
                    error,
                } => {
                    self.remote_skill_browser.action_in_flight = None;
                    self.push_output(
                        format!("Remote {action_label} failed for '{identifier}': {error}"),
                        OutputRole::Error,
                    );
                    self.needs_redraw = true;
                }
                AgentResponse::VoiceTranscript {
                    transcript,
                    submit_to_agent,
                    meta,
                } => {
                    let transcript = normalize_voice_transcript(&transcript);
                    let filtered = meta.is_some_and(|meta| {
                        self.voice_hallucination_filter
                            && is_probable_voice_hallucination(
                                &transcript,
                                meta.capture_duration_secs,
                            )
                    });
                    if transcript.trim().is_empty() {
                        self.push_output(
                            "Transcription completed, but no speech was detected.",
                            OutputRole::System,
                        );
                        self.note_empty_voice_capture();
                    } else if filtered {
                        self.push_output(
                            format!(
                                "Filtered probable STT hallucination from a short capture:\n{}",
                                transcript
                            ),
                            OutputRole::System,
                        );
                        self.note_empty_voice_capture();
                    } else if submit_to_agent {
                        self.voice_no_speech_count = 0;
                        self.push_output(
                            format!("Voice reply transcript:\n{transcript}"),
                            OutputRole::System,
                        );
                        self.process_input(&transcript);
                    } else {
                        self.voice_no_speech_count = 0;
                        self.push_output(
                            format!("Voice transcript:\n{transcript}"),
                            OutputRole::System,
                        );
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::VoiceCaptureFailed {
                    error,
                    continuous_session,
                } => {
                    if continuous_session {
                        self.voice_continuous_active = false;
                        self.voice_no_speech_count = 0;
                        self.push_output(
                            format!("{error}\nContinuous voice stopped to avoid a restart loop."),
                            OutputRole::Error,
                        );
                    } else {
                        self.push_output(error, OutputRole::Error);
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::VoicePlaybackFinished => {
                    self.voice_playback_active = false;
                    if self.voice_continuous_active && !self.is_processing {
                        self.maybe_restart_continuous_voice_session(
                            "Spoken reply finished. Listening again for continuous voice...",
                        );
                    }
                    self.needs_redraw = true;
                }
                AgentResponse::BackgroundPromptComplete {
                    task_num,
                    task_id,
                    prompt_preview,
                    response,
                } => {
                    self.background_tasks_active.remove(&task_id);
                    let body = if response.trim().is_empty() {
                        "(No response generated)".to_string()
                    } else {
                        response
                    };
                    self.push_output(
                        format!(
                            "EdgeCrab (background #{task_num})\nTask ID: {task_id}\nPrompt: \"{prompt_preview}\"\n\n{body}"
                        ),
                        OutputRole::Assistant,
                    );
                }
                AgentResponse::BackgroundPromptProgress { task_id, text, .. } => {
                    if let Some(status) = self.background_tasks_active.get_mut(&task_id) {
                        self.progress_seq = self.progress_seq.saturating_add(1);
                        status.last_progress = Some(text.clone());
                        status.last_seq = self.progress_seq;
                        self.push_output(text, OutputRole::System);
                    }
                }
                AgentResponse::BackgroundPromptFailed {
                    task_num,
                    task_id,
                    error,
                } => {
                    self.background_tasks_active.remove(&task_id);
                    self.push_output(
                        format!(
                            "Background task #{task_num} failed\nTask ID: {task_id}\nError: {error}"
                        ),
                        OutputRole::Error,
                    );
                }
                AgentResponse::SideQuestionComplete { question, response } => {
                    let body = if response.trim().is_empty() {
                        "(No response generated)".to_string()
                    } else {
                        response
                    };
                    self.push_output(
                        format!(
                            "/btw {}\n\n{body}",
                            edgecrab_core::safe_truncate(question.trim(), 72)
                        ),
                        OutputRole::Assistant,
                    );
                }
                AgentResponse::SideQuestionFailed { question, error } => {
                    self.push_output(
                        format!(
                            "/btw {}\nError: {error}",
                            edgecrab_core::safe_truncate(question.trim(), 72)
                        ),
                        OutputRole::Error,
                    );
                }
            }
        }

        // Drain cron job completion notifications from the background scheduler.
        // These arrive as pre-formatted markdown strings and are shown as
        // assistant-style output so the user knows a job completed.
        while let Ok(msg) = self.cron_rx.try_recv() {
            self.push_output(msg, OutputRole::Assistant);
            self.needs_redraw = true;
        }
    }
}
