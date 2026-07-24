# 002 — Product Owner Lens

**Persona goal:** Make EdgeCrab the best agent harness for Mac power users running **private, free, fast** local models via oMLX — without teaching them HTTP base URLs.

---

## 1. Jobs to be done

| Job | User | Success looks like |
|-----|------|--------------------|
| JTBD-1 | Mac developer | “I already run oMLX; EdgeCrab sees my models in `/model` and just works.” |
| JTBD-2 | Privacy-sensitive | Offline coding agent with no cloud key; cost stays $0. |
| JTBD-3 | Hermes migrant | Guide parity with Hermes “Local LLM on Mac” (llama.cpp + omlx). |
| JTBD-4 | Homelab operator | Doctor tells me if oMLX is down; setup picks oMLX without YAML surgery. |
| JTBD-5 | Multi-local user | Can switch `ollama/*` ↔ `lmstudio/*` ↔ `omlx/*` mid-session like any provider. |

---

## 2. Competitive position

| Product | Local Mac story |
|---------|-----------------|
| **Hermes** | Documented omlx + llama.cpp; long prefill timeouts for local endpoints |
| **Cursor / Claude Code** | Point at OpenAI-compatible base URL (generic) |
| **EdgeCrab today** | First-class Ollama + LM Studio; **oMLX invisible** |
| **EdgeCrab target** | First-class **named** oMLX with harness parity |

**Wedge:** Named provider + local agent harness (no dual GEN, tool ceilings, prefill prune) — not just “set base URL.”

---

## 3. User journeys (P0)

### Journey A — Cold start (new EdgeCrab + existing oMLX)

```text
1. User has oMLX menu bar server running (port 8000)
2. edgecrab setup → choose "oMLX (local MLX, free)"
3. Config writes model: omlx/<detected-or-default>
4. First chat streams; tools work; /cost shows $0
```

### Journey B — Existing user discovers oMLX

```text
1. /model → provider list includes omlx
2. Live rows populate from /v1/models when server up
3. Select omlx/qwen3-… → hot-swap works
4. doctor → "oMLX: reachable (N models)" or clear fix hint
```

### Journey C — Server down

```text
1. Selector still shows provider + static seed / last cache
2. Chat fails with actionable error (start oMLX / check OMLX_HOST)
3. No crash loop, no silent fallback to cloud
```

---

## 4. Product requirements (MoSCoW)

### Must (P0)

| ID | Requirement | Acceptance |
|----|-------------|------------|
| PO-M1 | Provider id `omlx` selectable as `omlx/<model>` | setup, CLI `--model`, `/model` |
| PO-M2 | Live model list when oMLX up | `/models omlx`, selector refresh |
| PO-M3 | Chat + tool-calling ReAct works | local harness e2e or manual script |
| PO-M4 | Zero token cost | `/cost` and pricing path |
| PO-M5 | Doctor probes oMLX | port or `OMLX_HOST` health |
| PO-M6 | Docs: local providers page mentions oMLX | site + feature-docs |
| PO-M7 | Optional `OMLX_API_KEY` | config/env only; no secret logging |

### Should (P1)

| ID | Requirement |
|----|-------------|
| PO-S1 | Vision models via oMLX when VLM loaded |
| PO-S2 | Embeddings if oMLX serves embedding models |
| PO-S3 | Profile model ids (`model:thinking`) preserved end-to-end |
| PO-S4 | Anthropic `/v1/messages` path for specialized clients |
| PO-S5 | `edgecrab doctor` suggests brew/DMG install link when down |

### Could (P2)

| ID | Requirement |
|----|-------------|
| PO-C1 | One-click “open oMLX admin” deep link |
| PO-C2 | Auto-detect oMLX vs LM Studio on shared ports (rare conflict) |
| PO-C3 | Gateway default model can be omlx when detected |

### Won’t (this initiative)

| ID | Non-requirement |
|----|------------------|
| PO-W1 | Ship oMLX binary |
| PO-W2 | Change global default model away from product default without separate decision |
| PO-W3 | Windows/Linux inference support claims |

---

## 5. Copy & naming (UX)

| Surface | Copy |
|---------|------|
| Setup row | `omlx` — **oMLX (local MLX on Apple Silicon, free)** |
| Doctor up | `oMLX: reachable at {host} ({n} models)` |
| Doctor down | `oMLX: not reachable at {host} — start oMLX (menu bar / \`omlx start\`)` |
| Help | `omlx — oMLX (local, OMLX_HOST, default http://127.0.0.1:8000)` |
| Auth column | `local, optional key` (not “no key” if API key mode exists) |

Avoid: “MLX server”, “Apple ML”, “omlx.ai” as the **id**. Id stays `omlx`.

---

## 6. Risks & mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Users confuse LM Studio vs oMLX (both can share model dirs) | Wrong port | Distinct labels + doctor ports 1234 vs 8000 |
| Tool calling fails on some MLX chat templates | “Broken agent” perception | Document tool-capable families; harness keeps non-stream tool turns |
| Long prefill looks hung | Abort spam | 600s timeout + progress tail labels like other locals |
| Profile ids with `:` break parsers | Selection fails | Treat model segment as opaque after first `/` |
| Port 8000 already used | False doctor positive/negative | Respect `OMLX_HOST` / `OMLX_PORT` |

---

## 7. Success metrics (product)

| Metric | How measured | Target |
|--------|--------------|--------|
| Setup completion with oMLX | manual / dogfood | Same steps as LM Studio |
| Support tickets “how do I use oMLX” | qualitative | Near zero after docs |
| Local provider NPS (Mac cohort) | dogfood | oMLX ranked equal to LM Studio for agent work |

---

## 8. Launch checklist (PO sign-off)

- [ ] README providers table includes oMLX  
- [ ] CHANGELOG entry  
- [ ] Site local docs updated  
- [ ] Setup lists oMLX  
- [ ] Doctor healthy/unhealthy paths verified on a Mac with oMLX  
- [ ] `/model` shows live models  
- [ ] One dogfood session: multi-tool coding task on oMLX  
