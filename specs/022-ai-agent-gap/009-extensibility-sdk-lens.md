# 009 — Extensibility, MCP, Proxy & SDK Lens (Re-assessed)

**Authority:** [000 §11 AE10](000-code-is-law.md)  
**Date:** 2026-07-19

---

## 1. Extension channels (law)

| Channel | EdgeCrab | Hermes | Score |
|---------|----------|--------|-------|
| Skills | hub/guard/bundles | hub/guard/bundles + larger optional | = / H catalog |
| MCP client | stdio/HTTP + **OAuth multi-grant** (`OAuthGrantType`: auto, client_credentials, refresh, device, auth_code) | oauth + **MCPOAuthManager** depth | **H slight** |
| MCP serve | present | `mcp_serve.py` | = |
| Script/WASM/Lua plugins | `edgecrab-plugins` | Python native | ≠ |
| Platform plugins | mostly in-tree | `plugins/platforms` | **H** |
| Memory plugins | Honcho | 8+ | **H** |
| Model provider plugins | catalog + oauth | 20+ plugins | **H** |
| Context engine | trait + plugin engine | ABC + plugins | = |
| Lifecycle hooks | `lifecycle_hooks` + pre_verify | plugin hooks pre/post llm, transform output | = / H plugin density |
| OpenAI proxy | `edgecrab-proxy` clear Mode A/B | portal patterns | **EC** clarity |
| ACP | yes | yes | = |
| **Application SDKs** | Rust/Node/Python/WASM | weak as library | **EC** |

**Correction:** “EC MCP = static bearer only” is **false** — `mcp_client.rs` has full OAuthConfig grant types + token persistence. Hermes still leads on OAuth *manager* sophistication (disk-watch, 401 recovery flows).

---

## 2. SDK wedge (strategic AE10)

```text
edgecrab-sdk*          → Rust embed
sdks/nodejs-native     → NAPI
sdks/python            → PyO3
sdks/wasm              → edge/browser
npm-cli / pypi-cli     → distribution
```

Hermes optimizes *running Hermes*. EdgeCrab optimizes *shipping products that contain an agent*.

**PO rule:** never sacrifice SDK stability for long-tail plugin parity.

---

## 3. Extension policy (recommended)

1. Core tools (security-sensitive) stay Rust + security crate.  
2. Long-tail → **MCP** first.  
3. Procedures → **skills**.  
4. Trusted local automation → script plugins.  
5. Platform plugin ABI only if webhook/MCP insufficient.

---

## 4. Auth extensibility

| Target | EC | H |
|--------|----|---|
| Grok/Nous/Claude Pro/ChatGPT/Copilot | ✅ | ✅ |
| Credential pools | flag in failover; thin pool | **credential_pool 2459 LOC** |
| Qwen/Gemini CLI OAuth | gap | ✅ |
| Codex runtime | gap | ✅ |

---

## 5. Scorecard

| Dimension | Score |
|-----------|-------|
| Plugin breadth | **H** |
| Embed SDKs | **EC** |
| MCP OAuth presence | = |
| MCP OAuth manager depth | **H** |
| Proxy clarity | **EC** |
| Skills | = |
