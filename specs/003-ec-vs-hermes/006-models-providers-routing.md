# 006 — Models, Providers & Routing

Who you can call, how credentials work, how requests get routed.

---

## Provider counts

| | Hermes | EdgeCrab |
|---|--------|----------|
| Registry entries | 30+ in `PROVIDER_REGISTRY` + auto-extended plugins | 19 in `model_catalog_default.yaml` |
| OAuth subscription routes | nous, openai-codex, xai-oauth, qwen-oauth, google-gemini-cli, copilot, minimax-oauth, … | claude-pro, chatgpt-pro, super-grok, copilot, nous (via proxy/auth) |
| API key providers | anthropic, openai-api, gemini, zai, deepseek, bedrock, … | anthropic, openai, google, bedrock, … |
| Local | ollama, lmstudio, custom | ollama, lmstudio |
| Aggregator | openrouter (first-class) | openrouter |

**Verdict:** **Hermes leads breadth** — especially Asia-market providers (Kimi CN, Stepfun, Alibaba, Tencent TokenHub, Xiaomi, Arcee, …).

---

## API modes (Hermes-specific)

Hermes `run_agent.py` supports three internal API shapes:

| Mode | Use case |
|------|----------|
| `chat_completions` | OpenAI-compatible |
| `codex_responses` | OpenAI Codex / xAI OAuth |
| `anthropic_messages` | Anthropic + Bedrock converse |

EdgeCrab routes via `edgequake-llm` with provider-specific adapters in `model_router.rs`.

**Verdict:** **≠** — same outcomes, different internal wiring.

---

## Routing features

| Feature | Hermes | EdgeCrab |
|---------|--------|----------|
| Cheap model routing | Heuristics + config | `model_router.rs` keyword/heuristic |
| Fallback provider chain | `fallback_providers` | `FallbackConfig` |
| Credential pools | Multi-key rotation (`auth.json`) | OAuth + env keys (pools thinner) |
| Model aliases | `model_aliases` | Catalog + CLI |
| Auxiliary models | vision, compression, goal_judge, **curator**, web_extract | vision, goal_judge, **shadow_judge**, image |
| `/transfer-model` with brief | Yes | Yes (`model_transfer.rs`) |
| `/fast` (priority tier) | Yes | Partial |
| OpenRouter Pareto router | `openrouter.min_coding_score` | Gap 029 |
| Bedrock guardrails | Yes | Via edgequake-llm |
| **Codex app-server runtime** | Optional subprocess | **No** |
| **Nous Portal Tool Gateway** | Managed tool routing | Separate proxy shape |

**Verdict:** **Hermes leads provider ops** (pools, fast mode, Codex runtime). **EdgeCrab leads cost guard UX** (`model_cost_guard.rs`).

---

## Subscription OAuth

| Target | Hermes CLI | EdgeCrab CLI |
|--------|------------|--------------|
| Nous Portal | `hermes auth add nous` | `edgecrab auth add nous` |
| xAI Grok | `hermes auth add xai-oauth` | `edgecrab auth add grok` |
| Claude Pro | OAuth paths | `edgecrab auth add claude-pro` |
| ChatGPT/Codex | `openai-codex` | `edgecrab auth add chatgpt-pro` |
| Copilot | `copilot` / device flow | `edgecrab auth login copilot` |
| Qwen OAuth | Yes | No |
| Google Gemini CLI OAuth | Yes | No |

Storage: both use `~/.hermes/auth.json` / `~/.edgecrab/auth.json` (Hermes-compatible shape in migrate).

**Verdict:** **Hermes leads OAuth breadth**; **parity on Western subscription triad** (Claude/OpenAI/Copilot/Grok).

---

## OpenAI-compatible proxy

Both expose local `/v1/chat/completions` for external tools (Aider, Continue, etc.).

| | Hermes (`hermes_cli/proxy/`) | EdgeCrab (`edgecrab-proxy/`) |
|---|------------------------------|------------------------------|
| Purpose | Subscription credential bridge | Mode A forward + Mode B provider bridge |
| Adapters | nous, xai | static, hermes_auth, nous_portal, xai_oauth |
| CLI | `hermes proxy start` | `edgecrab proxy start` |
| Distinct from gateway API | Yes | Yes |

**Verdict:** **Parity (B+)** — EdgeCrab spec 008 still marked OPEN in roadmap but code exists; verify deployment docs.

---

## Model catalog maintenance

| | Hermes | EdgeCrab |
|---|--------|----------|
| Source | `website/static/api/model-catalog.json` + plugins | Embedded YAML + user `models.yaml` merge |
| User overrides | config | `~/.edgecrab/models.yaml` |
| Pricing in catalog | Yes | `pricing.rs` |

**Verdict:** **Hermes leads update velocity** (web catalog generation); **EdgeCrab leads compile-time consistency**.

---

## Grades

| Dimension | Hermes | EdgeCrab |
|-----------|--------|----------|
| Provider breadth | A | B |
| OAuth/subscription | A | B+ |
| Routing/fallback | A | B+ |
| Local/offline | A | A |
| Proxy | B+ | B+ |
| Enterprise (Bedrock/Azure) | A | B+ |

Cross-ref: [001-gap-analysis 008/024/029](../001-gap-analysis-v14/999-roadmap.md)
