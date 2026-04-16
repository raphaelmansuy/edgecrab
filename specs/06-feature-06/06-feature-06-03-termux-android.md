# ADR-0603: Termux / Android Support

| Field       | Value                                              |
|-------------|----------------------------------------------------|
| Status      | Implemented                                        |
| Date        | 2026-04-14                                         |
| Implements  | hermes-agent PR #6834                              |
| Crates      | `edgecrab-cli`, `edgecrab-tools`, `edgecrab-core`  |

---

## 1. Context

hermes-agent v0.9.0 added Termux/Android support — adapting install paths, TUI
layout for small screens, voice backend fallbacks, and `/image` command for
Android. EdgeCrab is a compiled Rust binary (no Python wheels problem), but
still needs: cross-compilation for `aarch64-linux-android`, TUI adaptations
for narrow terminals, and graceful feature degradation.

---

## 2. First Principles

| Principle       | Application                                              |
|-----------------|----------------------------------------------------------|
| **SRP**         | Termux detection isolated to a single function           |
| **OCP**         | Feature gates, not if/else branches throughout codebase  |
| **DRY**         | Single `Platform::is_termux()` predicate                 |
| **Code is Law** | hermes-agent `hermes_constants.py:is_termux()` as ref    |

---

## 3. Architecture

```
+------------------------------------------------------------------+
|                    Termux Environment                             |
|                                                                   |
|  +------------------+   +--------------------+   +--------------+ |
|  | Detection        |   | TUI Adaptations    |   | Feature      | |
|  | is_termux()      |   | Narrow screen mode |   | Degradation  | |
|  | TERMUX_VERSION   |   | Compact help text  |   | No Docker    | |
|  | PREFIX path      |   | Responsive layout  |   | No browser   | |
|  +------------------+   +--------------------+   | No voice(?)  | |
|                                                   +--------------+ |
|  +------------------+   +--------------------+                    |
|  | Cross-Compile    |   | Doctor Adaptations |                    |
|  | aarch64-linux-   |   | Skip Docker check  |                    |
|  | android target   |   | pkg install hints  |                    |
|  +------------------+   +--------------------+                    |
+------------------------------------------------------------------+
```

---

## 4. Detection

### 4.1 hermes-agent Reference

```python
# hermes_constants.py:161-168
def is_termux() -> bool:
    prefix = os.getenv("PREFIX", "")
    return bool(os.getenv("TERMUX_VERSION") or "com.termux/files/usr" in prefix)
```

### 4.2 EdgeCrab Implementation

```rust
// crates/edgecrab-types/src/platform.rs (or lib.rs)
pub fn is_termux() -> bool {
    std::env::var("TERMUX_VERSION").is_ok()
        || std::env::var("PREFIX")
            .map(|p| p.contains("com.termux/files/usr"))
            .unwrap_or(false)
}
```

Cache the result at startup — env vars don't change mid-process:

```rust
use std::sync::LazyLock;
pub static IS_TERMUX: LazyLock<bool> = LazyLock::new(is_termux);
```

---

## 5. Impact Areas

### 5.1 Cross-Compilation

```
Cargo target: aarch64-linux-android

Required:
  - Android NDK (r25+)
  - Ring crypto: needs android-specific build flags
  - OpenSSL: use rustls instead (already default in edgecrab)

Build command:
  cargo build --target aarch64-linux-android --release \
    --no-default-features --features termux
```

### 5.2 Feature Flags

```toml
# Cargo.toml (workspace)
[features]
termux = []          # Compile-time flag for Termux-specific behavior
# Default features exclude heavy desktop-only deps
```

### 5.3 TUI Adaptations (edgecrab-cli)

| Area              | Desktop              | Termux                               | Source                           |
|-------------------|----------------------|--------------------------------------|----------------------------------|
| Terminal width    | Full-width layout    | Compact layout (< 60 cols)           | `cli.py:L1082`                   |
| Help text         | Full descriptions    | Abbreviated, Termux-specific tips    | `cli.py:L1082`                   |
| Image paste       | Clipboard integration| File path hint (`~/storage/...`)     | `cli.py:L1082`                   |
| Banner            | Full ASCII art       | Single-line banner (narrow fit)      | `cli.py:L1082`                   |
| Status bar        | Full metrics         | Abbreviated (model + cost only)      | `cli.py:L1082`                   |
| Scroll            | Mouse scroll events  | Touch-friendly key bindings          | New                              |

### 5.4 Doctor Adaptations (edgecrab-cli)

