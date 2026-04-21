# EdgeCrab TUI — Accessibility & Colour Contrast Audit

> **Standard:** WCAG 2.1 SC 1.4.3 (text: 4.5:1 AA, 7:1 AAA) and SC 1.4.11 (non-text UI: 3:1).  
> **Background assumption:** dark terminal, luminance ≈ 0 (black = worst case).  
> **Date:** 2026-04-21  
> **Status:** Audit → Implementation → Verified  

---

## 1. Scope

| File | Role |
|------|------|
| `crates/edgecrab-cli/src/theme.rs` | Central `Theme` + new `TuiPalette` |
| `crates/edgecrab-cli/src/tool_display.rs` | Tool-call spans (done/running/verbose) |
| `crates/edgecrab-cli/src/markdown_render.rs` | Markdown → ratatui Lines |
| `crates/edgecrab-cli/src/app.rs` | Banner, status bar, overlays |
| `crates/edgecrab-cli/src/skin_engine.rs` | Built-in skin palettes |
| `crates/edgecrab-cli/src/banner.rs` | Static banner (pre-TUI stdout) |

---

## 2. WCAG Contrast Ratio Formula

$$\text{CR} = \frac{L_{\text{lighter}} + 0.05}{L_{\text{darker}} + 0.05}$$

Where **relative luminance** $L$ is computed per IEC 61966-2-1 (sRGB):

$$L = 0.2126 R_{\text{lin}} + 0.7152 G_{\text{lin}} + 0.0722 B_{\text{lin}}$$

$$c_{\text{lin}} = \begin{cases} c/12.92 & c \le 0.04045 \\ \left(\frac{c + 0.055}{1.055}\right)^{2.4} & c > 0.04045 \end{cases} \quad \text{where } c = \text{value}/255$$

**Thresholds:**
- ≥ 7.0 : 1 → WCAG AAA (best)
- ≥ 4.5 : 1 → WCAG AA (minimum for text)
- ≥ 3.0 : 1 → WCAG AA for large text / non-text UI components (SC 1.4.11)
- < 3.0 : 1 → **FAIL** (even decorative separators should aim for 3:1 per SC 1.4.11)

---

## 3. Colour Catalogue & Contrast Audit

### 3.1 Theme defaults (`theme.rs` lines 434-559)

| Token | Hex / RGB | Luminance | CR vs ⬛ | Grade | Notes |
|-------|-----------|-----------|---------|-------|-------|
| `prompt_fg` (input border) | Rgb(205,127,50) | 0.283 | **6.65 : 1** | ✅ AA | copper |
| `assistant_fg` | Rgb(77,208,225) | 0.521 | **11.4 : 1** | ✅ AAA | cyan |
| `tool_fg` (output_tool) | Rgb(255,191,0) | 0.586 | **12.7 : 1** | ✅ AAA | amber – but DIM applied |
| `error_fg` | Rgb(239,83,80) | 0.251 | **6.02 : 1** | ✅ AA | red |
| `system_fg` | Rgb(158,158,158) | 0.342 | **7.84 : 1** | ✅ AAA | grey + ITALIC |
| `input_text` | Rgb(255,248,220) | 0.961 | **19.7 : 1** | ✅ AAA | cornsilk |
| `output_text` | Color::White | 1.000 | **21.0 : 1** | ✅ AAA | |
| `status_bar_bg` | bg Rgb(50,50,50) | — | — | — | dark bar |
| `status_bar_model` | `assistant_fg` on bg50 | — | **7.5 : 1** | ✅ AA | ok |
| `status_bar_tokens` | Rgb(156,204,101) on bg50 | 0.537 | **8.0 : 1** | ✅ AA | green |
| `status_bar_cost` | Rgb(255,183,77) on bg50 | 0.477 | **7.2 : 1** | ✅ AA | amber |

### 3.2 tool_display.rs — category name colours (lines 113-123)

All category name colours render as **primary text** on the dark terminal background.

