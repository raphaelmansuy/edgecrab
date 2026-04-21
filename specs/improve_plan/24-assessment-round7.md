# Round 7 — Brutal Honest Assessment: Session Management, Prompt Directives, Prompt Pipeline

Cross-reference: [README.md](README.md) | [23-assessment-round6.md](23-assessment-round6.md)
Target crate: `edgecrab-core` (`prompt_builder.rs`, `conversation.rs`)

---

## 1. WHY This Round Exists

Rounds 1–6 fixed tool mechanics and loop safety. This round attacks the **prompt
pipeline and session management** — the layer that directly shapes what the LLM
believes it is, what tools it has, and how it should behave.

A bad prompt pipeline means:
- The model "describes" instead of "acts" (most common user complaint)
- The model picks wrong strategies for the provider family (GPT vs Gemini vs Anthropic)
- The system prompt is rebuilt every turn, blowing Anthropic prefix cache efficiency
- The skills cache evicts too aggressively, causing disk I/O spikes
- Injection scanner misses Unicode variants and regex-evasion patterns
- The model doesn't know its own session ID, model name, or provider — loses context

Hermes Agent and Claude Code both have explicit solutions for each of these. We must
match them.

---

## 2. Methodology

Code Is Law: every claim below is backed by a line reference.

```
Hermes ref: hermes-agent/agent/prompt_builder.py
EdgeCrab ref: crates/edgecrab-core/src/prompt_builder.rs
Hermes loop: hermes-agent/run_agent.py (_build_system_prompt, ~line 2908)
EdgeCrab loop: crates/edgecrab-core/src/conversation.rs (~line 734)
```

---

## 3. Gap Matrix

```
+---+-------+----------------------------------------------+----------+----------+
| # | FP    | Gap                                          | Hermes   | EdgeCrab |
+---+-------+----------------------------------------------+----------+----------+
| 1 | FP22  | No TOOL_USE_ENFORCEMENT_GUIDANCE             | YES ✅   | NO  ❌  |
|   |       | "describe not act" — most common failure     |          |          |
+---+-------+----------------------------------------------+----------+----------+
| 2 | FP23  | No model-specific guidance                   | YES ✅   | NO  ❌  |
|   |       | GPT/Codex: execution discipline XML block    |          |          |
|   |       | Gemini/Gemma: operational rules              |          |          |
|   |       | 'developer' role for GPT-5/Codex family      |          |          |
+---+-------+----------------------------------------------+----------+----------+
| 3 | FP24  | Skills cache: no LRU eviction, no per-key    | 2-layer  | 1-layer  |
|   |       | (Mutex<HashMap>, 60s TTL, same key always)   | LRU+disk | HashMap  |
|   |       | Cold-start hit miss; no platform keying      |          |          |
+---+-------+----------------------------------------------+----------+----------+
| 4 | FP25  | Injection scanner uses str.contains()        | regex    | str      |
|   |       | Misses: "IGNORE  PREVIOUS" (double space)    |          |          |
|   |       | "IgnorePreviousInstructions" (camelCase)     |          |          |
|   |       | Also missing: hidden-div, exfil-curl,        |          |          |
|   |       | translate-execute, cat-secrets patterns      |          |          |
+---+-------+----------------------------------------------+----------+----------+
| 5 | FP26  | Timestamp omits Session ID, Model, Provider | YES ✅   | NO  ❌  |
|   |       | Model can't self-report its session ID       |          |          |
|   |       | Hermes: "Session ID: X\nModel: Y\nProvider: Z"|          |          |
+---+-------+----------------------------------------------+----------+----------+
| 6 | FP27  | Skills prompt: plain text, no XML wrapper    | XML +    | plain    |
|   |       | Missing: <available_skills> block            | header   | text     |
|   |       | Missing: "## Skills (mandatory)" header      |          |          |
|   |       | Missing: "scan before replying" directive    |          |          |
+---+-------+----------------------------------------------+----------+----------+
| 7 | FP28  | Truncation marker is vague                   | YES ✅   | partial  |
|   |       | EdgeCrab: "... [N chars omitted] ..."        |          |          |
|   |       | Hermes: "[…truncated filename: kept N+M …    |          |          |
|   |       | Use file tools to read the full file.]"      |          |          |
+---+-------+----------------------------------------------+----------+----------+
```

