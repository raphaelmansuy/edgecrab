# 000 — Code Is Law (Evidence Ledger)

**Status:** Authoritative for `specs/022-ai-agent-gap/`  
**Verified:** 2026-07-19 (full re-audit)  
**Trees:**

| Agent | Root |
|-------|------|
| EdgeCrab | `/Users/raphaelmansuy/Github/03-working/edgecrab` |
| Hermes | `/Users/raphaelmansuy/Github/03-working/hermes-agent` |

**Rule:** Every score in 001–012 is subordinate to this ledger. Narrative loses to paths + symbols + tests.

---

## 0. July 2026 agent-engineering rubric (first principles)

Industry consensus for long-horizon agents (Anthropic *Effective harnesses for long-running agents*, Nov 2025; production coding agents through mid-2026):

| # | Principle | What “good” looks like in code |
|---|-----------|--------------------------------|
| **AE1** | **Bounded autonomy** | Hard iteration/time/cost/cancel caps; thrash circuit breakers |
| **AE2** | **Incremental progress across windows** | Goals/feature contracts outside chat; progress artifacts; session lineage |
| **AE3** | **Completion = evidence, not vibe** | Typed outcomes; verify-on-stop; refuse self-declared “done” without proof |
| **AE4** | **Tool truth in context budget** | Spill large outputs; turn budgets; read-before-write after spill |
| **AE5** | **Stable prompt cache** | Stable identity vs dynamic memory/skills/time; ≤4 Anthropic breakpoints |
| **AE6** | **Classify → recover** | One error taxonomy → retry / compress / rotate / fallback / abort |
| **AE7** | **Mediated side effects** | Path/URL/shell/injection policy before I/O |
| **AE8** | **Human sovereignty** | Interrupt, steer, approve, clarify mid-flight |
| **AE9** | **Observable harness** | Structured outcomes, forensics, metrics, trajectory |
| **AE10** | **Extend without core bloat** | MCP/skills/plugins/SDK; not infinite core tools |

This ledger maps both codebases onto AE1–AE10 with **owning symbols**.

---

## 1. Harness topology

| Fact | EdgeCrab | Hermes |
|------|----------|--------|
| Public API | `Agent::chat` / `run_conversation` → `execute_loop` (`agent.rs`, `conversation.rs:364`) | `run_agent.py` → `conversation_loop.run_conversation` |
| Loop body size | `conversation.rs` **8051** LOC | `conversation_loop.py` **5562** LOC |
| Orchestrator shell | `agent.rs` **4671** LOC | `run_agent.py` **6247** LOC |
| Prologue | `turn_prologue.rs` **39** LOC — trackers only | `turn_context.py` **623** LOC — full setup |
| Epilogue | `turn_epilogue.rs` **734** LOC — real (verify, document latch) | `turn_finalizer.py` **546** LOC |
| Pre-dispatch policy | `turn_dispatch_policy::pre_dispatch_decision` (storm, port-shop, theater, spill-blind write, guardrails) | `tool_guardrails.ToolCallGuardrailController` |
| Tool dispatch | `process_response` JoinSet parallel + sequential (`conversation.rs` ~3799+) | `tool_executor.execute_tool_calls_{concurrent,sequential,segmented}` |
| Provider call | `provider_call.rs` **1583** LOC | `chat_completion_helpers.py` **3384** LOC |
| Failover | `failover.rs` **854** LOC, 21 `FailoverReason` | `error_classifier.py` **1621** LOC, 23 reasons |

**Law:** Hermes has better **module extraction** of prologue. EdgeCrab has richer **pre-dispatch mediation** and typed **epilogue**. Loop residual mass favors Hermes slightly; EC still concentrates physics in `conversation.rs`.

---

## 2. AE1 — Bounded autonomy

| Control | EdgeCrab | Hermes |
|---------|----------|--------|
| Iteration budget | `max_iterations` (default 90) in agent config | `IterationBudget` + `max_iterations` |
| Tool-loop hard stop default | **ON** — `HarnessConfig.guardrails_hard_stop: true`; `ToolLoopGuardrailConfig.hard_stop_enabled: true` | **OFF** — `hard_stop_enabled: bool = False` (`tool_guardrails.py`, `hermes_cli/config.py`) |
| Unit proof | `harness_loop_policy::guardrails_hard_stop_default_on` | docstring: hard stops “explicit opt-in” |
| Cancel | `Agent::interrupt` one-way latch; reset next turn | interrupt + gateway drain |
| Cost | `model_cost_guard` | `credits_tracker`, `usage_pricing`, `account_usage` |

**Score AE1 defaults: EC.** Hermes is operator-flexible by design.

---

## 3. AE2 — Cross-window progress

