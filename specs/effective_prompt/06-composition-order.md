# Composition Order — Before and After

> The most impactful single change to EdgeCrab's prompt assembly.
> Shows the before/after ASCII diagrams, explains why the order matters,
> and quantifies the cache benefit.

---

## The Stable Prefix Law (recap)

Every token that Anthropic caches must have identical bytes at that position
across all subsequent API calls in the session. As soon as ANY byte changes,
ALL subsequent bytes are cache-invalidated.

```
  Token position:  0      N    N+1   M
                   │      │    │     │
  ─────────────────▼──────▼────▼─────▼──────────────────────────────
  TURN 1:          [stable prefix][VOLATILE][more content]
                    ████████████████░░░░░░░░░░░░░░░░░░░░░░░
                    ← cached →     ← NOT cached (N+1 onwards)→

  TURN 2:          [stable prefix][VOLATILE'][more content]
                    ████████████████░░░░░░░░░░░░░░░░░░░░░░░
                    ← cache hit →  ← NOT cached (N+1 onwards)→
```

The `VOLATILE` section is the datetime string. It changes every second. Moving
it to the END of the prompt maximizes the cacheable prefix length.

---

## BEFORE: Composition Order in EdgeCrab (Bug)

```
  Position  Section                         Stable?   Approx tokens
  ────────  ──────────────────────────────  ────────  ─────────────
  1         Identity (DEFAULT_IDENTITY)     YES       ≈ 1,800
  2         Platform hint                   YES       ≈   150
  3         Tool enforcement (non-Anthropic) YES      ≈   500
  4         Model-specific guidance         YES       ≈   400

  ╔══════════════════════════════════════════════════════════════╗
  ║  5   DATETIME ("Current date: 2025-01-15 14:23:07")  NO     ║  ← 35 tokens, changes each session
  ╚══════════════════════════════════════════════════════════════╝
            ↑ CACHE BOUNDARY HERE — only 2,850 tokens cached ↑

  6         Execution environment           NO        ≈   200
  7         Context files (AGENTS.md etc.)  NO        ≈ 2,000
  8         Memory sections                 NO        ≈   500
  9         session_search guidance         YES       ≈   500
  10        task_status guidance            YES       ≈   300
  11        skills guidance                 YES       ≈   200
  12        scheduling guidance             YES       ≈   300
  13        message_delivery guidance       YES       ≈   200
  14        moa guidance                    YES       ≈   150
  15        vision guidance                 YES       ≈   200
  16        lsp guidance                    YES       ≈   150
  17        code_editing guidance           YES       ≈   400
  17a       file_output enforcement         YES       ≈   200
  17b       research_task guidance          YES       ≈   300
  18        Skills prompt                   NO        ≈   600
  ─────────────────────────────────────────────────────────────
  Total cacheable prefix (before fix):     ≈ 2,850 tokens (22% of 13,000)
  Total prompt (typical):                  ≈ 13,000 tokens
```

---

## AFTER: Optimal Composition Order (Fix)

```
  Position  Section                         Stable?   Approx tokens
  ────────  ──────────────────────────────  ────────  ─────────────
  1         Identity (DEFAULT_IDENTITY)     YES       ≈ 1,800
  2         Platform hint                   YES       ≈   150
  3         Tool enforcement (non-Anthropic) YES      ≈   500
  4         Model-specific guidance         YES       ≈   400
  5         session_search guidance         YES       ≈   500
  6         task_status guidance            YES       ≈   300
  7         skills guidance                 YES       ≈   200
  8         scheduling guidance             YES       ≈   300
  9         message_delivery guidance       YES       ≈   200
  10        moa guidance                    YES       ≈   150
  11        vision guidance                 YES       ≈   200
  12        lsp guidance                    YES       ≈   150
  13        code_editing guidance           YES       ≈   400
  14        file_output enforcement         YES       ≈   200
  15        research_task guidance          YES       ≈   300
  ─────────────────────────────────────────────────────────────
  Total stable prefix:                     ≈ 5,850 tokens (45% of 13,000)

  ════════════════════════════════════════════════ DYNAMIC BOUNDARY ════

  16        DATETIME + Session ID + Model   NO        ≈    50  ← 35 tok, changes each session
  17        Execution environment           NO        ≈   200
  18        Context files (AGENTS.md etc.)  NO        ≈ 2,000
  19        Memory sections                 NO        ≈   500
  20        Personality addon               NO        ≈   100
  21        Skills prompt                   NO        ≈   600
  ─────────────────────────────────────────────────────────────
  Total dynamic section:                   ≈ 3,450 tokens (27% of 13,000)

  ════════════════════════════════════════════════════════════════════════
  Total cacheable prefix (after fix):      ≈ 5,850 tokens (45% of 13,000)
  Improvement:                             +3,000 tokens more cached = 2.05× improvement
```