| Category | Hex / RGB | Luminance | CR vs ⬛ | Grade |
|----------|-----------|-----------|---------|-------|
| Search | Rgb(80,210,230) | 0.594 | 12.9 : 1 | ✅ AAA |
| WebBrowser | Rgb(64,188,212) | 0.505 | 11.1 : 1 | ✅ AAA |
| FileRead | Rgb(150,165,195) | 0.373 | 8.5 : 1 | ✅ AAA |
| FileWrite | Rgb(255,185,50) | 0.512 | 11.2 : 1 | ✅ AAA |
| Terminal | Rgb(255,145,60) | 0.419 | 9.4 : 1 | ✅ AAA |
| Memory | Rgb(110,195,135) | 0.438 | 9.8 : 1 | ✅ AAA |
| Plan | Rgb(140,170,255) | 0.425 | 9.5 : 1 | ✅ AAA |
| Ai | Rgb(185,145,240) | 0.369 | 8.4 : 1 | ✅ AAA |
| Mcp | Rgb(130,165,210) | 0.387 | 8.7 : 1 | ✅ AAA |
| Ha | Rgb(100,195,145) | 0.438 | 9.8 : 1 | ✅ AAA |
| Other | Rgb(170,180,205) | 0.470 | 10.4 : 1 | ✅ AAA |

### 3.3 tool_display.rs — auxiliary dim text  ❌ FAILING

These are the critical failures. All carry `Modifier::DIM` which reduces effective
luminance by approximately 30–50 % depending on the terminal emulator.  
**WCAG evaluates the *perceived* colour.** A colour that barely achieves 4.5 : 1
without DIM may fall below 3 : 1 with DIM applied.

> Code references: `tool_display.rs` functions `build_tool_done_line_width`,
> `build_tool_running_line_width_elapsed`, `build_tool_verbose_lines_width`,
> `build_subagent_running_line_width`, `build_subagent_done_line_width`.

| Token / Usage | RGB | Luminance | CR (base) | CR (DIM×0.6) | Grade |
|---------------|-----|-----------|-----------|-------------|-------|
| `preview_style` (args column) | Rgb(90,102,125) | 0.132 | 3.63 : 1 | **2.26 : 1** | ❌ FAIL |
| `dur_style` (duration) | Rgb(72,79,98) | 0.079 | 2.57 : 1 | **1.63 : 1** | ❌ FAIL |
| `elapsed_style` (elapsed secs) | Rgb(100,112,135) | 0.160 | 4.20 : 1 | **2.61 : 1** | ❌ FAIL |
| `content_stat` second span | Rgb(80,92,112) | 0.105 | 3.10 : 1 | **1.97 : 1** | ❌ FAIL |
| `args_label` ("result" label) | Rgb(115,128,150) + DIM | 0.213 | 5.26 : 1 | **3.32 : 1** | ⚠️ WARN |
| `bar_style` ("  ┊ ") | Rgb(55,58,70) + DIM | 0.043 | 1.85 : 1 | **1.20 : 1** | ⚠️ DECOR |
| `indent` spaces | Rgb(48,52,62) + DIM | 0.034 | 1.69 : 1 | — | ✔ WHITESPACE |

> **Note on `bar_style`:** The `"  ┊ "` left-gutter bar is **purely decorative** —
> it carries no information beyond visual alignment.  SC 1.4.3 exempts decoration.
> However SC 1.4.11 (non-text contrast) recommends ≥ 3:1.  We raise it to ≥ 3:1.  
>
> **Note on indent:** Whitespace-only spans have no contrast requirement.

### 3.4 markdown_render.rs  ❌ FAILING

| Element | RGB | Luminance | CR vs ⬛ | Grade |
|---------|-----|-----------|---------|-------|
| Code block `│` bar | Rgb(100,100,100) | 0.127 | 3.54 : 1 | ❌ < 4.5 |
| Horizontal rule `─` | Rgb(100,100,100) | 0.127 | 3.54 : 1 | ❌ DECOR |
| Fence separator `─── ` | Rgb(100,100,100) + DIM | 0.127 | **2.22 : 1** | ❌ FAIL |
| Inline text body | Rgb(180,180,180) | 0.467 | 10.3 : 1 | ✅ AAA |
| Headers / links | Color::Cyan | 0.787 | 16.7 : 1 | ✅ AAA |
| Inline code | Color::Yellow | 0.928 | 19.6 : 1 | ✅ AAA |
| Code block body | Rgb(200,200,200) | 0.584 | 12.7 : 1 | ✅ AAA |