| Mechanism | EdgeCrab | Hermes |
|-----------|----------|--------|
| Persistent goals | `goals/` + SQLite `session_goals` / `session_subgoals`; `GoalStore`; `goal_set` / `subgoal_push`; Ralph continuation in `loop_manager.rs` | Goals CLI + state; feature lists in product patterns |
| Goal judge | `goal_judge.rs` | goal judge in kanban tools |
| Goal contract | `GoalContract` in `edgecrab-types` harness | weaker typing |
| Session lineage | **`parent_session_id`** on sessions (`session_db.rs`) | `parent_session_id` + compression root walk (`web_server.py`, CLI branch) |
| Progress files / initializer agent | not first-class Anthropic-style init agent | richer product flows; not identical to Anthropic demo |
| Learning reflection | **`run_learning_reflection` / bg spawn** after ≥5 tool calls (`conversation.rs`) | `background_review.py`, curator, learning_graph |

**Score AE2: ≈ parity with different shapes.** EC goals+contracts are strong; Hermes curator/background_review/learning_graph go further for *personal* agent longevity. Session lineage exists in **both** (prior “EC missing parent_session_id” was **false**).

---

## 4. AE3 — Completion truth

| Mechanism | EdgeCrab | Hermes |
|-----------|----------|--------|
| Typed state | `CompletionDecision` (8) + `ExitReason` (incl. GuardrailHalt, InvalidToolBudget, VerificationPending) | string `_turn_exit_reason` |
| Bundle | `RunOutcome` + `VerificationSummary` | dict fields in finalizer |
| Assessor | `assess_completion` (`completion_assessor.rs` 1143 LOC) | finalizer heuristics |
| verify_on_stop | default **true**; `turn_epilogue` coding debt → NeedsVerification | `verify_on_stop` config `"auto"`; `build_verify_on_stop_nudge` (soft) |
| Shadow judge | `shadow_judge::run_shadow_judge` mid-loop veto | absent |
| Document done latch | `task_class::document_done_latch_ready` (artifact evidence) | weaker specialty |
| Contract verify | `contract_verify::run_contract_verification` | `verification_evidence` |
| report_task_status | typed `ReportedTaskStatus` / `TaskStatusKind` | present as tools |

**Score AE3 types+defaults: EC.** Shared SOTA gap: neither forces end-to-end browser/user-path proof for all web tasks (Anthropic 2025 finding still applies).

---

## 5. AE4 — Tool truth / spill

| Mechanism | EdgeCrab | Hermes |
|-----------|----------|--------|
| Spill module | `edgecrab-tools/src/artifact_spill.rs` **913** LOC | `tools/tool_result_storage.py` **254** LOC |
| APIs | `maybe_spill`, `enforce_turn_budget`, `detect_spill_without_read`, web extract/search helpers | `maybe_persist_tool_result`, `enforce_turn_budget` |
| Config | `tools.result_spill` default true; thresholds + turn budget chars | budget_config + storage |
| Pre-dispatch | **blocks write/patch if spill unread** (`turn_dispatch_policy`) | guidance-oriented |
| Parallel tools | `JoinSet` + `can_parallelize_in_batch` | concurrent ThreadPool + segmented |

**Score AE4: EC slight / =.** Spill-blind-write block is an EC SOTA-aligned win. Hermes concurrent executor is mature and well-isolated in its own module.

---

## 6. AE5 — Prompt cache

| Mechanism | EdgeCrab | Hermes |
|-----------|----------|--------|
| Policy | `prompt_cache_policy.rs` (≤4 breakpoints; stable/dynamic) | `prompt_caching.py` `system_and_3` |
| Builder | `prompt_builder.rs` **4451** LOC | `prompt_builder.py` 2034 + `system_prompt.py` 592 |
| Stable zone law | documented in AGENTS.md; goals/steers inject into **messages**, not cached system | restore/build helpers in loop |

**Score AE5: =** (both invested; EC more explicit about cache-safe zones in product docs + code comments).

---

## 7. AE6 — Classify → recover

| | EdgeCrab | Hermes |
|-|----------|--------|
| Enum size | 21 | 23 |
| Hermes-only | — | `upstream_rate_limit`, `ssl_cert_verification` |
| Body depth | 854 LOC | 1621 LOC |
| Credential rotate flag | `ClassifiedError.should_rotate_credential` | full `credential_pool.py` **2459** LOC |
| Operator billing copy | thinner | dense Nous/billing helpers in `conversation_loop.py` |

**Score AE6: = taxonomy · H depth + pools + copy.**

---

## 8. AE7 — Mediated side effects

