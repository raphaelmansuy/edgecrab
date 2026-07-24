# 008 — Edge Cases & Contracts

Each case: **trigger → expected behavior → test idea**.

---

## 1. Identity & parsing

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-ID-01 | `omlx/foo` | provider=`omlx`, model=`foo` | unit resolve |
| EC-ID-02 | `omlx/mlx-community/Qwen-4bit` | model=`mlx-community/Qwen-4bit` (not truncated) | unit lenient |
| EC-ID-03 | `omlx/qwen:thinking` | model id keeps `:` | unit + e2e model field |
| EC-ID-04 | `o-mlx/foo` / `OMLX/foo` | normalize to omlx | alias unit |
| EC-ID-05 | bare `omlx` | reject or require model — **match lmstudio behavior** | unit |
| EC-ID-06 | `omlx//` empty model | InvalidArgs / clear error | unit |

---

## 2. Connectivity

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-NET-01 | Nothing on :8000 | discovery empty; chat NetworkError; doctor red | unit mock + doctor |
| EC-NET-02 | Slow prefill > 60s | no client abort before timeout (600s default) | policy unit + manual |
| EC-NET-03 | Timeout fires | **no** automatic retry; stall suffix mentions oMLX | policy unit |
| EC-NET-04 | Stream then drop | cancel token; no second request | integration if feasible |
| EC-NET-05 | `OMLX_HOST=http://192.168.1.10:8000` | works (homelab); user responsibility | doc + optional |
| EC-NET-06 | Host includes `/v1` suffix | normalize; no `/v1/v1` | unit |
| EC-NET-07 | HTTPS local with self-signed | document unsupported or allow insecure only if other locals do | follow OpenAICompatible |

---

## 3. Auth

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-AUTH-01 | Server requires API key; env unset | 401 → actionable “set OMLX_API_KEY” | e2e or mock |
| EC-AUTH-02 | Key set | Bearer sent; no key in logs | redaction unit if pattern |
| EC-AUTH-03 | Key in config.yaml committed | warn in docs; prefer env | doc |

---

## 4. Models & multi-model server

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-MOD-01 | Zero models loaded | list empty; chat error model not found | e2e |
| EC-MOD-02 | Model unloaded mid-session (LRU) | next call fails clearly; user reloads in oMLX admin | manual |
| EC-MOD-03 | Profile id `m:p` | discovery lists; chat uses exact id | unit parse + e2e |
| EC-MOD-04 | Alias vs directory name | both accepted if server does | e2e optional |
| EC-MOD-05 | Static `omlx/default` when live empty | selector still usable; runtime may fail until real id | unit catalog |
| EC-MOD-06 | Concurrent models + EdgeCrab subagents | two models on same oMLX; continuous batching | manual stress |

---

## 5. Tools & harness

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-TOOL-01 | Tool-capable model | multi-round ReAct | e2e |
| EC-TOOL-02 | Model without tools template | failure or prose; no panic | e2e |
| EC-TOOL-03 | Huge tool result | spill / trim; loop continues | existing spill tests + omlx name |
| EC-TOOL-04 | tool_choice required local | options set on tool turns | policy unit |
| EC-TOOL-05 | Streaming + tools | non-stream preferred | policy unit |
| EC-TOOL-06 | Invalid JSON tool args from model | ToolError to loop; model can retry | existing loop |

---

## 6. Vision / multimodal (P1)

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-VIS-01 | Text-only model + image | fail or aux vision route | vision policy |
| EC-VIS-02 | VLM loaded + image | native multimodal if supported | e2e optional |
| EC-VIS-03 | Unknown vision capability | do not claim vision in UI | unit |

---

## 7. Platform & build

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-PLAT-01 | Linux CI build | compiles; no macos-only cfg on type | CI |
| EC-PLAT-02 | macOS without oMLX | doctor red; app usable with other providers | manual |
| EC-PLAT-03 | Termux / Android | omlx still listed but unreachable — OK | no special case |

---

## 8. Product / UX

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-UX-01 | Port 8000 is something else (not oMLX) | chat may fail with protocol error; doctor “reachable” may be false positive if only TCP probe | prefer HTTP GET /v1/models for doctor |
| EC-UX-02 | User confuses with LM Studio | labels show ports 1234 vs 8000 | copy review |
| EC-UX-03 | Mid-session `/model` to omlx | hot-swap works | integration |
| EC-UX-04 | Cost display | always $0 for omlx | unit pricing |
| EC-UX-05 | Offline static catalog only | omlx appears; live badge shows cache/static | TUI ADR |

**Doctor probe recommendation:** TCP open is weak (EC-UX-01). Prefer:

```text
GET {host}/v1/models  with 1–2s timeout → up if 200
```

Same pattern as health_check on provider.

---

## 9. Security edge cases

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-SEC-01 | Model id with path traversal junk | only sent as JSON string to server; no FS open | unit |
| EC-SEC-02 | Prompt injection in model name | display-safe in TUI | existing UI |
| EC-SEC-03 | SSRF via OMLX_HOST=http://169.254.169.254 | **out of scope** for local provider env (user config); document | doc |

---

## 10. Compression & long sessions

| ID | Trigger | Expected | Test |
|----|---------|----------|------|
| EC-CTX-01 | Context near limit | compress/prune still runs; server SSD cache independent | existing compress + local thresholds |
| EC-CTX-02 | After /compress | omlx still selected; system prompt cache rules unchanged | manual |
| EC-CTX-03 | Goals + omlx | goal injection still as user message | existing goal tests |

---

## 11. Failure contract summary (user-visible)

| Failure | Message intent |
|---------|----------------|
| Connection refused | Start oMLX (`omlx start` / menu bar); check `OMLX_HOST` |
| Timeout | Wait for server; avoid retry storm; increase `OMLX_TIMEOUT_SECONDS` |
| 401 | Set `OMLX_API_KEY` |
| Model not found | `/models omlx` or oMLX admin; load model |
| Tools unsupported | Pick tool-capable MLX model |

Never: silent switch to cloud OpenAI/Anthropic.