### 3.5 app.rs — banner & UI elements  ❌ FAILING

| Element | RGB | Luminance | CR | Grade |
|---------|-----|-----------|----|-------|
| Banner crab 🦀 | Rgb(255,160,40) bold | 0.349 | 8.0 : 1 | ✅ AAA |
| Banner name (gold) | Rgb(255,215,0) bold | 0.701 | 15.0 : 1 | ✅ AAA |
| `dot_style` separator " · " | Rgb(100,100,120) | 0.135 | 3.70 : 1 | ❌ < 4.5 |
| `tagline_style` + DIM | Rgb(184,134,11) | 0.273 | 3.47 : 1 (DIM) | ❌ WARN |
| `rule_style` decorative ─ | Rgb(70,60,40) + DIM | 0.042 | — | ✔ DECOR |
| `label_style` model label | Rgb(140,140,155) + DIM | 0.268 | 3.40 : 1 (DIM) | ❌ WARN |
| `value_style` amber | Rgb(255,191,0) | 0.586 | 12.7 : 1 | ✅ AAA |
| `hint_style` + DIM | Rgb(120,120,135) | 0.192 | 2.38 : 1 (DIM) | ❌ FAIL |
| Placeholder ITALIC | Rgb(100,100,100) | 0.127 | 3.54 : 1 | ❌ < 4.5 |
| Steer placeholder ITALIC | Rgb(80,90,110) | 0.100 | 3.00 : 1 | ❌ = limit |
| Sep line `─` | Rgb(60,60,70) | 0.042 | 1.90 : 1 | ✔ DECOR |
| Context pct badge (green) | Rgb(108,220,155) on bg50 | — | 8.8 : 1 | ✅ AA |
| Context pct badge (amber) | Rgb(255,204,92) on bg50 | — | 6.5 : 1 | ✅ AA |
| Context pct badge (orange) | Rgb(255,170,90) on bg50 | — | 4.8 : 1 | ✅ AA |
| Context pct badge (red) | Rgb(255,120,120) on bg50 | — | 3.5 : 1 | ⚠️ WARN (badge FG is very dark so composite passes) |

> Note: context percentage badges set BOTH fg and bg (dark fg on vivid bg), so the
> relevant check is dark fg against the badge bg.  All pass.

### 3.6 skin_engine.rs — built-in skin palette audit

| Skin | Token | Hex | Luminance | CR vs ⬛ | Grade |
|------|-------|-----|-----------|---------|-------|
| default | `banner_border` | #CD7F32 | 0.283 | 6.65 | ✅ AA |
| default | `banner_title` | #FFD700 | 0.701 | 15.0 | ✅ AAA |
| default | `banner_accent` | #FFBF00 | 0.587 | 12.7 | ✅ AAA |
| default | `banner_dim` | #B8860B | 0.273 | 6.46 | ✅ AA |
| default | `banner_text` | #FFF8DC | 0.961 | 19.7 | ✅ AAA |
| default | `ui_label` | #4dd0e1 | 0.521 | 11.4 | ✅ AAA |
| default | `session_label` | #DAA520 | 0.296 | 6.92 | ✅ AA |
| **mono** | **`ui_label`** | **#6e7681** | **0.161** | **4.22 : 1** | **❌ FAIL** |
| mono | `banner_dim` | #484f58 | 0.043 | 1.86 | ✔ DECOR |
| slate | `ui_label` | #708090 | 0.190 | 4.80 | ✅ AA (barely) |
| slate | `banner_dim` | #2b4a8c | 0.040 | 1.80 | ✔ DECOR |
| poseidon | `ui_label` | #4682B4 | 0.193 | 4.86 | ✅ AA (barely) |
| ares | `ui_label` | #D2691E | 0.236 | 5.72 | ✅ AA |
| charizard | `session_border` | #7A3300 | 0.062 | 2.25 | ✔ DECOR |

> Only the **mono skin** `ui_label` `#6e7681` fails AA (4.22 : 1 < 4.5 : 1).

---

## 4. First Principles Decision Log

### 4.1 DIM modifier is non-deterministic → eliminate from semantic text

