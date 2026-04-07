
# Sandboxing (Deep Dive)

EdgeCrab implements sandboxed code execution using a pluggable backend system, supporting **six execution backends**:

- **Local** (default): Runs commands as subprocesses, strips secrets from env, persistent shell, fast but no isolation. ([local.rs](../../crates/edgecrab-tools/src/tools/backends/local.rs))
- **Docker**: Runs each task in a container (bollard API), drops all Linux capabilities, PID limit, tmpfs at /tmp, workspace bind-mount, container removed on cleanup. ([docker.rs](../../crates/edgecrab-tools/src/tools/backends/docker.rs))
- **SSH**: Remote execution via openssh, multiplexed channel, env-var blocklist, persistent session. ([ssh.rs](../../crates/edgecrab-tools/src/tools/backends/ssh.rs))
- **Modal**: Cloud sandbox via Modal REST API, ephemeral sandboxes, env-var blocklist, best-effort integration. ([modal.rs](../../crates/edgecrab-tools/src/tools/backends/modal.rs))
- **Daytona**: Cloud dev env via Daytona Python SDK, persistent sandboxes, resource config. ([daytona.rs](../../crates/edgecrab-tools/src/tools/backends/daytona.rs))
- **Singularity/Apptainer**: HPC container backend, persistent overlays, preflight checks. ([singularity.rs](../../crates/edgecrab-tools/src/tools/backends/singularity.rs))

## Tooling & API

- The [`execute_code`](../../crates/edgecrab-tools/src/tools/execute_code.rs) tool exposes a **7-tool RPC** (Python script can call agent tools via Unix socket, e.g. web_search, file_read, etc.).
- Only a safe subset of tools is exposed (no memory, no agent spawning, no browser, no persistent mutation).
- Non-Python languages (JS, Bash, etc.) run without RPC tool access.
- **Timeouts**: 5 min max per execution (configurable), process group kill with SIGTERM→SIGKILL.
- **Output limits**: 50KB stdout, 10KB stderr (head+tail truncation).
- **Tool call limit**: 50 per script.

## Security Model

- **Env-var blocklist**: All backends strip API keys/secrets from child env (see [local.rs](../../crates/edgecrab-tools/src/tools/backends/local.rs), `HARDCODED_BLOCKLIST`).
- **Path policy**: All file access is mediated by [path_policy.rs](../../crates/edgecrab-security/src/path_policy.rs) — canonicalizes, enforces workspace root, supports virtual tmp roots.
- **Containerization**: Docker/Modal/Daytona/Singularity provide OS-level isolation; Docker drops all Linux capabilities, PID limit, tmpfs, workspace bind-mount.
- **Remote**: SSH backend does not propagate secrets, uses strict host key checking if configured.
- **No macOS Seatbelt**: Unlike EdgeCode, EdgeCrab does not use macOS-specific Seatbelt policies; all isolation is via containers or remote sandboxes.

## Configuration

- Backend is selected via `config.yaml` (`terminal.backend`) or `EDGECRAB_TERMINAL_BACKEND` env var.
- Each backend has its own config struct (resource limits, image, credentials, etc.).
- See [docs/008_environments/001_environments.md](../../docs/008_environments/001_environments.md) for backend comparison and config options.

## Edge Cases & Limitations

- **Local backend**: Fastest, but no isolation; only env-var blocklist and path policy protect the host.
- **Docker**: Requires Docker daemon, workspace must be bind-mountable, not available on all platforms.
- **Modal/Daytona**: Workspace files are not automatically synced; file tools are rooted in the sandbox, not the host.
- **Singularity**: Requires Apptainer/Singularity installed, best for HPC.
- **No OS-level policy on macOS**: No Seatbelt or App Sandbox; consider for future parity with EdgeCode.
- **Tooling**: Only a safe subset of tools is available in the sandbox; agent cannot spawn itself recursively.

## Key Code & Docs

- [execute_code.rs](../../crates/edgecrab-tools/src/tools/execute_code.rs)
- [backends/mod.rs](../../crates/edgecrab-tools/src/tools/backends/mod.rs)
- [path_policy.rs](../../crates/edgecrab-security/src/path_policy.rs)
- [docs/008_environments/001_environments.md](../../docs/008_environments/001_environments.md)
- [terminal.rs](../../crates/edgecrab-tools/src/tools/terminal.rs)

---
**TODOs:**
- Consider adding macOS Seatbelt/App Sandbox support for parity with EdgeCode.
- Add per-backend resource limit config in user-facing docs.
- Document how to sync workspace files for remote/cloud backends.