---

## 4. First Principles

```
FP22: "Act Don't Describe" Is A Non-Negotiable Model Directive
  WHY: Models like GPT-4.1, Gemini 2.0 routinely produce responses
  that narrate intended actions without executing them. The system
  prompt is the only surface with enough authority to break this habit.
  Without TOOL_USE_ENFORCEMENT_GUIDANCE, every GPT/Gemini/Grok session
  is a coin-flip on whether the model acts or just talks.

FP23: Model Families Have Different Pathologies
  WHY: GPT-5 skips prerequisite lookups and declares "done" prematurely.
  Gemini uses relative paths when absolute paths are safer. Grok omits
  verification. One generic prompt cannot address all families.
  First principle: tailor the guidance to the model, not the reverse.

FP24: Cache Key Must Reflect Cache Invalidation Surface
  WHY: The current cache key is the home directory path. Two sessions
  with different platforms but same home will get the same cached skills
  — even when platform_disabled would filter differently.
  First principle: cache key = hash(inputs that affect output).

FP25: Security Scanners Must Match Attack Surface
  WHY: "IGNORE PREVIOUS" (double space) evades contains("ignore previous").
  "IgnorePreviousInstructions" (camelCase) evades it too.
  The injection surface is the ENTIRE CONTEXT FILE which can be controlled
  by an attacker. Regex with IGNORE_CASE is the minimum bar.
  First principle: security rules must handle adversarial input.

FP26: Session Context Enables Self-Aware Debugging
  WHY: When a session fails mid-conversation, the user needs to identify
  which session it was, which model was active, and which provider they
  were using. Without session ID + model in the system prompt, the model
  cannot surface this in error messages. It's also critical for Anthropic
  cache efficiency monitoring — the model can confirm which session's
  prompt is cached.

FP27: Skills Are Contracts, Not Suggestions
  WHY: Skills represent the agent's accumulated institutional knowledge.
  Injecting them as plain text buries them in the prompt noise.
  Hermes wraps them in <available_skills> XML with a "## Skills (mandatory)"
  header and a "scan before replying" directive — treating skills as
  a required preflight check, not an optional reference.
  First principle: critical directives need structural emphasis.

FP28: Truncation Marker Must Enable Recovery
  WHY: When a context file is truncated, the model must know it can
  recover the full content using a file tool. "... [N chars omitted] ..."
  gives a count but no recovery path. Hermes's marker includes the
  filename and tells the model exactly what tool to use.
  First principle: every truncation is a recoverable error.
```

---

## 5. Implementation Plan

### FP22 — TOOL_USE_ENFORCEMENT_GUIDANCE (prompt_builder.rs)

Port `TOOL_USE_ENFORCEMENT_GUIDANCE` constant from Hermes verbatim.

```
WHERE: prompt_builder.rs — new constant TOOL_USE_ENFORCEMENT_GUIDANCE
GATE: new method has_tool_use_enforcement_model(&self, model: &str) -> bool
     matches: gpt, codex, gemini, gemma, grok (case-insensitive)
INJECT: build() step after identity, before platform hint
        always-on: inject for all models (generic version)
        plus model-specific block when model matches

EDGE CASES:
  - model name is empty string → always inject generic version
  - model is "anthropic/claude-*" → skip model-specific block
  - model is None (build() has no model param yet) →
      extend build() signature to accept Option<&str> model
```

**build() signature change:**
```rust
// Before
pub fn build(&self, override_identity, cwd, memory_sections, skill_prompt) -> String

// After
pub fn build(&self, override_identity, cwd, memory_sections, skill_prompt,
             model: Option<&str>) -> String
```