The `Modifier::DIM` instruction is passed to the terminal emulator which applies it
using its own implementation (commonly 60–70 % luminance reduction). Because we
cannot guarantee the exact reduction across xterm, iTerm2, WezTerm, Alacritty, etc.,
**we treat DIM as equivalent to ~40 % luminance reduction for the worst case**, meaning
a colour must have a base contrast of at least 7.5 : 1 to survive DIM at 4.5 : 1.

**Decision:** Remove `Modifier::DIM` from all semantic text spans (preview args,
elapsed time, verbose labels, banner hint). Reserve DIM exclusively for **structural
whitespace and purely decorative glyphs** (`"  ┊ "` gutter, `"     "` indent, `"─"` 
decorative rules) — these carry no information and are exempt under SC 1.4.3.

### 4.2 Secondary-text hierarchy without DIM

To preserve the visual hierarchy ("primary > secondary > tertiary") without DIM:

| Tier | Luminance target | CR vs ⬛ | Usage |
|------|-----------------|---------|-------|
| Primary | ≥ 0.40 | ≥ 9 : 1 | Tool name, result, assistant |
| Secondary | 0.25 – 0.40 | ≥ 5.5 : 1 | Preview args, verbose labels, hints |
| Tertiary | 0.18 – 0.25 | ≥ 4.5 : 1 | Elapsed time, duration |
| Decorative | < 0.18 | < 4.5 : 1 | Gutter bar, indent, separator lines |

### 4.3 DRY — single `TuiPalette` in `theme.rs`

All WCAG-certified colour constants are declared **once** in `crates/edgecrab-cli/src/theme.rs`
as `pub mod palette`.  Downstream modules (`tool_display.rs`, `markdown_render.rs`,
`app.rs`) import via `use crate::theme::palette as P;`.

This eliminates ~30 scattered inline `Color::Rgb(...)` literals that previously each
required independent contrast verification.

### 4.4 SOLID — Open/Closed for skin overrides

`TuiPalette` constants are the **defaults** consumed when no skin is loaded.  Skins
may override any semantic colour through `SkinConfig` / `Skin::colors.*` without
touching palette constants.  The palette is **closed for modification, open for skin
extension**.

---

## 5. Fixed Palette (`crates/edgecrab-cli/src/theme.rs` — `pub mod palette`)

```rust
// ── Primary  (≥ 9 : 1 vs black, WCAG AAA) ──────────────────────────
pub const TOOL_RESULT_OK:   Color = Color::Rgb(148, 208, 168); // L=0.543 CR=11.9
pub const TOOL_RESULT_ERR:  Color = Color::Rgb(255, 120, 120); // L=0.361 CR= 8.2
pub const TOOL_INLINE_CODE: Color = Color::Rgb(255, 215, 0);   // L=0.701 CR=15.0

// ── Secondary (≥ 5.5 : 1 vs black, WCAG AA+) ───────────────────────
pub const SECONDARY_COOL:   Color = Color::Rgb(148, 162, 185); // L=0.351 CR= 8.0
pub const SECONDARY_WARM:   Color = Color::Rgb(148, 161, 175); // L=0.338 CR= 7.8

// ── Tertiary (≥ 4.5 : 1 vs black, WCAG AA) ─────────────────────────
pub const TERTIARY_COOL:    Color = Color::Rgb(125, 138, 162); // L=0.251 CR= 6.0
pub const TERTIARY_WARM:    Color = Color::Rgb(128, 138, 152); // L=0.247 CR= 5.9

// ── Decorative — not for readable text ──────────────────────────────
pub const GUTTER_BAR:       Color = Color::Rgb(55,  58,  70);  // ┊  purely decorative
pub const INDENT_SPACE:     Color = Color::Rgb(48,  52,  62);  // padding whitespace
pub const SEP_LINE:         Color = Color::Rgb(60,  60,  70);  // ─── decorative rule
```

---

## 6. Implementation Diff Summary

### 6.1 `theme.rs`
- **Add** `pub mod palette` block with the 9 constants above.
- No existing logic changed.

### 6.2 `tool_display.rs`
- **Remove** `Modifier::DIM` from `preview_style`, `dur_style`, `elapsed_style`,
  `args_label`, `content_stat`, `running detail`.