| Check              | Desktop Behavior             | Termux Behavior                       |
|--------------------|------------------------------|---------------------------------------|
| Docker backend     | Check Docker availability    | Skip: "not available on Android"      |
| Browser/CDP        | Check agent-browser          | Info: "expected in Termux"            |
| Node.js            | `brew install node`          | `pkg install nodejs`                  |
| Package install    | `cargo install`              | `pkg install edgecrab` (if available) |
| Gateway service    | systemd/launchd              | Manual PID detection                  |
| Background warning | None                         | "Android may stop background jobs"    |

### 5.5 Gateway on Termux

```
Problem: Android kills background processes when Termux is suspended.

Mitigations:
  1. Termux:Boot add-on for auto-restart
  2. Wake lock notification (Termux-specific)
  3. Foreground service hint in doctor output
  4. Warn user on gateway start if Termux detected
```

### 5.6 Tool Degradation

```
+--------------------+--------+-------+---------------------------+
| Tool               | Desktop| Termux| Degradation               |
+--------------------+--------+-------+---------------------------+
| terminal           | full   | full  | bash, not zsh default     |
| file_read/write    | full   | full  | paths under /data/data/.. |
| browser (CDP)      | full   | skip  | No Chrome on Android      |
| execute_code       | full   | full  | Python via pkg install    |
| text_to_speech     | full   | skip  | No ctranslate2 wheels     |
| transcribe         | full   | skip  | No whisper on Android     |
| vision             | full   | full  | API-based, works          |
| web_search         | full   | full  | API-based, works          |
+--------------------+--------+-------+---------------------------+
```

---

## 6. Edge Cases & Roadblocks

| # | Edge Case                            | Remediation                                     | Source                      |
|---|--------------------------------------|--------------------------------------------------|------------------------------|
| 1 | Ring crate fails to compile          | Use rustls (already default), avoid ring          | Cross-compile known issue    |
| 2 | Terminal width < 40 columns          | Absolute minimum layout, truncate long strings    | `cli.py:L1082`               |
| 3 | `/data/data/com.termux` path jail    | Expand allowed roots to include Termux paths      | edgecrab-security path_jail  |
| 4 | No systemd/launchd for gateway       | PID file + manual process management              | `hermes_cli/status.py:L332`  |
| 5 | Android kills bg processes           | Warn user, suggest Termux:Boot                    | `hermes_cli/status.py:L348`  |
| 6 | Storage permissions                  | Guide user to `termux-setup-storage`              | `doctor.py:L55`              |
| 7 | No `/tmp` directory                  | Use `$TMPDIR` (Termux sets this)                  | Standard Termux behavior     |
| 8 | Clipboard unavailable                | Fall back to file path input for `/paste`          | `cli.py:L1082`               |
| 9 | Keyboard height reduces viewport     | Auto-shrink ratatui frame on resize signal         | New — SIGWINCH handler       |
| 10| `pkg` vs `apt` package manager       | Detect Termux and use `pkg` in suggestions         | `doctor.py:L55`              |

---

## 7. Implementation Plan

### 7.1 Files to Modify

| File                                       | Change                                       |
|--------------------------------------------|----------------------------------------------|
| `crates/edgecrab-types/src/lib.rs`         | Add `is_termux()` + `IS_TERMUX` lazy static |
| `crates/edgecrab-cli/src/app.rs`           | Termux-aware layout (compact mode)           |
| `crates/edgecrab-cli/src/doctor.rs`        | Termux-specific check adaptations            |
| `crates/edgecrab-cli/src/setup.rs`         | Termux-specific install guidance             |
| `crates/edgecrab-security/src/path_jail.rs`| Add Termux data dir to allowed roots         |
| `Cargo.toml` (workspace)                  | Add `termux` feature flag                    |
| `Makefile`                                 | Add `build-android` target                   |
| `Dockerfile`                               | Multi-stage for android NDK (optional)       |

### 7.2 Test Matrix

| Test                              | Validates                                  |
|-----------------------------------|--------------------------------------------|
| `test_is_termux_env_var`          | Detection via TERMUX_VERSION               |
| `test_is_termux_prefix`           | Detection via PREFIX path                  |
| `test_is_termux_false_desktop`    | False on normal Linux/macOS                |
| `test_compact_layout_narrow`      | TUI renders correctly at 40 cols           |
| `test_doctor_skips_docker`        | Docker check skipped on Termux             |
| `test_path_jail_termux_roots`     | Termux data dir is allowed root            |

---

## 8. Acceptance Criteria

- [ ] `is_termux()` detection function with `IS_TERMUX` lazy static
- [ ] Cross-compilation to `aarch64-linux-android` documented in Makefile
- [ ] TUI compact mode for terminals < 60 columns wide
- [ ] Doctor command adapts checks for Termux environment
- [ ] Path jail allows Termux data directory
- [ ] Browser/TTS/STT tools gracefully degrade (no panic) on Termux
- [ ] Gateway warns about Android background process limitations
- [ ] All tests pass with `TERMUX_VERSION=1` env override