---

## What This Means in Practice

```
                     BEFORE FIX               AFTER FIX
                  ┌────────────────┐        ┌────────────────┐
  Turn 1          │ 2,850 write    │        │ 5,850 write    │
  (cache write)   │ 10,150 normal  │        │  7,150 normal  │
                  └────────────────┘        └────────────────┘
                  
                  ┌────────────────┐        ┌────────────────┐
  Turn 2          │ 2,850 HIT ✓   │        │ 5,850 HIT ✓   │
  Turn 3          │ 2,850 HIT ✓   │        │ 5,850 HIT ✓   │
  ...             │ 10,150 normal  │        │  7,150 normal  │
  Turn N          └────────────────┘        └────────────────┘

Cost comparison (claude-sonnet, 50-turn session, $3.00/Mtok normal, $0.30/Mtok cached):

  BEFORE: Turn 1: 2850 × $3.75 + 10150 × $3.00 = $0.0107 + $0.0305 = $0.0412
          Turn 2-50: 2850 × $0.30 + 10150 × $3.00 = $0.0009 + $0.0305 = $0.0313
          Total: $0.0412 + 49 × $0.0313 = $0.0412 + $1.5337 = $1.575

  AFTER:  Turn 1: 5850 × $3.75 + 7150 × $3.00 = $0.0219 + $0.0215 = $0.0434
          Turn 2-50: 5850 × $0.30 + 7150 × $3.00 = $0.0018 + $0.0215 = $0.0232
          Total: $0.0434 + 49 × $0.0232 = $0.0434 + $1.1368 = $1.180

  Savings: 25% lower total prompt cost per session (without changing content).
  (The savings grow with session length — more turns = larger benefit.)
```

---

## How the Split Works in Code

```rust
// prompt_builder.rs — build_blocks() method

pub fn build_blocks(&self, ...) -> PromptBlocks {
    let mut stable = String::with_capacity(16_384);
    let mut dynamic = String::with_capacity(8_192);
    
    // ─── STABLE ZONE ─────────────────────────────────────────────
    // (All additions here are deterministic binary constants or
    //  deterministic functions of static session config)
    
    append_section(&mut stable, DEFAULT_IDENTITY);
    if let Some(hint) = self.platform_hint() {
        append_section(&mut stable, &hint);
    }
    if self.needs_tool_use_enforcement() {
        append_section(&mut stable, TOOL_USE_ENFORCEMENT_GUIDANCE);
    }
    if let Some(g) = self.model_specific_guidance() {
        append_section(&mut stable, g);
    }
    // ... all behavioral constants (has_tool gated) ...
    append_section(&mut stable, MEMORY_GUIDANCE);           // gated
    append_section(&mut stable, SESSION_SEARCH_GUIDANCE);   // gated
    // ... etc ...
    
    // ─── DYNAMIC ZONE ────────────────────────────────────────────
    // (All additions here change between sessions or conversations)
    
    // Datetime — changes every session
    if let Ok(dt) = Local::now().format("...").to_string() {
        append_section(&mut dynamic, &dt);
    }
    // Execution environment — changes with project
    if let Some(env) = self.build_execution_environment() {
        append_section(&mut dynamic, &env);
    }
    // Context files — changes with project
    for file in self.discover_context_files() {
        append_section(&mut dynamic, &file.content);
    }
    // Memory — grows over time
    for section in self.load_memory_sections() {
        append_section(&mut dynamic, &section);
    }
    // Skills — changes after skill installs
    if let Some(skills) = self.build_skills_prompt() {
        append_section(&mut dynamic, &skills);
    }
    
    PromptBlocks { stable, dynamic }
}

// Backward-compatible build() — unchanged call sites don't need updating
pub fn build(&self, ...) -> String {
    let blocks = self.build_blocks(...);
    blocks.combined()  // stable + "\n\n" + dynamic
}
```

---

## Verification

After the reordering, verify that:

1. No static content moved to the dynamic section
2. No dynamic content moved to the stable section  
3. Behavioral guidance still appears before context files
4. Model enforcement still appears before behavioral guidance
5. The skills prompt is last (longest, most variable)

```bash
# Build a debug prompt and check the DYNAMIC BOUNDARY marker position
cargo test -p edgecrab-core -- prompt_builder::tests --nocapture 2>&1 | \
    grep -A 5 "DYNAMIC BOUNDARY"
```

See [07-implementation.md](07-implementation.md) for the full implementation
with all the boundary markers and test assertions.
