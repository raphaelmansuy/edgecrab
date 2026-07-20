//! Ratatui main loop — poll/draw cadence isolated from `app.rs` (Hermes `useMainApp` parity).

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use ratatui::prelude::CrosstermBackend;

use super::App;
use super::TerminalUiProfile;

/// Run the interactive TUI until `App::should_exit()`.
pub(super) fn run_event_loop(
    terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    fn heartbeat_interval_for(profile: TerminalUiProfile) -> Duration {
        match profile {
            TerminalUiProfile::Standard => Duration::from_millis(250),
            TerminalUiProfile::ReducedMotion => Duration::from_millis(800),
            TerminalUiProfile::BasicCompat => Duration::from_millis(1000),
        }
    }

    fn adaptive_draw_interval_after_draw(
        profile: TerminalUiProfile,
        floor: Duration,
        current: Duration,
        draw_cost: Duration,
    ) -> Duration {
        match profile {
            TerminalUiProfile::Standard => Duration::ZERO,
            TerminalUiProfile::ReducedMotion | TerminalUiProfile::BasicCompat => {
                let ceiling = match profile {
                    TerminalUiProfile::ReducedMotion => Duration::from_millis(120),
                    TerminalUiProfile::BasicCompat => Duration::from_millis(180),
                    TerminalUiProfile::Standard => Duration::ZERO,
                };
                let floor_ms = floor.as_millis() as u64;
                let ceiling_ms = ceiling.as_millis() as u64;
                let current_ms = current.as_millis() as u64;
                let draw_ms = draw_cost.as_millis() as u64;
                let scaled_ms = match profile {
                    TerminalUiProfile::ReducedMotion => draw_ms.saturating_mul(3) / 2,
                    TerminalUiProfile::BasicCompat => draw_ms.saturating_mul(2),
                    TerminalUiProfile::Standard => 0,
                };
                let target_ms = scaled_ms.clamp(floor_ms, ceiling_ms);
                if target_ms > current_ms {
                    Duration::from_millis(target_ms)
                } else {
                    Duration::from_millis(current_ms.saturating_sub(10)).max(floor)
                }
            }
        }
    }

    fn draw_app(
        terminal: &mut ratatui::Terminal<CrosstermBackend<io::Stdout>>,
        app: &mut App,
        effective_draw_interval: &mut Duration,
        min_draw_interval: Duration,
        last_draw: &mut Instant,
        last_status_heartbeat: &mut Instant,
    ) -> io::Result<()> {
        if app.needs_full_terminal_clear {
            terminal.clear()?;
            app.needs_full_terminal_clear = false;
        }
        let draw_started = Instant::now();
        terminal.draw(|f| app.render(f))?;
        let draw_cost = draw_started.elapsed();
        app.needs_redraw = false;
        *last_draw = Instant::now();
        *last_status_heartbeat = *last_draw;
        *effective_draw_interval = adaptive_draw_interval_after_draw(
            app.terminal_ui_profile,
            min_draw_interval,
            *effective_draw_interval,
            draw_cost,
        );
        Ok(())
    }

    let mut last_tick = Instant::now();
    let tick_rate = if app.animate_status_indicators {
        Duration::from_millis(80)
    } else {
        Duration::from_millis(250)
    };

    let min_draw_interval = app.min_draw_interval;
    let mut effective_draw_interval = min_draw_interval;
    let mut last_draw = Instant::now() - min_draw_interval;
    let mut last_status_heartbeat = Instant::now();

    loop {
        app.check_responses();
        app.poll_remote_skill_search();
        app.poll_remote_plugin_search();
        app.poll_remote_mcp_search();
        app.poll_mcp_add_discovery();
        app.refresh_log_follow_if_due_at(Instant::now());

        let now_elapsed = last_tick.elapsed();
        if now_elapsed >= tick_rate {
            if app.tick_spinner() {
                app.needs_redraw = true;
            }
            last_tick = Instant::now();
        }

        if !app.animate_status_indicators
            && app.needs_periodic_status_refresh()
            && last_status_heartbeat.elapsed() >= heartbeat_interval_for(app.terminal_ui_profile)
        {
            app.needs_redraw = true;
            last_status_heartbeat = Instant::now();
        }

        let draw_interval_ok = last_draw.elapsed() >= effective_draw_interval;
        if app.needs_redraw && draw_interval_ok {
            draw_app(
                terminal,
                app,
                &mut effective_draw_interval,
                min_draw_interval,
                &mut last_draw,
                &mut last_status_heartbeat,
            )?;
        }

        let poll_timeout = if effective_draw_interval >= Duration::from_millis(120) {
            Duration::from_millis(40)
        } else if app.is_processing {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(40)
        };

        let mut priority_interaction = false;
        if event::poll(poll_timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    app.handle_key_event(key);
                    priority_interaction = true;
                }
                Event::Paste(text) => {
                    app.handle_paste(text);
                    priority_interaction = true;
                }
                Event::Mouse(mouse) => {
                    app.handle_mouse_event(mouse);
                    priority_interaction = true;
                }
                Event::Resize(_, _) => {
                    app.needs_redraw = true;
                    app.needs_full_terminal_clear = true;
                    for line in &mut app.output {
                        line.invalidate_render_cache();
                    }
                    priority_interaction = true;
                }
                _ => {}
            }
        }

        app.check_responses();

        if priority_interaction && app.needs_redraw {
            draw_app(
                terminal,
                app,
                &mut effective_draw_interval,
                min_draw_interval,
                &mut last_draw,
                &mut last_status_heartbeat,
            )?;
        }

        if let Some(enabled) = app.take_mouse_capture_request() {
            if enabled {
                crossterm::execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture)?;
            } else {
                crossterm::execute!(
                    terminal.backend_mut(),
                    crossterm::event::DisableMouseCapture
                )?;
            }
        }

        if app.should_exit() {
            if app.voice_recording.is_some() {
                app.abort_voice_recording("Voice recording cancelled during exit.");
            }
            return Ok(());
        }
    }
}
