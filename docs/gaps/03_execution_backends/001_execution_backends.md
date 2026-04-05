# Execution Backends Gap Analysis

## Bottom line

EdgeCrab now matches Hermes on the six core execution worlds, on direct plus managed Modal transport variants, and on background-process routing. It exceeds Hermes on backend-system design and now closes the last direct-Modal filesystem gap that previously prevented a clean surpass claim.

That needed re-audit before it was credible.

The earlier version was too broad because Hermes still had one execution-adjacent strength that mattered:

- a deeper direct-Modal filesystem path with snapshot persistence and host-file sync

After re-audit and code closure, that item is no longer open.

## Audited facts

Hermes ships six core execution worlds under `tools/environments/`:

- `local.py`
- `docker.py`
- `modal.py`
- `daytona.py`
- `ssh.py`
- `singularity.py`

EdgeCrab ships the same six core worlds under `crates/edgecrab-tools/src/tools/backends/`:

- `local.rs`
- `docker.rs`
- `modal.rs`
- `daytona.rs`
- `ssh.rs`
- `singularity.rs`

At raw backend inventory level, parity is closed.

That parity now extends beyond foreground commands.

Hermes' Modal stack still includes `managed_modal.py`.

EdgeCrab's `modal.rs` now supports typed Modal transport selection via `auto`, `direct`, and `managed` modes, including managed gateway resolution from explicit config, environment overrides, and `~/.edgecrab/auth.json` Nous tokens.

EdgeCrab now routes `run_process` through Docker / SSH / Modal / Daytona / Singularity by reusing the active `ExecutionBackend`, launching detached shell jobs inside the sandbox, and polling backend-side log / exit files through the same backend abstraction.

EdgeCrab's direct Modal backend now preserves requested working directories instead of flattening execution into the sandbox default cwd, which closes a subtle but real parity hole versus Hermes.

Its managed Modal path preserves working directories too, and the same remote-process machinery now runs through that transport as well.

EdgeCrab's direct Modal path now also restores task-scoped filesystem snapshots, deletes stale snapshot entries when restore fails, saves a fresh snapshot on cleanup, and syncs EdgeCrab auth / skills / cache files into the live sandbox before each command.

## Where EdgeCrab exceeds

### 1. The backend substrate is more coherent

EdgeCrab's six backends sit behind one Rust trait, one backend factory, one typed config surface, one cache model, and one health-check contract.

From first principles, this matters because execution backends are only useful when the rest of the runtime can treat them uniformly:

- same dispatch path
- same cancellation contract
- same cleanup contract
- same cache eviction logic
- same output-shaping contract

Hermes has the same core worlds, but EdgeCrab's typed backend layer is cleaner to reason about and easier to extend safely.

### 2. The parity backends were added without special-casing the architecture

EdgeCrab's new Daytona and Singularity backends plug into the same `BackendKind`, `BackendConfig`, `ExecutionBackend`, and `build_backend()` flow as the existing backends.

That yields:

- compile-time interface consistency
- consistent runtime selection
- uniform health-based cache eviction
- easier reuse from the rest of the async runtime

That is a real exceed, not just a parity statement. EdgeCrab did not bolt on extra backend branches outside the abstraction boundary.

### 3. Backend lifecycle handling is now explicit instead of sticky

EdgeCrab's terminal backend cache now has code-backed lifecycle rules:

- unhealthy cached backends are evicted and rebuilt on access
- idle cached backends can be reaped with one typed cleanup path
- CLI and gateway shutdown call explicit backend cleanup hooks
- cleanup only reaps entries when the cache is the last owner, so in-flight commands are not torn down underneath active callers

Hermes has inactive-environment cleanup too, but EdgeCrab's cache manager is now easier to audit because the ownership and eviction rules live in one typed module instead of being spread across thread-managed dictionaries and backend objects.

### 4. Edge-case handling is explicit across the backend substrate

EdgeCrab now has source-backed tests covering:

- full terminal-tool dispatch into Modal via a fake HTTP API, including working-directory preservation
- full terminal-tool dispatch into managed Modal via a fake gateway API
- Modal auto-mode fallback to managed transport when direct credentials are absent
- descriptive direct-mode failure when Modal credentials are missing
- backend kind parsing and serde
- full terminal-tool dispatch into Daytona via a fake helper
- full terminal-tool dispatch into Singularity via a fake runtime
- full non-local `run_process` launch / wait / kill flows through fake managed Modal, Daytona, and Singularity backends
- idle backend cleanup resetting stale shell state end-to-end
- timeout behavior
- cleanup behavior
- actionable missing-runtime errors

From first principles, backends are failure-handling systems. Testing the unhappy path is part of the feature, not an afterthought.

## Residual Hermes advantages

Hermes no longer holds a code-surface lead in execution backends.

It still has one narrower advantage:

- more historical operating mileage in the field

## Gap verdict

After re-assessment, the defensible claim is:

EdgeCrab is no longer behind on execution backends. It matches Hermes on the six core execution worlds, on direct plus managed Modal transport variants, on direct-Modal filesystem persistence/sync semantics, and on non-local background-process routing. It exceeds Hermes on typed backend integration, uniform cache/lifecycle handling, and code-level cohesion.

The remaining caution is operational rather than architectural: Hermes still has more field mileage. The code-surface gap itself is closed.

## Sources audited

- `edgecrab/crates/edgecrab-tools/src/tools/backends/mod.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/daytona.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/local.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/docker.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/modal.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/ssh.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/backends/singularity.rs`
- `edgecrab/crates/edgecrab-core/src/config.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/terminal.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/process.rs`
- `edgecrab/crates/edgecrab-tools/src/process_table.rs`
- `edgecrab/crates/edgecrab-tools/tests/terminal_backends.rs`
- `edgecrab/crates/edgecrab-cli/src/main.rs`
- `edgecrab/crates/edgecrab-gateway/src/run.rs`
- `edgecrab/docs/008_environments/001_environments.md`
- `hermes-agent/tools/terminal_tool.py`
- `hermes-agent/tools/process_registry.py`
- `hermes-agent/tools/environments/base.py`
- `hermes-agent/tools/environments/local.py`
- `hermes-agent/tools/environments/docker.py`
- `hermes-agent/tools/environments/modal.py`
- `hermes-agent/tools/environments/managed_modal.py`
- `hermes-agent/tools/environments/daytona.py`
- `hermes-agent/tools/environments/ssh.py`
- `hermes-agent/tools/environments/singularity.py`
