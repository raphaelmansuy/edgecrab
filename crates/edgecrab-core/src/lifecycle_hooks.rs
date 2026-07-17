//! Core lifecycle hook events — file-based scripts under `~/.edgecrab/hooks/`.
//!
//! Fire-and-forget subprocess hooks (non-blocking, best-effort). Mirrors the
//! gateway `HookRegistry` discovery
//! layout but targets agent-loop boundaries instead of platform events.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

static GLOBAL_REGISTRY: OnceLock<LifecycleHookRegistry> = OnceLock::new();

/// Agent lifecycle events emitted from the conversation loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LifecycleEvent {
    SessionStart,
    TurnBefore,
    TurnAfter,
    ToolBefore,
    ToolAfter,
    CompressAfter,
    SkillsChanged,
}

impl LifecycleEvent {
    /// Directory name under `~/.edgecrab/hooks/<name>/`.
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::TurnBefore => "turn_before",
            Self::TurnAfter => "turn_after",
            Self::ToolBefore => "tool_before",
            Self::ToolAfter => "tool_after",
            Self::CompressAfter => "compress_after",
            Self::SkillsChanged => "skills_changed",
        }
    }

    /// Flat-file basename prefix (`<name>.sh`, `<name>.py`, …).
    pub fn flat_prefix(self) -> &'static str {
        self.dir_name()
    }
}

/// Loaded script hook metadata.
#[derive(Debug, Clone)]
pub struct LifecycleHookScript {
    pub event: LifecycleEvent,
    pub path: PathBuf,
    pub runtime: &'static str,
}

/// Registry of discovered lifecycle hook scripts.
#[derive(Debug, Default)]
pub struct LifecycleHookRegistry {
    scripts: Vec<LifecycleHookScript>,
}

impl LifecycleHookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load hooks from `~/.edgecrab/hooks/` (or `EDGECRAB_HOME/hooks`).
    pub fn discover_and_load(&mut self) {
        if let Some(dir) = hooks_dir() {
            self.discover_and_load_from(&dir);
        }
    }

    /// Testable entry: load from an explicit hooks root.
    pub fn discover_and_load_from(&mut self, hooks_dir: &Path) {
        self.scripts.clear();
        if !hooks_dir.is_dir() {
            return;
        }

        for event in [
            LifecycleEvent::SessionStart,
            LifecycleEvent::TurnBefore,
            LifecycleEvent::TurnAfter,
            LifecycleEvent::ToolBefore,
            LifecycleEvent::ToolAfter,
            LifecycleEvent::CompressAfter,
            LifecycleEvent::SkillsChanged,
        ] {
            let event_dir = hooks_dir.join(event.dir_name());
            if event_dir.is_dir() {
                self.load_scripts_in_dir(event, &event_dir);
            }
            self.load_flat_scripts(hooks_dir, event);
        }

        tracing::debug!(count = self.scripts.len(), "lifecycle hooks loaded");
    }

    fn load_scripts_in_dir(&mut self, event: LifecycleEvent, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_file()
                && let Some((runtime, _)) = detect_runtime(&path)
            {
                self.scripts.push(LifecycleHookScript {
                    event,
                    path,
                    runtime,
                });
            }
        }
    }

    fn load_flat_scripts(&mut self, hooks_dir: &Path, event: LifecycleEvent) {
        let prefix = event.flat_prefix();
        let Ok(entries) = std::fs::read_dir(hooks_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if stem == prefix
                && let Some((runtime, _)) = detect_runtime(&path)
            {
                self.scripts.push(LifecycleHookScript {
                    event,
                    path,
                    runtime,
                });
            }
        }
    }

    pub fn script_count(&self) -> usize {
        self.scripts.len()
    }

    pub fn scripts_for(&self, event: LifecycleEvent) -> Vec<&LifecycleHookScript> {
        self.scripts
            .iter()
            .filter(|s| s.event == event)
            .collect()
    }

    /// Fire-and-forget: spawn matching scripts with JSON context on stdin.
    pub fn emit(&self, event: LifecycleEvent, context: Value) {
        let scripts: Vec<LifecycleHookScript> = self
            .scripts
            .iter()
            .filter(|s| s.event == event)
            .cloned()
            .collect();
        if scripts.is_empty() {
            return;
        }

        let mut payload = context;
        if let Some(obj) = payload.as_object_mut() {
            obj.entry("event")
                .or_insert_with(|| Value::String(event.dir_name().into()));
        } else {
            payload = serde_json::json!({ "event": event.dir_name(), "data": payload });
        }

        let json = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "lifecycle hook context serialization failed");
                return;
            }
        };

        for script in scripts {
            let input = json.clone();
            let path = script.path.clone();
            let runtime = script.runtime;
            tokio::spawn(async move {
                let mut cmd = if runtime.is_empty() {
                    tokio::process::Command::new(&path)
                } else {
                    let mut c = tokio::process::Command::new(runtime);
                    c.arg(&path);
                    c
                };
                cmd
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null());
                match cmd.spawn() {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            use tokio::io::AsyncWriteExt;
                            let _ = stdin.write_all(input.as_bytes()).await;
                        }
                        let _ = child.wait().await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            hook = %path.display(),
                            error = %e,
                            "lifecycle hook spawn failed (non-fatal)"
                        );
                    }
                }
            });
        }
    }
}

