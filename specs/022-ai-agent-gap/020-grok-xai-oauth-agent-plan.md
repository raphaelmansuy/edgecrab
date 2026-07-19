# 020 — Grok / xAI OAuth for the Agent (grok-build parity) · Plan

**Status:** Waves 1–4 implemented (2026-07-19)  
**Date:** 2026-07-19  
**Authority:** First principles · code is law · DRY · SOLID · e2e-first · exceptional TUI  
**References:**  
- grok-build: `crates/codegen/xai-grok-auth` (`AuthCredentialProvider`, 401 retry)  
- EdgeCrab today: `edgecrab-proxy/src/backend/xai/*`, `cli/grok_auth_tui.rs`, `cli/main.rs::prepare_super_grok_oauth_env`  
- LLM transport: `/Users/raphaelmansuy/Github/03-working/edgequake-llm` (`providers/xai.rs`, API-key only)  
- Spec 024 (Agents.md): subscription OAuth surface  

---

## 0. Intent

Make **any** EdgeCrab agent run that uses **xAI Grok models** work with **SuperGrok / X Premium+ OAuth** the same way operators experience in **grok-build** and EdgeCrab’s own `/login grok` — not only when the model string is `super-grok/...` or when `XAI_API_KEY` is a static console key.

**Success signal**

```text
edgecrab auth add grok          # or /login grok in TUI
edgecrab --model xai/grok-4.3   # OR super-grok/grok-4.3
# chat works, token refreshes before expiry, 401 once → refresh → retry
/status shows: xai · oauth · expires in …
```

---

## 1. First principles (strip brands)

| Principle | Meaning for this work |
|-----------|------------------------|
| **Auth ≠ transport** | Login + refresh + auth.json are **product** concerns. HTTP chat is **transport**. |
| **Bearer is bearer** | OAuth access token and API key both end as `Authorization: Bearer …` on `api.x.ai`. |
| **One credential source of truth** | `~/.edgecrab/auth.json` providers.`xai-oauth` (already Hermes-shaped). |
| **No process-env mutation as long-term design** | `set_var("XAI_API_KEY", …)` is a bridge; prefer explicit credential handoff. |
| **Refresh is a control-plane duty** | Expiry + 401 → one refresh → one retry (grok-build middleware). |
| **TUI is a surface, not a second auth stack** | Same login/finish APIs as CLI. |
| **No flaky heuristics** | Exact provider ids, JWT `exp` or stored `expires_at`, typed 401 — not “looks like token.” |

### 1.1 Problem decomposition (code law today)

```text
┌──────────────────┐     ┌─────────────────────────────┐
│ Login / TUI      │ ✅  │ edgecrab-proxy xai oauth    │
│ auth add grok    │     │ PKCE, refresh, auth.json    │
└────────┬─────────┘     └──────────────┬──────────────┘
         │                              │
         ▼                              ▼
┌──────────────────┐     ┌─────────────────────────────┐
│ Agent provider   │ ⚠️  │ Only super-grok/* injects   │
│ create_provider  │     │ OAuth into XAI_API_KEY      │
└────────┬─────────┘     └─────────────────────────────┘
         │
         ▼
┌──────────────────┐
│ edgequake-llm    │ ⚠️  API key env only; no 401 refresh;
│ XAIProvider      │    no OAuth header nuance
└──────────────────┘
```

| Path | Status | Gap |
|------|--------|-----|
| `edgecrab auth add grok` / `/login grok` | **Implemented** | Polish / unify messaging |
| Proxy Mode A `xai_oauth` adapter | **Implemented** | Separate from agent ReAct |
| `super-grok/*` model → OAuth env inject | **Partial** | Env mutation; not all `xai/*` |
| `xai/grok-*` without `XAI_API_KEY` | **Fails** | No OAuth fallback |
| Mid-session 401 / expiry | **Weak** | No transport-level refresh retry |
| `edgequake-llm` OAuth awareness | **None** | Only `XAI_API_KEY` |

---

## 2. Do we need to update `edgequake-llm` **first** before publication?

### Short answer

| Approach | Update edgequake-llm first? | Publish crates.io first? | Recommendation |
|----------|----------------------------|---------------------------|----------------|
| **A — EdgeCrab-only bridge** | **No** | **No** | Ship agent OAuth fallback by resolving auth.json → inject bearer before factory (extend today’s super-grok path to all `xai`/`grok`) |
| **B — SOLID transport (preferred)** | **Yes (local path dep)** | **Only after stable** | Add thin auth hook in edgequake-llm; EdgeCrab already path-deps `../edgequake-llm` |