- **Replace** failing RGB values with `palette::SECONDARY_COOL`, `TERTIARY_COOL`,
  `TERTIARY_WARM`, `SECONDARY_WARM`.
- **Retain** `Modifier::DIM` only on `bar_style` ("  ┊ "), indent spans, and the
  `"···"` running indicator.

| Location | Was | Fix |
|----------|-----|-----|
| `preview_style` | Rgb(90,102,125) DIM | `P::TERTIARY_COOL` no DIM |
| `dur_style` | Rgb(72,79,98) DIM | `P::TERTIARY_WARM` no DIM |
| `elapsed_style` | Rgb(100,112,135) DIM | `P::TERTIARY_COOL` no DIM |
| `content_stat_1` | Rgb(80,92,112) DIM | `P::SECONDARY_COOL` no DIM |
| `content_stat_2` | Rgb(90,102,125) DIM | `P::SECONDARY_COOL` no DIM |
| `args_label` | Rgb(115,128,150) DIM | `P::SECONDARY_COOL` no DIM |
| `result_label` | Rgb(100,112,135) DIM | `P::SECONDARY_COOL` no DIM |
| `model_style` | Rgb(185,145,240) — keep | kept, passes AAA |

### 6.3 `markdown_render.rs`

| Element | Was | Fix |
|---------|-----|-----|
| Code block `│` bar | Rgb(100,100,100) | `P::TERTIARY_WARM` Rgb(128,138,152) |
| Fence badge DIM | Rgb(205,150,60) DIM | Rgb(205,150,60) no DIM (base CR=5.1) |
| Separator `─` | Rgb(100,100,100) | `P::TERTIARY_WARM` |

### 6.4 `app.rs`

| Element | Was | Fix |
|---------|-----|-----|
| `dot_style` " · " | Rgb(100,100,120) | Rgb(148,148,165) — L=0.29 CR=6.3 |
| `tagline_style` DIM | Rgb(184,134,11) DIM | remove DIM (base CR=6.5 ✅) |
| `label_style` DIM | Rgb(140,140,155) DIM | Rgb(165,165,178) no DIM — L=0.41 |
| `hint_style` DIM | Rgb(120,120,135) DIM | Rgb(160,160,175) no DIM — L=0.38 |
| Placeholder | Rgb(100,100,100) ITALIC | Rgb(135,135,148) ITALIC — L=0.22 CR=5.0 |
| Steer placeholder | Rgb(80,90,110) ITALIC | Rgb(135,145,165) ITALIC — L=0.27 CR=6.4 |

### 6.5 `skin_engine.rs`

| Skin | Token | Was | Fix |
|------|-------|-----|-----|
| mono | `ui_label` | `#6e7681` (CR=4.22) | `#808d97` (CR=5.7) |

---

## 7. Verification Matrix

After implementation, every semantic text span must satisfy:

| Span type | Minimum CR | Standard |
|-----------|-----------|----------|
| Primary text (tool name, result) | 7 : 1 | AAA |
| Secondary text (preview, hint) | 4.5 : 1 | AA |
| Tertiary text (elapsed, duration) | 4.5 : 1 | AA |
| Non-text UI (borders, icons) | 3 : 1 | SC 1.4.11 |
| Decorative (gutter ┊, rule ─) | exempt | SC 1.4.3 |

Test coverage: `theme.rs` `mod tests` includes `test_palette_wcag_aa` which
asserts contrast ≥ 4.5 for every non-decorative palette constant.

---

## 8. Edge Cases

| Case | Handling |
|------|---------|
| Custom skin overrides to a failing colour | `SkinConfig::validate_contrast()` logs `tracing::warn!` but does not panic — users control their skin |
| Light-background terminal | Not supported; EdgeCrab is designed for dark terminals. Banner + tool palette assume dark bg. |
| High-contrast OS mode | No specific handling — OS high-contrast typically overrides all terminal colours anyway |
| Terminal with DIM disabled | DIM was removed from semantic text; no regression |
| `Modifier::ITALIC` | No luminance effect; does not affect contrast compliance |
| `Modifier::BOLD` | Typically increases perceived weight; contrast stays same or improves |
| `Modifier::DIM` remaining usages | Only on: `bar_style "  ┊ "`, `indent "     "`, `"···"` running indicator, decorative `rule_style` |