All callers in `conversation.rs` pass `Some(&config.model)`.

---

### FP23 — Model-Specific Guidance (prompt_builder.rs)

Port `OPENAI_MODEL_EXECUTION_GUIDANCE` and `GOOGLE_MODEL_OPERATIONAL_GUIDANCE`.

```
WHERE: prompt_builder.rs — two new constants
GATE: model_guidance_for() fn → Option<&'static str>
     "gpt"|"codex" → OPENAI_MODEL_EXECUTION_GUIDANCE
     "gemini"|"gemma" → GOOGLE_MODEL_OPERATIONAL_GUIDANCE
     "grok" → generic TOOL_USE_ENFORCEMENT_GUIDANCE only
     "claude"|"anthropic" → None (model handles tool use natively)

INJECT: build() step 2.5 — after identity, after TOOL_USE_ENFORCEMENT_GUIDANCE

EDGE CASES:
  - "openrouter/openai/gpt-4o" — must match "gpt" in provider+model string
  - unknown model family → generic enforcement only, no crash
```

---

### FP24 — Skills Cache Key Fix (prompt_builder.rs)

```
WHERE: SkillsCacheEntry + SKILLS_CACHE + cache key type

CURRENT: key = PathBuf (home dir)
FIX: key = (PathBuf, String) where String = platform string

WHY platform in key: platform_disabled skills differ per platform.
Same home, different platform → different valid cache entries.

EDGE CASES:
  - key = (home, "cli") vs (home, "telegram") → independent entries
  - TTL still 60s (no change)
  - invalidate_skills_cache() invalidates all (correct behavior)
```

---

### FP25 — Regex Injection Scanner (prompt_builder.rs)

```
WHERE: scan_for_injection() + ThreatPattern + INJECTION_PATTERNS

CURRENT: plain str.contains() matching
FIX: use regex::Regex with IGNORE_CASE flag

NEW PATTERNS (ported from Hermes _CONTEXT_THREAT_PATTERNS):
  - r"<\s*div\s+style\s*=\s*[\"'][\s\S]*?display\s*:\s*none"  → "hidden_div"
  - r"translate\s+.*\s+into\s+.*\s+and\s+(execute|run|eval)"  → "translate_execute"
  - r"curl\s+[^\n]*\$\{?\w*(KEY|TOKEN|SECRET|PASSWORD|API)"   → "exfil_curl"
  - r"cat\s+[^\n]*(\.env|credentials|\.netrc|\.pgpass)"       → "read_secrets"

EXISTING PATTERNS: convert from contains() to regex

EDGE CASES:
  - Regex compile error → log error, fall back to contains() for that pattern
  - Very large files → regex is O(n) same as contains() for linear patterns
  - OnceLock<Vec<Regex>> for compile-once-reuse semantics

cargo add regex (already in edgecrab-core Cargo.toml? check first)
```

---

### FP26 — Rich Timestamp (prompt_builder.rs + conversation.rs)

```
WHERE: prompt_builder.rs build() step 3

CURRENT:
  "Current date/time: 2026-04-21 14:30:00 +0000"

FIX:
  "Current date/time: 2026-04-21 14:30:00 +0000
   Session ID: {session_id}
   Model: {model}
   Provider: {provider}"

INTERFACE CHANGE: build() accepts model: Option<&str> (already added in FP22)
  Add session_id: Option<&str> parameter

CALLER: conversation.rs passes Some(&conversation_session_id) and Some(&config.model)

PROVIDER EXTRACTION: split model "openrouter/gpt-4o" → provider="openrouter"
  fn extract_provider(model: &str) -> &str { model.split('/').next().unwrap_or(model) }

EDGE CASES:
  - no session_id yet → omit that line
  - model is empty → omit model/provider lines
```

---