**Workspace law (already true):**

```toml
# edgecrab/Cargo.toml
edgequake-llm = { path = "../edgequake-llm", version = "0.10.2", ... }
```

So development order is:

```text
1. Optional: improve edgequake-llm locally (no crates.io yet)
2. Implement EdgeCrab agent wiring against path dep
3. E2E green
4. THEN publish edgequake-llm if Approach B API changed
5. Pin version in edgecrab when ready to leave path dep
```

**You do NOT need to publish edgequake-llm before implementing.**  
You **do** need the **local** edgequake-llm tree only if Approach B (401 retry / explicit credential API) is chosen.

### Recommendation (first principles)

| Priority | Choice | Why |
|----------|--------|-----|
| **P0** | **Approach A+** in EdgeCrab | Unblocks operators immediately; reuses `resolve_xai_credentials_async`; no public API change |
| **P1** | **Approach B** in local edgequake-llm | Removes unsafe `set_var`; grok-build parity (401 → refresh → retry once); SOLID |
| **Publish** | After B is tested in EdgeCrab | Avoid shipping half-baked crates.io API |

**Approach A+ (minimum product fix):**  
For **any** provider resolved as `xai` / `grok` / `super-grok`, if `XAI_API_KEY` empty → OAuth resolve → set env (or better, call factory with explicit key). Same code path as `prepare_super_grok_oauth_env`, renamed to `prepare_xai_credentials`.

**Approach B (exceptional / non-flaky):**

```text
edgequake-llm:
  trait BearerTokenSource { async fn bearer(&self) -> Result<String>; }
  XAIProvider::with_token_source(source) 
  On HTTP 401: source.refresh_if_possible() once, retry once
  Optional: X-XAI-Token-Auth for OAuth (grok-build needs_token_auth_header)

edgecrab:
  impl BearerTokenSource for XaiAuthJsonSource { auth.json + refresh }
  create_provider_async wires source for xai/*
```

edgequake-llm must **not** know about `~/.edgecrab/auth.json` (D in SOLID).

---

## 3. SOLID / DRY ownership map

```text
┌─────────────────────────────────────────────────────────────────────┐
│ edgecrab-proxy (existing)                                           │
│   backend/xai/oauth_login.rs   PKCE start/finish                    │
│   backend/xai/refresh.rs       token refresh + resolve_*            │
│   backend/auth_file.rs         auth.json IO                         │
│   SOLE owner of OAuth wire protocol + auth.json shape               │
└───────────────────────────────┬─────────────────────────────────────┘
                                │ public resolve_xai_credentials_async
┌───────────────────────────────▼─────────────────────────────────────┐
│ edgecrab-core                                                       │
│   oauth/mod.rs                 aliases, is_xai_oauth_alias          │
│   oauth/runtime.rs             inject_* env (bridge) OR             │
│   NEW oauth/xai_agent.rs       prepare_xai_agent_credentials()      │
│   model_router / provider      call prepare before factory          │
│   NO duplicate PKCE                                             │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────┐
│ edgecrab-cli                                                        │
│   auth_cmd / grok_auth_tui     login UX only                        │
│   main create_provider_async   call core prepare (all xai paths)    │
│   /login grok · /status        exceptional TUI                      │
│   /model xai/…                 doctor-style auth hint if missing    │
└───────────────────────────────┬─────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────┐
│ edgequake-llm (optional Wave B)                                     │
│   providers/xai.rs             accept explicit key OR TokenSource   │
│   NO auth.json, NO PKCE, NO EdgeCrab imports                        │
└─────────────────────────────────────────────────────────────────────┘
```

| SOLID | Application |
|-------|-------------|
| **S** | Login UI ≠ refresh ≠ chat HTTP |
| **O** | New OAuth targets add alias + resolve, not rewrite XAIProvider |
| **L** | Any bearer source works with same chat client |
| **I** | TUI only needs start/finish/status; agent only needs resolve |
| **D** | edgequake-llm depends on abstract token, not auth.json |

| DRY | Forbidden |
|-----|-----------|
| Second PKCE implementation in core/cli | Reuse proxy login |
| Second auth.json parser | Reuse `auth_file` / resolve_xai |
| TUI reimplements CLI finish | Shared `auth_cmd` / proxy APIs |

---

## 4. Target operator UX (exceptional TUI)

### 4.1 Login (already good — polish)