| Control | EdgeCrab | Hermes |
|---------|----------|--------|
| Crate boundary | `edgecrab-security` (path, SSRF, scan, injection, threats, secrets, sandbox, redact, website policy) | distributed `tools/*safety*`, `approval`, `tirith`, `secret_scope` |
| SSRF escape | `PreviewPolicy` + port allowlist + Once/Session grants | global `security.allow_private_urls` |
| Pre-dispatch anti-theater | port shopping, browser nav repeat, verification theater, document GUI thrash | guardrails focus on failure loops |
| Tool result delimiters | `threat_patterns::wrap_tool_result` / brainworm defense | present patterns |

**Score AE7 structure+defaults: EC.** Hermes vault/tirith/secret_sources depth: H slight.

---

## 9. AE8 — Human sovereignty

| | EdgeCrab | Hermes |
|-|----------|--------|
| Steering | `SteeringKind::{Hint,Redirect,Stop}` channel | interrupt, `/steer` |
| Clarify | multi-platform buttons | clarify tool + gateway |
| Approvals | `ApprovalMode` / session approvals; capability grants (preview Once/Session) | write_approval, slash_confirm |
| Goals mid-flight | `/goal` Ralph loop | goals |

**Score AE8: = / EC typed steer.**

---

## 10. AE9 — Observability

| | EdgeCrab | Hermes |
|-|----------|--------|
| Offline log doctor | `harness_analyzer::analyze_harness_log` | — |
| OTEL | `otel_export`, `otel_metrics` | plugins/observability (langfuse, nemo) |
| Trajectory | config flag + `build_trajectory` | mature trajectory tooling |
| Advisory | `harness_advisory` (theater, port shop) | display / status phrases |

**Score AE9 forensics: EC · product telemetry plugins: H.**

---

## 11. AE10 — Extensibility

| Channel | EdgeCrab | Hermes |
|---------|----------|--------|
| Skills | hub/guard/bundles | hub/guard/bundles + larger optional tree |
| MCP | stdio/HTTP + **OAuth grants** in `mcp_client.rs` (`OAuthGrantType`, token store) | `mcp_oauth.py` + `mcp_oauth_manager.py` (deeper provider) |
| Plugins | WASM/Lua/script + hermes bridge | native Python plugins (platforms, memory, models) |
| SDK | Rust/Node/Python/WASM | process-first |
| Context engine | `ContextEngine` trait + builtin + plugin | `ContextEngine` ABC + plugins |
| Lifecycle hooks | `lifecycle_hooks` scripts + pre_verify gate | plugin hooks `pre_llm_call`, `post_llm_call`, `transform_llm_output` |

**Score AE10 ecosystem velocity: H · embed SDK: EC · MCP OAuth: H slight (EC already has multi-grant types — not “bearer only”).**

---

## 12. Inventory (re-counted)

| Metric | EdgeCrab | Hermes |
|--------|----------|--------|
| Crates / top modules | 17 crates | monorepo agent+tools+gateway+apps |
| `CORE_TOOLS` | 56 | TOOLSETS multi |
| `inventory::submit!` (tools) | 85 | ~93 tool modules |
| `LSP_TOOLS` | **26** names | fewer first-class |
| Platform adapters | **17** `impl PlatformAdapter` | **20** plugin platforms + in-tree specialties |
| Slash commands | **88** unique | **82** CommandDef |
| Spill | 913 LOC artifact_spill | 254 LOC tool_result_storage |
| Hard-stop default | ON | OFF |
| FailoverReason | 21 | 23 |

---

## 13. Claim errata (this re-audit)

| Stale claim | Law |
|-------------|-----|
| EC lacks unified failover | **False** — `failover.rs` |
| EC missing parent_session_id | **False** — column + CRUD in `session_db` |
| EC spill “partial / thin” | **False** — 913 LOC + turn budget + spill-blind write block |
| EC no learning reflection | **False** — bg `run_learning_reflection` |
| EC MCP OAuth “static bearer only” | **Overstated** — OAuthConfig multi-grant in `mcp_client.rs` |
| EC no parallel tools | **False** — JoinSet parallel path |
| Prologue parity | **False** — EC stub vs H 623 LOC |
| Error classifier “H wins decisively” | **Overstated** — near-parity enum; H wins depth/pools/copy |

---

## 14. SOTA gap both share (July 2026)

1. **End-to-end product VERIFY** — unit/curl theater still possible without browser-as-user (Anthropic finding).
2. **Initializer agent pattern** — explicit first-session environment scaffolding (feature_list.json, init.sh, progress.txt) is not fully productized as a first-class mode in either (EC goals/contracts approximate; not identical).
3. **Loop file mass** — both still have multi-kLOC loop centers.
4. **Partial stream tool JSON** — both have recovery paths; still brittle across providers.

---

## 15. Refresh protocol

1. Re-run counts: `wc -l` on loop files; FailoverReason variants; PlatformAdapter impls.  
2. Diff this file before editing lens docs.  
3. Prefer symbols over prose when scoring.
