# 013 — Cross-Reference Index (Re-assessed)

**Date:** 2026-07-19

---

## 1. This folder

| Doc | Role |
|------|------|
| [000-code-is-law.md](000-code-is-law.md) | **Evidence ledger — read first** |
| [001-first-principles.md](001-first-principles.md) | J1–J10 + AE1–AE10 |
| [002-systems-architect-lens.md](002-systems-architect-lens.md) | Topology |
| [003-ai-engineer-harness-lens.md](003-ai-engineer-harness-lens.md) | Loop physics |
| [004-product-owner-lens.md](004-product-owner-lens.md) | Personas / wedge |
| [005-security-trust-lens.md](005-security-trust-lens.md) | AE7 |
| [006-tools-capabilities-lens.md](006-tools-capabilities-lens.md) | Capability surface |
| [007-gateway-ecosystem-lens.md](007-gateway-ecosystem-lens.md) | Messaging |
| [008-tui-operator-lens.md](008-tui-operator-lens.md) | Operator UX |
| [009-extensibility-sdk-lens.md](009-extensibility-sdk-lens.md) | AE10 |
| [010-engineering-quality-lens.md](010-engineering-quality-lens.md) | Maintainability |
| [011-master-gap-matrix.md](011-master-gap-matrix.md) | Synthesis |
| [012-borrow-reject-priority.md](012-borrow-reject-priority.md) | Action backlog |
| [014-improvement-plan.md](014-improvement-plan.md) | **SOLID/DRY harness/MCP plan + e2e** |
| [015-grok-build-tui-code-display.md](015-grok-build-tui-code-display.md) | **Grok Build TUI deep analysis** |
| [016-tui-edit-display-plan.md](016-tui-edit-display-plan.md) | **TUI code-edit improvement plan** |
| [017-session-forensics-2026-07-19.md](017-session-forensics-2026-07-19.md) | **Live forensics** · Pinguin + Hyperframe · AC-V1…V8 |
| [018-non-flaky-harness-best-practices-2026-07.md](018-non-flaky-harness-best-practices-2026-07.md) | **Non-flaky AE** · July 2026 harness BP catalog |
| [019-non-flaky-harness-improvement-plan.md](019-non-flaky-harness-improvement-plan.md) | **Implement plan** · DRY/SOLID · e2e · Waves A–D |
| [020-grok-xai-oauth-agent-plan.md](020-grok-xai-oauth-agent-plan.md) | **Grok OAuth agent** · edgequake-llm publish policy |
| [README.md](README.md) | Map |

---

## 2. Prior specs (do not rewrite)

| Spec | Use |
|------|-----|
| [`mcp/`](../mcp/) | MCP ADRs — pair with **014 WS-A** |
| [`001-gap-analysis-v14`](../001-gap-analysis-v14/) | Port backlog |
| [`003-ec-vs-hermes`](../003-ec-vs-hermes/) | Older matrix (pre-ledger; prefer 022) |
| [`017-hermes-vs-edgecrab`](../017-hermes-vs-edgecrab/) | Harness deep-dive |
| [`015`](../015-improve-harness-and-agent/) | games003 parity |
| [`018-agent-harness`](../018-agent-harness/) | Forensics proofs |
| [`002-tui-hemes-vs-edgecrab`](../002-tui-hemes-vs-edgecrab/) | TUI dimensions |
| [`019-skills-install`](../019-skills-install/) | Skills multi-lens |
| [`020-tool-streaming-visibility`](../020-tool-streaming-visibility/) | Focus tool pane |
| [`021-session-capability-grants`](../021-session-capability-grants/) | Trust elevation |
| [`AGENTS.md`](../../AGENTS.md) | Living architecture |

---

## 3. EdgeCrab anchors (high signal)

| Concern | Path |
|---------|------|
| Loop | `crates/edgecrab-core/src/conversation.rs` (`execute_loop` :364) |
| Prologue/epilogue | `turn_prologue.rs`, `turn_epilogue.rs` |
| Pre-dispatch | `turn_dispatch_policy.rs` |
| Completion | `completion_assessor.rs`, `edgecrab-types/src/harness.rs` |
| Failover | `failover.rs` |
| Compression | `compression.rs` |
| Spill | `crates/edgecrab-tools/src/artifact_spill.rs` |
| Guardrails | `crates/edgecrab-tools/src/tool_loop_guardrails.rs` |
| Goals | `goals/*`, `schema.sql` session_goals |
| Security | `crates/edgecrab-security/src/*` |
| MCP OAuth | `tools/mcp_client.rs` OAuthConfig · `cli/mcp_oauth.rs` |
| MCP register | `cli/mcp_register.rs` (014 WS-A) |
| Tool batch | `core/tool_batch.rs` (014 WS-B) |
| Edit diff (current) | `cli/edit_diff.rs`, `tool_display.rs`, `transcript.rs` |
| Lineage | `edgecrab-state/src/session_db.rs` parent_session_id |

### Grok Build TUI anchors (borrow mechanisms only)

| Concern | Path under `/Users/raphaelmansuy/Github/03-working/grok-build` |
|---------|----------------------------------------------------------------|
| Diff hunk builder | `crates/codegen/xai-grok-pager/src/diff.rs` |
| Edit tool block + progressive HL | `…/scrollback/blocks/tool/edit.rs` |
| Tool block sum type | `…/scrollback/blocks/tool/mod.rs` |
| Tool category stats | `…/tool_usage.rs` |
| Turn status row | `…/views/turn_status.rs` |
| Session hunk model | `crates/codegen/xai-hunk-tracker/` |
| Apply patch / search-replace tools | `crates/codegen/xai-grok-tools/src/implementations/` |
| SDK | `sdks/*`, `edgecrab-sdk*` |
| Gateway adapters | `edgecrab-gateway/src/{telegram,discord,...}.rs` |

---

## 4. Hermes anchors (high signal)

| Concern | Path under hermes-agent |
|---------|-------------------------|
| Loop | `agent/conversation_loop.py` |
| Prologue | `agent/turn_context.py` |
| Finalizer | `agent/turn_finalizer.py` |
| Executor | `agent/tool_executor.py` |
| Classifier | `agent/error_classifier.py` |
| Guardrails | `agent/tool_guardrails.py` |
| Spill | `tools/tool_result_storage.py` |
| Credential pool | `agent/credential_pool.py` |
| Curator / bg review | `agent/curator.py`, `background_review.py` |
| Compressor | `agent/context_compressor.py` |
| MCP OAuth | `tools/mcp_oauth.py`, `mcp_oauth_manager.py` |
| Platforms | `plugins/platforms/*` |
| Desktop / TUI | `apps/desktop`, `ui-tui`, `tui_gateway` |

---

## 5. External SOTA references

- Anthropic Engineering: *Effective harnesses for long-running agents* (2025-11) — multi-window progress, feature lists, evidence-based completion, clean session state.  
- AE1–AE10 rubric encoded in [000 §0](000-code-is-law.md).

---

## 6. Refresh protocol

1. Update **000** with `wc -l` / symbol diffs.  
2. Cascade scores to 011 then 012.  
3. Never “upgrade” a score without a path citation.
