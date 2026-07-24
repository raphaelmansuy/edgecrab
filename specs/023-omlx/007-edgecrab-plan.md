# 007 — EdgeCrab Implementation Plan

**Depends on:** [006-edgequake-llm-plan.md](006-edgequake-llm-plan.md) merged or path-linked.  
**Principle:** Register oMLX at every identity boundary; collapse local-family checks to one source of truth.

---

## 1. Workstreams

| WS | Name | Crates |
|----|------|--------|
| WS-A | Local family DRY + policy | edgecrab-core, edgecrab-tools |
| WS-B | Catalog + discovery | edgecrab-core |
| WS-C | Pricing + transfer aliases | edgecrab-core |
| WS-D | CLI / TUI / doctor / setup | edgecrab-cli |
| WS-E | Docs / site / README | docs, site, root |
| WS-F | Tests | core, tools, cli |

Gateway/ACP inherit automatically if factory + catalog work.

---

## 2. WS-A — Local family DRY (do first inside EdgeCrab)

### Goal
Adding the next local server is **one line** in a const array + optional message copy.

### Tasks

1. Expand `is_local_inference_provider` to include `"omlx"`.  
2. Introduce `LOCAL_INFERENCE_PROVIDERS` (or equivalent) and route all family checks through `is_local_inference_provider`.  
3. Replace hardcoded `lmstudio | ollama` in:

| File | Function / area |
|------|-----------------|
| `mutation_turn_policy.rs` | local provider match |
| `registry.rs` | `annotate_llm_definitions_for_local_turn` gate |
| `tool_progress_tail.rs` | where family-wide behavior is intended |

4. Keep **provider-specific strings** as explicit match arms:

```rust
match provider_name {
    "lmstudio" => …,
    "ollama" => …,
    "omlx" => " oMLX may still be processing…",
    _ => generic_local,
}
```

5. Timeouts:

```rust
"omlx" => env OMLX_TIMEOUT_SECONDS.unwrap_or(DEFAULT_LOCAL_HTTP_TIMEOUT_SECS)
```

6. `prefers_nonstreaming_tool_turns`: include `"omlx"` (or derive from is_local if product wants all locals non-stream — today only lmstudio|ollama; **decide:** include omlx yes).

### Tests
- `is_local_inference_provider("omlx")`  
- harness activation with tools  
- timeout env precedence  

---

## 3. WS-B — Catalog + discovery

### Catalog YAML

Append after lmstudio block in `model_catalog_default.yaml`:

```yaml
  omlx:
    label: "oMLX (local MLX)"
    default_model: "default"
    models:
      - model: "default"
        context: 128000
        tier: standard
```

### model_catalog.rs

- Alias normalize: `o-mlx`, `o_mlx` → `omlx`  
- Tests: resolve_spec_lenient for `omlx/foo/bar` and `omlx/m:profile`

### model_discovery.rs

- `struct OmlxDiscovery;`  
- implement `ModelDiscoveryAdapter`  
- register in adapter list / discovery providers enumeration  
- tests: normalize alias; providers.contains("omlx")

Optional: call edgequake-llm discovery if already wired; else reuse `fetch_openai_compatible_models` like LM Studio (preferred for consistency).

---

## 4. WS-C — Pricing & secondary identity

| Change | File |
|--------|------|
| `ZERO_COST_PROVIDERS` += `"omlx"` | `pricing.rs` |
| vision normalize aliases | `vision_models.rs` |
| model_transfer alias test | `model_transfer.rs` |
| mutation / registry already covered in WS-A | — |

---

## 5. WS-D — CLI / TUI

### setup.rs

```rust
("omlx", "oMLX (local MLX on Apple Silicon, free)"),
// default_model:
"omlx" => "omlx/default",
```

### doctor.rs

```rust
let omlx_host = env OMLX_HOST/OMLX_BASE_URL unwrap default 127.0.0.1:8000
let omlx_up = check_local_host_port(omlx_host) // or check_local_port(8000)
```

Print status in local providers section next to Ollama/LM Studio.

### commands.rs

Help text row for omlx + `discovery_note("omlx")`.

### main.rs

Auth/key hint: `"local, optional key"`.

### app.rs

Where refresh filters `ollama || lmstudio`, add `|| omlx` **or** better: use `is_local_inference_provider` / discovery capability flag.

---

## 6. WS-E — Documentation

| Doc | Edit |
|-----|------|
| `docs/feature-docs/02-model-providers.md` | enumeration + discovery list |
| `site/src/content/docs/providers/local.md` | oMLX install + EdgeCrab connect |
| `README.md` | providers table |
| `CHANGELOG.md` | user-facing |
| Optional short guide | `docs/feature-docs/omlx.md` or section under local |

Content skeleton for site:

```markdown
## oMLX (Apple Silicon)

1. Install from https://omlx.ai or `brew install omlx`
2. Start server (`omlx start` / menu bar)
3. edgecrab setup → select oMLX
   or: edgecrab --model omlx/<id>
4. Env: OMLX_HOST (default http://127.0.0.1:8000), OMLX_API_KEY optional
```

---

## 7. WS-F — Tests (EdgeCrab)

| Test | Crate |
|------|-------|
| local policy omlx membership | edgecrab-core |
| catalog resolve | edgecrab-core |
| discovery registry contains omlx | edgecrab-core |
| pricing zero cost | edgecrab-core |
| tool_progress_tail omlx strings | edgecrab-tools |
| setup provider list contains omlx | edgecrab-cli (if unit-testable) |
| optional geometry e2e with NamedProvider omlx | edgecrab-core |

Live multi-tool ReAct: see [009](009-e2e-test-plan.md).

---

## 8. Config examples (user-facing)

```yaml
# ~/.edgecrab/config.yaml
model: omlx/qwen3-8b
```

```bash
export OMLX_HOST=http://127.0.0.1:8000
export OMLX_API_KEY=optional-secret
export OMLX_TIMEOUT_SECONDS=600
edgecrab --model omlx/qwen3-8b
```

---

## 9. Out of scope for EdgeCrab P0

- Proxy “enable omlx” (proxy is outbound cloud bridge)  
- Changing default model to omlx  
- Bundling oMLX  
- Anthropic-wire client mode  

---

## 10. Definition of done (EdgeCrab)

- [ ] `cargo test --workspace` green  
- [ ] `cargo clippy --workspace -- -D warnings` green  
- [ ] Manual Mac dogfood: setup → chat → tool → /cost $0  
- [ ] Doctor shows oMLX status  
- [ ] Docs updated  

---

## 11. Effort

| WS | Effort |
|----|--------|
| A | 0.5 day |
| B | 0.5 day |
| C | 0.25 day |
| D | 0.5 day |
| E | 0.25 day |
| F | 0.5 day |
| **Total** | **~2.5 days** after edgequake-llm ready |
