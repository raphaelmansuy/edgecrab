# Update Command Overview

## Problem

EdgeCrab advertises `/update` in the TUI, but today it only inspects git state.
That does not solve the real user problem:

- users install EdgeCrab through multiple channels
- the app should tell them when a new release exists
- the app must not block startup while checking
- package-managed installs must be updated through their owning package manager

## Scope

This specification covers:

- `edgecrab update` as a first-class CLI subcommand
- `/update` in the TUI
- non-blocking update notices on startup for CLI and TUI
- install-channel detection for `npm`, `pypi`, `cargo`, `brew`, `source`, and manual binary installs
- release-process and CI/CD adjustments needed to keep update metadata correct

This specification does not cover:

- automatic background self-replacement of the running executable
- auto-applying updates on startup without user intent
- Docker self-update flows

## First Principles

- Code is law: update behavior must be derived from the actual install channel and actual published versions, not from vague heuristics or docs-only promises.
- Respect ownership: if a package manager owns the installation, EdgeCrab must delegate the upgrade to that package manager.
- Never block startup: release checks run in the background with strict timeouts and cache reuse.
- Prefer one source of truth per concern:
  - installed version: current running binary
  - release discovery: latest GitHub release
  - release version authority: workspace `[workspace.package].version`
  - channel-specific install command: resolved install method
- Fail soft: network, registry, and shell failures must degrade to "no notice" or an actionable report, never a startup failure.

## Deliverables

- shared updater module in `crates/edgecrab-cli`
- new `edgecrab update` subcommand
- improved `/update` slash command
- startup release notice in TUI and non-interactive/CLI modes
- persisted update-check cache/state under `~/.edgecrab/`
- config knobs for update checks
- release documentation and CI updates
- tests for channel detection, version parsing, cache behavior, rendering, and command generation

## High-Level Decision

EdgeCrab will:

1. Detect how it was installed.
2. Check the latest GitHub release asynchronously with a short timeout.
3. Cache the result locally to avoid repeated network hits and startup latency.
4. Show a startup notice only when a newer version exists.
5. Run the correct upgrade command for the detected install channel when the user invokes `edgecrab update`.

## Why This Approach

- It is seamless for users because `edgecrab update` works from the app they already use.
- It avoids unsafe self-mutation of package-managed files.
- It keeps release discovery consistent across channels.
- It remains robust if npm, PyPI, crates.io, or Homebrew lags slightly behind the GitHub release.
