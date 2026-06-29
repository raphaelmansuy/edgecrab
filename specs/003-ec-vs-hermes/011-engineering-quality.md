# 011 — Engineering Quality

Tests, CI, maintainability, and honest debt — not feature marketing.

---

## Test scale

| | Hermes | EdgeCrab |
|---|--------|----------|
| Framework | pytest + xdist | cargo test |
| Approx. tests | ~25,000 (per architecture docs) | ~650+ |
| Test files | ~1,250 under `tests/` | Workspace integration + unit |
| Runner | **`scripts/run_tests.sh` required** | `cargo test --workspace` |
| Hermetic home | `HERMES_HOME` temp | `EDGECRAB_HOME` temp |
| Docker CI | `tests/docker/` | Less prominent |
| Windows lint | `check-windows-footguns.py` | Cross-compile possible |
| UI tests | 71 TS files (`ui-tui`) | Stream harness Rust tests |
| E2E ignored tests | Copilot/VS Code | `--include-ignored` ACP E2E |

**Verdict:** **Hermes leads raw test mass by 40×**. EdgeCrab leads **Rust compile-time guarantees** (types, clippy deny unwrap in prod).

**Brutal note:** Hermes test count includes extensive matrix/param tests; EC count is smaller but core paths are covered. Neither metric alone proves reliability.

---

## CI / quality gates

| Gate | Hermes | EdgeCrab |
|------|--------|----------|
| Lint | ruff/flake conventions | `clippy -- -D warnings` (zero warnings) |
| Format | black/ruff | `cargo fmt --check` |
| CI matrix | Linux/macOS/Windows docs | Ubuntu + macOS in `.github/workflows/ci.yml` |
| SDK CI | N/A | Python maturin + pytest |
| Release | PyPI / uv | Static binary releases |

**Verdict:** **EdgeCrab leads strictness** on compiler+clippy gate; **Hermes leads platform coverage**.

---

## Code organization debt

| Hotspot | Lines (approx.) | Risk |
|---------|-----------------|------|
| Hermes `tui_gateway/server.py` | ~10,000 | Gateway RPC complexity |
| Hermes `run_agent.py` | Large monolith | Agent changes ripple |
| EC `edgecrab-cli/src/app.rs` | ~34,000 | UI changes high-risk |
| EC `edgecrab-core/conversation.rs` | Manageable | Core loop readable |

**Extraction progress (EC):** `app/response_dispatch.rs`, `stream_forward.rs`, `event_loop.rs`, overlays — see [002-tui 007](../002-tui-hemes-vs-edgecrab/007-first-principles-lead-plan.md).

**Verdict:** **Both carry monolith debt** in different files. Hermes splits UI; EC splits crates but not TUI.

---

## Documentation

| | Hermes | EdgeCrab |
|---|--------|----------|
| User docs | Docusaurus site + i18n | `docs/` + AGENTS.md |
| Architecture | `website/docs/developer-guide/` | `specs/`, AGENTS.md |
| API reference | tools-reference, slash-commands | command-catalog crate |
| Gap tracking | Implicit in issues | Explicit `001-gap-analysis-v14/` |

**Verdict:** **Hermes leads user-facing docs**; **EdgeCrab leads internal spec discipline**.

---

## Migration tooling

| Path | Tool |
|------|------|
| Hermes → EdgeCrab | `edgecrab migrate` (`edgecrab-migrate/`) |
| OpenClaw → either | `hermes claw` / `edgecrab claw` |
| Config compat | `compat.rs` env key aliasing | 

**Verdict:** **EdgeCrab leads** Hermes migration story ( intentional successor).

---

## Performance (first principles)

| Workload | Hermes | EdgeCrab |
|----------|--------|----------|
| Cold start | Python import + optional Node TUI | Single binary (gap 022 tracks perf) |
| Steady-state RAM | Python + Node higher | Lower |
| Tool dispatch | Python async | Rust async |
| Gateway throughput | Proven at Nous scale | Rust theoretical advantage |

**Verdict:** **EdgeCrab leads resource efficiency** (hypothesis; gap 022 not fully closed).

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Test coverage breadth | A | B |
| Type/static safety | C | A |
| CI strictness | B+ | A |
| Monolith risk | B | C+ (app.rs) |
| User documentation | A | B |
| Migration | B | A |

---

## What "production-ready" means here

Both are **production-ready for coding agents** with different caveats:

- **Hermes:** pick when you need plugins, kanban, curator, broad OAuth, Teams/LINE, desktop.
- **EdgeCrab:** pick when you need binary deploy, Rust performance, LSP depth, shadow judge, Hermes migration.

Neither is production-ready for **all** Hermes plugins on day one — see [012-master-gap-matrix.md](012-master-gap-matrix.md).
