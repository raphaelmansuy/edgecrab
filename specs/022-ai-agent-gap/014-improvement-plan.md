# 014 — EdgeCrab Improvement Plan (Hermes Gap Close)

**Status:** **Wave-1 + Wave-2 implemented — assessed 2026-07-19**  
**Authority:** [000-code-is-law.md](000-code-is-law.md) · [001-first-principles.md](001-first-principles.md) · AE1–AE10  
**Hermes tree:** `/Users/raphaelmansuy/Github/03-working/hermes-agent`  
**Principles:** July 2026 agent engineering · **DRY** · **SOLID** · e2e-first · code is law

---

## Assessment (latest)

### Wave summary

| Wave | Theme | Status |
|------|-------|--------|
| **1** | MCP OAuth URL register + multi-tool plan + failover +2 | ✅ |
| **2** | Prologue preflight · VERIFY ordering · credential pool · MCP parallel_safe · JoinSet cap | ✅ |

### Shipped (code law)

| Item | Module | Hermes / SOTA anchor |
|------|--------|----------------------|
| MCP `add --url --auth oauth` | `cli/mcp_register.rs` | `hermes mcp add --url --auth oauth` |
| TUI `/mcp add` | `cli/app.rs` | same control plane |
| Strict MCP SSRF | `validate_mcp_http_url` | AE7 (stricter than preview) |
| Multi-tool plan | `core/tool_batch.rs` | `tool_executor` partition |
| JoinSet concurrency cap | Semaphore + `parallel_max_workers` | AE4 / thrash control |
| Failover +2 reasons | `failover.rs` | `upstream_rate_limit`, `ssl_cert_verification` |
| Preflight few-but-huge gate | `turn_prologue.rs` | `turn_context._should_run_preflight_estimate` |
| Compression progress helper | `turn_prologue::compression_made_progress` | `_compression_made_progress` |
| VERIFY mutation-after-test debt | `turn_epilogue::coding_verify_on_stop_debt` | Anthropic long-running harness evidence |
| Credential pool rotate | `credential_pool.rs` + `provider_call` hook | `credential_pool.mark_exhausted_and_rotate` |
| MCP read-only parallel_safe | `McpDynamicTool` | AE4 multi-tool |

### Still open (honest)

| Item | Severity | Notes |
|------|----------|-------|
| Hot-swap LLM provider after key rotate | S1 | Pool rotates + env hint; provider Arc not rebuilt mid-turn |
| Full JoinSet body extract from conversation | S2 | Plan + cap done; spawn still in loop |
| Full Hermes curator / learning graph | S3 | Product, not AE core |
| Gateway circuit breaker | S2 | Separate workstream |
| Desktop / plugin platform long-tail | ≠ | Intentional |

### Tests run (2026-07-19 wave-2)

```text
turn_prologue unit                 4 passed
credential_pool unit               2 passed
turn_epilogue wave2 VERIFY         2 passed
wave2_gap_close_e2e                4 passed
tool_batch unit + e2e              5+5 passed (wave-1)
mcp_register unit + e2e            6+5 passed (wave-1)
mcp_e2e existing                   6 passed (wave-1)
mcp_readonly parallel_safe         1 passed
```

### SOLID / DRY (wave-2)

| Rule | Application |
|------|-------------|
| **S** | Prologue helpers ≠ compression engine; pool ≠ provider factory; VERIFY debt ≠ assessor |
| **O** | New failover reasons without rewriting loop |
| **I** | `plan_tool_batch` / `should_run_preflight_estimate` pure functions |
| **D** | Provider_call depends on failover + pool interfaces, not Hermes layout |
| **DRY** | One VERIFY debt function; one parallel policy; one MCP register API |

---

## July 2026 AI engineering map (AE → work)

| AE | Practice (2026) | EdgeCrab action |
|----|-----------------|-----------------|
| AE1 Bounded autonomy | Hard-stop + concurrency caps | Keep hard-stop ON; JoinSet semaphore |
| AE2 Cross-window progress | Goals + lineage + preflight | Prologue metrics; existing goals |
| AE3 Completion = evidence | Verify after *last* mutation | Wave-2 VERIFY ordering |
| AE4 Tool truth | Parallel safe + spill | tool_batch + MCP readonly parallel |
| AE5 Cache-stable prompts | Unchanged (already strong) | — |
| AE6 Classify → recover | Taxonomy + pool rotate | +2 reasons + credential_pool |
| AE7 Mediated I/O | SSRF on MCP register | Strict loopback gate |
| AE8 Human sovereignty | Unchanged | steering / grants |
| AE9 Observability | Preflight metrics, rotate events | HookEvent `credential:rotated` |
| AE10 Extend | MCP URL register | WS-A |

---

## Operator surface (full)

```bash
# MCP HTTP + OAuth
edgecrab mcp add linear --url https://mcp.example.com/mcp --auth oauth \
  --token-url https://auth.example.com/token --client-id ...
edgecrab mcp login linear && edgecrab mcp test linear

# Bearer / stdio / local
edgecrab mcp add acme --url https://… --auth bearer --token "$TOKEN"
edgecrab mcp add github npx -y @modelcontextprotocol/server-github
edgecrab mcp add local --url http://127.0.0.1:3100/mcp --allow-loopback

# Multi-tool concurrency
export EDGECRAB_TOOL_PARALLEL_MAX=8

# Credential pool (comma-separated keys)
export EDGECRAB_API_KEY_POOL=sk-key1,sk-key2,sk-key3
# or EDGECRAB_API_KEY_POOL_OPENAI=...
```

TUI: `/mcp add <name> --url … [--auth oauth|bearer|none]`

---

## Architecture (as shipped)

```text
CLI/TUI ──► mcp_register ──► config.yaml + token store
                │
                └── is_safe_url + strict loopback gate

execute_loop
  turn_prologue (trackers + preflight metrics)
  tool_batch.plan_tool_batch ──► JoinSet (semaphore cap) + sequential
  turn_epilogue (VERIFY last-mutation debt)
  provider_call ──► failover ──► credential_pool.rotate on 429
```

---

## Follow-up backlog (wave-3)

1. **Provider hot-swap** after pool rotate (rebuild `Arc<dyn LLMProvider>` mid-session).  
2. **Move JoinSet spawn** into `tool_batch` behind a dispatch trait.  
3. **Gateway circuit breaker** (Hermes readiness/drain).  
4. **OIDC discovery** when MCP `--token-url` omitted.  
5. Optional: use `compression_made_progress` in anti-thrash path (helper ready).

---

## Acceptance checklist

### Wave-1
- [x] MCP URL + OAuth register CLI/TUI  
- [x] SSRF e2e  
- [x] Multi-tool plan e2e  
- [x] Failover +2 variants  

### Wave-2
- [x] Prologue preflight helpers + metrics  
- [x] VERIFY mutation-after-verify debt  
- [x] Credential pool + rate-limit rotate hook  
- [x] MCP readonly `parallel_safe`  
- [x] JoinSet concurrency semaphore  
- [x] wave2_gap_close_e2e  

### Wave-3 (open)
- [ ] Provider rebuild on rotate  
- [ ] Full prologue extract (system restore/todo hydrate as modules)  
- [ ] Gateway breaker  

---

## One-line summary

**EdgeCrab now matches Hermes on MCP URL/OAuth registration and multi-tool planning, and exceeds on typed VERIFY debt + hard-stop defaults; wave-2 closes preflight, evidence-after-mutation, credential pool, and parallel caps under DRY/SOLID with e2e.**