/// Shared registry (lazy-loaded once per process).
pub fn global_registry() -> &'static LifecycleHookRegistry {
    GLOBAL_REGISTRY.get_or_init(|| {
        let mut reg = LifecycleHookRegistry::new();
        reg.discover_and_load();
        reg
    })
}

/// Emit on the process-global registry (best-effort).
pub fn emit_global(event: LifecycleEvent, context: Value) {
    global_registry().emit(event, context);
}

/// Resolve `~/.edgecrab/hooks/` (or `$EDGECRAB_HOME/hooks`).
///
/// Shared by core lifecycle hooks and gateway `HookRegistry` (DRY).
pub fn hooks_home() -> Option<PathBuf> {
    std::env::var("EDGECRAB_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".edgecrab")))
        .map(|home| home.join("hooks"))
}

fn hooks_dir() -> Option<PathBuf> {
    hooks_home()
}

fn detect_runtime(path: &Path) -> Option<(&'static str, ())> {
    if let Ok(meta) = std::fs::metadata(path) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 != 0 {
                return Some(("", ()));
            }
        }
    }
    match path.extension().and_then(|e| e.to_str()) {
        Some("sh") => Some(("sh", ())),
        Some("py") => Some(("python3", ())),
        Some("js") | Some("ts") => Some(("bun", ())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn event_dir_name_mapping() {
        assert_eq!(LifecycleEvent::SessionStart.dir_name(), "session_start");
        assert_eq!(LifecycleEvent::ToolBefore.dir_name(), "tool_before");
        assert_eq!(LifecycleEvent::SkillsChanged.dir_name(), "skills_changed");
    }

    #[test]
    fn discover_event_subdir_and_flat_file() {
        let tmp = tempfile::tempdir().expect("tempdir");

        let sub = tmp.path().join("hooks").join("turn_before");
        std::fs::create_dir_all(&sub).expect("mkdir");
        let sub_script = sub.join("log.py");
        std::fs::File::create(&sub_script)
            .and_then(|mut f| f.write_all(b"import sys; sys.exit(0)"))
            .expect("write sub script");

        let flat = tmp.path().join("hooks").join("session_start.sh");
        std::fs::create_dir_all(flat.parent().unwrap()).expect("mkdir hooks");
        std::fs::File::create(&flat)
            .and_then(|mut f| f.write_all(b"#!/bin/sh\nexit 0"))
            .expect("write flat script");

        let mut reg = LifecycleHookRegistry::new();
        reg.discover_and_load_from(&tmp.path().join("hooks"));

        assert_eq!(reg.scripts_for(LifecycleEvent::TurnBefore).len(), 1);
        assert_eq!(reg.scripts_for(LifecycleEvent::SessionStart).len(), 1);
        assert_eq!(reg.script_count(), 2);
    }
}