| Surface | Behavior |
|---------|----------|
| `/login grok` | Open `GrokAuthTui`: Start → browser → Finish (clipboard / paste) |
| `edgecrab auth add grok` | CLI same flow; `--no-browser` / `--manual-paste` |
| Status bar / `/status` | `xai · SuperGrok OAuth · exp 42m` or `xai · API key` or `xai · not configured` |
| Failure | Actionable: next command, not stack dump |

### 4.2 Model selection

| Input | Resolution |
|-------|------------|
| `xai/grok-4.3` | Prefer static key; else OAuth bearer |
| `super-grok/grok-4.3` | Prefer OAuth; else key |
| `grok` alias in catalog | Canonical `xai` |

### 4.3 Runtime

| Event | UX |
|-------|-----|
| Token expiring &lt; 5m | Silent refresh before next LLM call |
| 401 from api.x.ai | One refresh + one retry; shelf notice `auth refreshed` |
| Refresh fails | Clear error + `/login grok` CTA; do not thrash |

---

## 5. Phased delivery

### Wave 0 — Inventory freeze (0.5 day)

- Document current commands, file paths, provider ids (`xai-oauth`).
- Confirm e2e already present: `edgecrab-proxy` grok/xai tests, `grok_auth` unit tests.
- Gap list signed off against this plan.

### Wave 1 — Agent path OAuth (P0, EdgeCrab only, **no edgequake-llm publish**)

| Step | Work | Owner |
|------|------|-------|
| 1.1 | Rename/generalize `prepare_super_grok_oauth_env` → `prepare_xai_credentials` | cli or core |
| 1.2 | Call for **all** of: `xai`, `grok`, `super-grok`, `super_grok` when key missing | `create_provider_async` |
| 1.3 | Prefer explicit key env over OAuth when set (deterministic precedence) | same |
| 1.4 | Gateway/ACP provider create paths same helper | gateway, acp if separate |
| 1.5 | `/status` + doctor: OAuth vs key vs missing | cli |
| 1.6 | Unit + e2e with mock auth.json + mock token endpoint | proxy/cli tests |

**Done when:** With only OAuth in auth.json (no `XAI_API_KEY`), `xai/grok-4.3` provider constructs and a mocked chat succeeds.

### Wave 2 — Refresh correctness (P0)

| Step | Work |
|------|------|
| 2.1 | Before each turn (or on provider create), resolve credentials with refresh-if-expired (already in `resolve_xai_credentials_async`) |
| 2.2 | On classified 401/403 auth failure in failover, attempt one OAuth rotate + rebuild provider **or** refresh env + retry |
| 2.3 | Telemetry: `auth:xai:refreshed` / `auth:xai:failed` |

### Wave 3 — edgequake-llm local improvement (P1, **local first, publish later**)

| Step | Work | Publish? |
|------|------|----------|
| 3.1 | `XAIProvider::with_api_key(key)` already exists; ensure factory can take key without env mutation | local |
| 3.2 | Optional `TokenSource` / callback for refresh (grok-build inspired) | local |
| 3.3 | Single 401 retry when TokenSource provided | local |
| 3.4 | EdgeCrab wires `XaiAuthJsonSource` | local path dep |
| 3.5 | Publish edgequake-llm **after** EdgeCrab CI green | crates.io |

**If 3.x is deferred:** Wave 1+2 still ship; env inject remains acceptable bridge.

### Wave 4 — Exceptional TUI polish (P1)

| Step | Work |
|------|------|
| 4.1 | `/login grok` parity with Copilot handoff (progress, cancel Esc) |
| 4.2 | Model picker badge: 🔑 key / 🪪 oauth / ⚠ missing |
| 4.3 | First chat failure if unauthenticated: one-shot overlay “Sign in to Grok?” |
| 4.4 | Proxy hub already documents grok — align wording with agent path |

### Wave 5 — Docs & non-goals

- Agents.md / user guide: OAuth vs API key  
- Explicit non-goal: do not vendor grok-build crates  
- Explicit non-goal: do not store OAuth in edgequake-llm  

---

## 6. Credential precedence (deterministic)

```text
1. Explicit process env XAI_API_KEY (non-empty)     → use as-is (static or pre-injected)
2. Else auth.json providers.xai-oauth (valid/refresh) → bearer + base URL
3. Else error with CTA: auth add grok | export XAI_API_KEY
```

Never invent tokens. Never fall back to mock for `xai/*` when user explicitly selected xai (mock only when no provider configured globally).

