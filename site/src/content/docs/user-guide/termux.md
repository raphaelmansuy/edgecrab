---
title: Termux / Android
description: Building and running EdgeCrab on Android via Termux. Grounded in crates/edgecrab-cli/Cargo.toml and crates/edgecrab-cli/src/app.rs.
sidebar:
  order: 8
---

EdgeCrab can run on Android devices via [Termux](https://termux.dev/), the terminal emulator and Linux environment for Android.

---

## Building for Termux

Cross-compile from a Linux/macOS host:

```bash
make build-termux
# Equivalent to:
# cargo build --release --target aarch64-linux-android --features termux
```

The `termux` feature flag enables Android-specific adaptations.

---

## Automatic Adaptations

When running on Termux (detected via `IS_TERMUX` environment flag or terminal width < 60 columns):

### TUI Compact Mode

The TUI automatically switches to `BasicCompat` profile:
- Simplified rendering for small screens
- Reduced chrome and decorations
- Optimized for narrow terminal widths

### Path Jail

The Termux data directory is automatically added to allowed file paths:
- Uses `$PREFIX` environment variable if set
- Falls back to `/data/data/com.termux/files`
- All standard file tools work within this directory

---

## Environment Setup

```bash
# In Termux
pkg install rust   # or install via rustup
pkg install openssl-dev   # for TLS

# Set required API key
export ANTHROPIC_API_KEY=sk-...

# Run EdgeCrab
./edgecrab
```

---

## Limitations

- No GPU acceleration for TUI rendering
- Smaller default terminal size triggers compact mode
- Some tools (e.g. browser automation) may not be available
- Cross-compilation requires Android NDK toolchain