### FP27 — Skills Prompt XML Format (prompt_builder.rs)

```
WHERE: build() step 17 — skill_prompt injection

CURRENT:
  push skill_prompt as-is

FIX: wrap in XML + mandatory header (ported from Hermes build_skills_system_prompt):
  ## Skills (mandatory)
  Before replying, scan these skills for a matching workflow.
  If a skill applies, follow it precisely.

  <available_skills>
  {skill_prompt}
  </available_skills>

EDGE CASES:
  - skill_prompt is empty → no wrapper injected (unchanged behavior)
  - skill_prompt already has <available_skills> → don't double-wrap
    (guard: !sp.contains("<available_skills>"))
```

---

### FP28 — Informative Truncation Marker (prompt_builder.rs)

```
WHERE: truncate_context_file()

CURRENT:
  "... [{omitted} characters omitted] ..."

FIX: include filename hint
  "[…truncated {name}: kept first {head_len}+last {tail_len} of {total} chars. \
   Use file tools to read the full file.]"

API CHANGE: truncate_context_file(text, name) — add name parameter
CALLERS: build() already has the filename from context_files loop
```

---

## 6. DRY / SOLID Checks

```
DRY:
  - TOOL_USE_ENFORCEMENT_MODELS list defined once as constant
  - model_guidance_for() is the single dispatch point for model-specific guidance
  - extract_provider() is a pure fn, reusable by conversation loop logging

SOLID:
  S: PromptBuilder is extended, not rewritten. Each new constant is isolated.
  O: New guidance types added via new constants — no existing constants modified.
  L: build() return type unchanged. All callers work with new optional params.
  I: build() parameters optional (Option<>) — callers not forced to supply all.
  D: Regex patterns compiled to OnceLock — scanner depends on pattern abstraction.
```

---

## 7. Test Plan

```
FP22: test_tool_use_enforcement_injected_for_gpt()
      test_tool_use_enforcement_not_injected_for_claude()
      test_tool_use_enforcement_injected_for_generic()

FP23: test_openai_guidance_for_gpt_model()
      test_google_guidance_for_gemini_model()
      test_no_model_guidance_for_claude()
      test_openrouter_model_string_matches_gpt()

FP24: test_skills_cache_keyed_by_platform()

FP25: test_injection_scanner_catches_double_space()
      test_injection_scanner_catches_camelcase()
      test_injection_scanner_catches_hidden_div()
      test_injection_scanner_catches_exfil_curl()

FP26: test_timestamp_contains_session_id()
      test_timestamp_contains_model()
      test_timestamp_omits_missing_session_id()

FP27: test_skills_wrapped_in_available_skills_xml()
      test_skills_mandatory_header_present()
      test_empty_skills_not_wrapped()

FP28: test_truncation_marker_contains_filename()
      test_truncation_marker_contains_char_counts()
```

---

## 8. Cross-References

| FP | Hermes source | EdgeCrab target |
|----|--------------|-----------------|
| FP22 | run_agent.py:TOOL_USE_ENFORCEMENT_GUIDANCE | prompt_builder.rs:TOOL_USE_ENFORCEMENT_GUIDANCE |
| FP23 | prompt_builder.py:OPENAI_MODEL_EXECUTION_GUIDANCE | prompt_builder.rs:OPENAI_MODEL_EXECUTION_GUIDANCE |
| FP24 | prompt_builder.py:build_skills_system_prompt (LRU cache key) | prompt_builder.rs:SKILLS_CACHE key type |
| FP25 | prompt_builder.py:_scan_context_content (regex) | prompt_builder.rs:scan_for_injection |
| FP26 | run_agent.py:_build_system_prompt timestamp block | prompt_builder.rs:build() step 3 |
| FP27 | prompt_builder.py:build_skills_system_prompt XML format | prompt_builder.rs:build() step 17 |
| FP28 | prompt_builder.py:_truncate_content() marker | prompt_builder.rs:truncate_context_file() |