---

## 7. E2E / unit test plan

| ID | Layer | Assert |
|----|-------|--------|
| **GX-U1** | proxy | refresh_xai_tokens mock 200 → new access_token persisted |
| **GX-U2** | core/cli | prepare_xai with empty env + fixture auth.json → XAI_API_KEY set / provider builds |
| **GX-U3** | core/cli | prepare_xai with env key set → auth.json **not** required |
| **GX-U4** | core | `xai/…` and `super-grok/…` both call prepare |
| **GX-E1** | proxy e2e | existing mock OIDC + chat forward (extend if needed) |
| **GX-E2** | cli e2e | `auth add grok` finish with mock → auth.json shape |
| **GX-E3** | integration | create_provider_async(`xai/grok-4.3`) with temp EDGECRAB_HOME + auth.json |
| **GX-E4** | optional llm | 401 then 200 with TokenSource (Wave 3) |

Rules: **no real x.ai network** in CI; wiremock/mockito for OIDC + api.x.ai.

```bash
cargo test -p edgecrab-proxy xai
cargo test -p edgecrab-cli grok
cargo test -p edgecrab-core --test grok_xai_oauth_e2e   # new
# Wave 3:
cargo test -p edgequake-llm xai
```

---

## 8. Acceptance criteria

### Wave 1 done when

- [x] No `XAI_API_KEY` in shell; auth.json has valid/refreshable `xai-oauth` → prepare injects bearer  
- [x] All of `xai` / `grok` / `super-grok` call `prepare_xai_credentials`  
- [x] Missing both key and OAuth → clear multi-line CTA  
- [x] Unit: static key, oauth file, provider aliases  

### Wave 2 done when

- [x] `OAuthRefreshingProvider` wraps **xai** (401 → force OAuth refresh → rebuild)  
- [x] Static API-key mode skips OAuth force-refresh  
- [x] Re-login CTA on refresh failure  

### Wave 3 done when

- [x] edgequake-llm `XAIProvider::build_config` embeds literal `api_key` (no set_var race for key on config)  
- [x] `ProviderFactory::create_xai_with_bearer` for explicit handoff  
- [x] Env inject remains bridge (documented; publish edgequake-llm optional later)  

### Wave 4 done when

- [x] Doctor reports SuperGrok OAuth when auth.json present  
- [x] `xai_credential_status_line` for TUI/status  
- [x] x_search reads `xai-oauth` tokens path  
- [x] E2E: auth_grok_cli_e2e + xai_credentials unit tests

---

## 9. Risks

| Risk | Mitigation |
|------|------------|
| OAuth bearer rejected as “API key” by xAI | Already works for super-grok; if header needed, Wave 3 `X-XAI-Token-Auth` |
| Env mutation races multi-agent | Prefer per-provider credentials; Wave 3 |
| Publishing edgequake-llm too early | Path dep until e2e green |
| Duplicating PKCE in core | Forbidden — call proxy |
| TUI paste flaky | Keep clipboard + readline finish (already) |

---

## 10. Suggested sprint order

```text
Day 1     Wave 0 + Wave 1.1–1.4 (agent OAuth for all xai/*)
Day 2     Wave 1.5–1.6 + Wave 2 refresh/401
Day 3     Wave 4 TUI polish + docs
Day 4–5   Wave 3 local edgequake-llm (if capacity)
Later     Publish edgequake-llm + drop path-only caution
```

---

## 11. Explicit answers

### Q: Implement OAuth mode like grok-build?

**Yes — mostly already present for login/proxy.** Missing piece is **agent model routing** treating SuperGrok OAuth as a first-class credential for **all** xAI models, plus optional transport-level 401 refresh (grok-build `AuthCredentialProvider` pattern).

### Q: Update edgequake-llm locally first before publication?

| | |
|--|--|
| **For Wave 1–2 product fix** | **No** — EdgeCrab-only; path dep unchanged |
| **For Wave 3 (401 retry, no env inject)** | **Yes locally** — edit `../edgequake-llm`, test via path dep, **publish only after** EdgeCrab green |
| **Must publish before EdgeCrab merge?** | **No** |

---

## 12. One-line summary

**Reuse EdgeCrab’s existing xAI PKCE + auth.json; wire every `xai/*` agent path through one credential resolver (OAuth fallback); optionally add a thin TokenSource in local edgequake-llm for 401 retry — publish edgequake-llm only after that API is proven; TUI stays a single Grok login surface.**
