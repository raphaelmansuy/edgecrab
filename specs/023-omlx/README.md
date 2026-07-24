# 023 — Local Apple Silicon Providers

**Status:**  
- **oMLX:** Implemented (2026-07-24) — provider + discovery + `/endpoint` TUI  
- **MTPLX:** Implemented (2026-07-24) — same citizenship bar; plan in [012-mtplx-first-class.md](012-mtplx-first-class.md)  
- **Local prefix/KV:** Implemented (2026-07-24) — freeze tool wire schemas; [013-local-prefix-cache-july-2026.md](013-local-prefix-cache-july-2026.md)  
- **Wave D–F:** Implemented (2026-07-24) — **`llamacpp`**, **`vllm-mlx`**, **`mlx-lm`** full citizens via shared `LocalOpenAiProvider` ([014](014-apple-silicon-local-landscape.md), assessment [015](015-wave-def-implementation-assessment.md))

**Repos:** EdgeCrab + edgequake-llm (`/Users/raphaelmansuy/Github/03-working/edgequake-llm`)  
**Subjects:**  
- [oMLX](https://omlx.ai) — Apple Silicon MLX server (SSD KV, multi-model)  
- **MTPLX** (`com.youssofal.mtplx`) — native MTP speculative decoding on Apple Silicon  
- **llama-server** (`llamacpp`), **vLLM-MLX**, **mlx_lm.server** — thin OpenAI-compatible Mac servers

---

## One-screen summary

| | oMLX | MTPLX |
|--|------|-------|
| **Canonical id** | `omlx` | `mtplx` |
| **Default port** | **9050** (settings-aware) | **8000** docs / **settings.port** (e.g. 8002) |
| **Settings** | `~/.omlx/settings.json` | `~/Library/Application Support/MTPLX/settings.json` |
| **Models dir** | `~/.omlx/models` | `~/.mtplx/models` |
| **edgequake-llm** | `OmlxProvider` shipped | `MtplxProvider` shipped |
| **Local harness** | Yes | Same bar |
| **Zero cost** | Yes | Same bar |
| **Live discovery** | `GET /v1/models` | Same + optional FS cache fallback |
| **`/endpoint`** | Yes | Same row |

**Shared non-goals (P0):** reimplement MLX/MTP engines; invent a third HTTP stack; bundle the Mac apps.  
**Shared P0 protocol:** OpenAI-compatible chat + models list.

---

## Architecture (family, not one-off)

```text
┌─────────────────────┐   ┌─────────────────────┐
│ oMLX daemon :9050   │   │ MTPLX daemon :800x  │
└──────────┬──────────┘   └──────────┬──────────┘
           │  OpenAI /v1/*           │  OpenAI /v1/*
           └────────────┬────────────┘
                        ▼
              thin LocalOpenAi*Provider
              (edgequake-llm)
                        ▼
         catalog · discovery · local harness · TUI
              (EdgeCrab)
```

**DRY law:** MTPLX is the **second registration** of the local OpenAI-compatible Apple Silicon family — registration + settings resolve — not a copy-paste of the whole oMLX crate.

---

## Start here

| Goal | Doc |
|------|-----|
| oMLX evidence / laws / plan | [000](000-code-is-law.md) → [001](001-first-principles.md) → [010](010-implementation-plan.md) |
| **MTPLX full multi-lens plan** | **[012-mtplx-first-class.md](012-mtplx-first-class.md)** |
| **Other Mac servers (worth / how)** | **[014-apple-silicon-local-landscape.md](014-apple-silicon-local-landscape.md)** |
| Every model surface | [005](005-touchpoint-matrix.md) |
| Nav anchors | [011](011-cross-ref-index.md) |

---

## Document map

| # | Doc | Lens / purpose |
|---|-----|----------------|
| [000](000-code-is-law.md) | oMLX evidence ledger | Law / inventory |
| [001](001-first-principles.md) | Ontology + design laws | First principles |
| [002](002-product-owner-lens.md) | Personas (oMLX) | **Product Owner** |
| [003](003-ai-engineer-lens.md) | Harness physics (oMLX) | **AI Engineer** |
| [004](004-rust-expert-lens.md) | Types, factory (oMLX) | **Rust Expert** |
| [005](005-touchpoint-matrix.md) | All model surfaces | Synthesis |
| [006](006-edgequake-llm-plan.md) | Library layer (oMLX) | edgequake-llm |
| [007](007-edgecrab-plan.md) | Product layer (oMLX) | EdgeCrab |
| [008](008-edge-cases.md) | oMLX edge cases | Quality |
| [009](009-e2e-test-plan.md) | oMLX tests | Verification |
| [010](010-implementation-plan.md) | oMLX PR DAG | Execution |
| [011](011-cross-ref-index.md) | Anchors | Nav |
| **[012](012-mtplx-first-class.md)** | **MTPLX multi-lens + plan** | **Product · AI · Rust · e2e** |
| **[013](013-local-prefix-cache-july-2026.md)** | **Local prefix / KV cache (July 2026)** | **Agent harness · freeze · e2e** |
| **[014](014-apple-silicon-local-landscape.md)** | **Mac local landscape + decisions** | **Research · P0–P3 roadmap** |
| **[015](015-wave-def-implementation-assessment.md)** | **Wave D–F assessment** | **llamacpp · vllm-mlx · mlx-lm** |

---

## Canonical identity — oMLX (shipped)

| Concept | Value |
|---------|--------|
| Canonical provider id | `omlx` |
| Aliases | `o-mlx`, `o_mlx` |
| Model selector | `omlx/<model-id>` |
| Default base URL | `http://127.0.0.1:9050` |
| Env | `OMLX_HOST`, `OMLX_API_KEY`, `OMLX_TIMEOUT_SECONDS` |
| Settings auto-read | `~/.omlx/settings.json` |

## Canonical identity — MTPLX (shipped)

| Concept | Value |
|---------|--------|
| Canonical provider id | `mtplx` |
| Aliases | `mtp-lx`, `mtp_lx`, `mtpl-x` |
| Model selector | `mtplx/<model-id>` |

## Canonical identity — Wave D–F (shipped)

| Id | Default | Notes |
|----|---------|--------|
| `llamacpp` | `:8080` | llama-server Metal GGUF |
| `vllm-mlx` | `:8000` | continuous batching |
| `mlx-lm` | `:8080` | `mlx_lm.server` |

Shared shell: edgequake-llm `LocalOpenAiProvider`. Assessment: [015](015-wave-def-implementation-assessment.md).
| Default base URL | `http://127.0.0.1:8000` (CLI docs); **override via settings.port** |
| Env | `MTPLX_HOST`, `MTPLX_API_KEY`, `MTPLX_TIMEOUT_SECONDS` |
| Settings auto-read | `~/Library/Application Support/MTPLX/settings.json` |
| Models cache | `~/.mtplx/models` |
| Start server | `mtplx quickstart --host 127.0.0.1 --port <P>` |

Full locked decisions: [012 §10](012-mtplx-first-class.md).

---

## Citizenship bar (both)

A provider is first-class iff: **named · selectable · discoverable · runnable · policed · diagnosable · priced · overridable · documented · tested**.

See [001](001-first-principles.md) (oMLX) and [012 §7](012-mtplx-first-class.md) (MTPLX).

---

## External references

- oMLX: https://omlx.ai · https://github.com/jundot/omlx  
- MTPLX: macOS app `com.youssofal.mtplx` · CLI `mtplx` (app-managed venv)  
- Hermes Mac guide: `hermes-agent/website/docs/guides/local-llm-on-mac.md`  
- Hermes + MTPLX: `mtplx start hermes --port 18085`  
- EdgeCrab local harness: `crates/edgecrab-core/src/local_provider_policy.rs`  
- Shipped oMLX: `edgequake-llm/src/providers/omlx.rs`  
