//! App wiring for the provider base-URL overlay.

use super::*;
use crate::provider_endpoint_overlay::{
    EndpointOverlayPhase, ProbeStatus, ProviderEndpointOverlay, render_endpoint_overlay,
};
use edgecrab_core::provider_endpoints::{self, ProviderEndpointConfig};

impl App {
    pub(super) fn open_provider_endpoint_overlay(&mut self) {
        let mut overlay = ProviderEndpointOverlay::new();
        if let Some((provider, _)) = self
            .current_model_spec()
            .and_then(|s| edgecrab_tools::vision_models::parse_provider_model_spec(&s))
        {
            let specs = overlay.filtered_specs();
            if let Some(idx) = specs.iter().position(|s| s.id == provider) {
                overlay.cursor = idx;
            }
        }
        self.provider_endpoint_overlay = Some(overlay);
        self.needs_redraw = true;
    }

    pub(super) fn close_provider_endpoint_overlay(&mut self) {
        self.provider_endpoint_overlay = None;
        self.needs_redraw = true;
    }

    fn current_model_spec(&self) -> Option<String> {
        let m = self.load_runtime_config().model.default_model;
        if m.trim().is_empty() { None } else { Some(m) }
    }

    pub(super) fn handle_provider_endpoint_key(&mut self, key: crossterm::event::KeyEvent) {
        if self.provider_endpoint_overlay.is_none() {
            return;
        }

        // Snapshot phase kind without holding a borrow across config loads.
        let is_edit = matches!(
            self.provider_endpoint_overlay.as_ref().map(|o| &o.phase),
            Some(EndpointOverlayPhase::Edit { .. })
        );
        let filter_active = self
            .provider_endpoint_overlay
            .as_ref()
            .map(|o| o.filter_active)
            .unwrap_or(false);

        if is_edit {
            match key.code {
                KeyCode::Esc => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                        ov.cancel_edit();
                    }
                }
                KeyCode::Enter => {
                    self.commit_endpoint_edit();
                    return;
                }
                KeyCode::Backspace => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut()
                        && let EndpointOverlayPhase::Edit { buffer, .. } = &mut ov.phase
                    {
                        buffer.pop();
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut()
                        && let EndpointOverlayPhase::Edit { buffer, .. } = &mut ov.phase
                    {
                        buffer.push(c);
                    }
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        if filter_active {
            match key.code {
                KeyCode::Esc => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                        ov.filter_active = false;
                    }
                }
                KeyCode::Enter => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                        ov.filter_active = false;
                        ov.clamp_cursor();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                        ov.filter.pop();
                        ov.cursor = 0;
                        ov.clamp_cursor();
                    }
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                        ov.filter.push(c);
                        ov.cursor = 0;
                        ov.clamp_cursor();
                    }
                }
                _ => {}
            }
            self.needs_redraw = true;
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.close_provider_endpoint_overlay();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                    ov.move_cursor(-1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                    ov.move_cursor(1);
                }
            }
            KeyCode::Char('/') => {
                if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                    ov.filter_active = true;
                }
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                let config = self.load_runtime_config();
                let config_url = self
                    .provider_endpoint_overlay
                    .as_ref()
                    .and_then(|o| o.selected_spec())
                    .and_then(|s| {
                        config
                            .provider_endpoints
                            .get(s.id)
                            .and_then(|e| e.base_url.clone())
                    });
                if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                    ov.begin_edit(config_url.as_deref());
                }
            }
            KeyCode::Char('r') => {
                self.reset_selected_endpoint();
                return;
            }
            KeyCode::Char('p') => {
                self.probe_selected_endpoint();
                return;
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn reset_selected_endpoint(&mut self) {
        let provider_id = self
            .provider_endpoint_overlay
            .as_ref()
            .and_then(|o| o.selected_spec())
            .map(|s| s.id.to_string());
        let Some(provider_id) = provider_id else {
            return;
        };
        let mut config = self.load_runtime_config();
        config.provider_endpoints.remove(&provider_id);
        match config.save() {
            Ok(()) => {
                let _ = provider_endpoints::set_runtime_override(&provider_id, None);
                self.push_output(
                    format!("Endpoint override cleared for {provider_id} (using default/env)."),
                    OutputRole::System,
                );
            }
            Err(e) => self.push_output(format!("Failed to save config: {e}"), OutputRole::Error),
        }
        self.needs_redraw = true;
    }

    fn commit_endpoint_edit(&mut self) {
        let (provider_id, buffer) = match self.provider_endpoint_overlay.as_ref() {
            Some(ov) => match &ov.phase {
                EndpointOverlayPhase::Edit {
                    provider_id,
                    buffer,
                    ..
                } => (provider_id.clone(), buffer.clone()),
                _ => return,
            },
            None => return,
        };

        match provider_endpoints::normalize_base_url(&buffer) {
            Ok(normalized) => {
                let mut config = self.load_runtime_config();
                if normalized.is_empty() {
                    config.provider_endpoints.remove(&provider_id);
                    let _ = provider_endpoints::set_runtime_override(&provider_id, None);
                } else {
                    config.provider_endpoints.insert(
                        provider_id.clone(),
                        ProviderEndpointConfig {
                            base_url: Some(normalized.clone()),
                        },
                    );
                    let _ =
                        provider_endpoints::set_runtime_override(&provider_id, Some(&normalized));
                }
                match config.save() {
                    Ok(()) => {
                        self.push_output(
                            if normalized.is_empty() {
                                format!("Cleared base URL override for {provider_id}")
                            } else {
                                format!("Set {provider_id} base URL → {normalized}")
                            },
                            OutputRole::System,
                        );
                        if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
                            ov.phase = EndpointOverlayPhase::Browse;
                        }
                    }
                    Err(e) => {
                        if let Some(ov) = self.provider_endpoint_overlay.as_mut()
                            && let EndpointOverlayPhase::Edit { error, .. } = &mut ov.phase
                        {
                            *error = Some(format!("save failed: {e}"));
                        }
                    }
                }
            }
            Err(msg) => {
                if let Some(ov) = self.provider_endpoint_overlay.as_mut()
                    && let EndpointOverlayPhase::Edit { error, .. } = &mut ov.phase
                {
                    *error = Some(msg);
                }
            }
        }
        self.needs_redraw = true;
    }

    fn probe_selected_endpoint(&mut self) {
        let (provider_id, url) = {
            let Some(overlay) = self.provider_endpoint_overlay.as_ref() else {
                return;
            };
            let Some(spec) = overlay.selected_spec() else {
                return;
            };
            let config = self.load_runtime_config();
            let (url, _) =
                provider_endpoints::resolve_endpoint(spec.id, &config.provider_endpoints)
                    .unwrap_or((
                        spec.default_base_url.to_string(),
                        provider_endpoints::EndpointSource::Default,
                    ));
            (spec.id.to_string(), url)
        };

        if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
            ov.probe.insert(provider_id.clone(), ProbeStatus::Pending);
        }
        self.needs_redraw = true;

        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match rt {
                Ok(rt) => rt.block_on(provider_endpoints::probe_endpoint(&url, 2500)),
                Err(e) => Err(format!("runtime: {e}")),
            }
        })
        .join()
        .unwrap_or_else(|_| Err("probe thread panicked".into()));

        if let Some(ov) = self.provider_endpoint_overlay.as_mut() {
            ov.probe.insert(
                provider_id,
                match result {
                    Ok(msg) => ProbeStatus::Ok(msg),
                    Err(msg) => ProbeStatus::Err(msg),
                },
            );
        }
        self.needs_redraw = true;
    }

    pub(super) fn render_provider_endpoint_overlay(&self, frame: &mut Frame, area: Rect) {
        let Some(state) = self.provider_endpoint_overlay.as_ref() else {
            return;
        };
        let config = self.load_runtime_config();
        render_endpoint_overlay(frame, area, state, &config.provider_endpoints);
    }
}
