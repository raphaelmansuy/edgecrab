---
title: Browser Automation
description: Control Chrome/Chromium from EdgeCrab using the Chrome DevTools Protocol. Grounded in crates/edgecrab-tools/src/tools/browser.rs and BrowserConfig in config.rs.
sidebar:
  order: 9
---

EdgeCrab includes a full suite of browser automation tools using the Chrome DevTools Protocol (CDP). The agent can navigate, interact with, screenshot, and visually analyze web pages.

---

## Prerequisites

Browser tools require Chrome or Chromium:

```bash
# macOS
brew install --cask google-chrome
# or
brew install --cask chromium

# Linux
snap install chromium

# Or point to any CDP-compatible endpoint
export CDP_URL=http://localhost:9222
```

If no browser binary is found and `CDP_URL` is not set, browser tools return a structured error explaining what's missing.

---

## Browser Tools

| Tool | Description |
|------|-------------|
| `browser_navigate` | Navigate to a URL; waits for page load |
| `browser_snapshot` | Capture the accessibility tree (text-based page view) |
| `browser_screenshot` | Take a viewport or full-page screenshot as base64 PNG |
| `browser_click` | Click an element by CSS selector, text, or coordinate |
| `browser_type` | Type text into the focused or specified input element |
| `browser_scroll` | Scroll the page by pixels or to an element |
| `browser_console` | Return buffered `console.log`/`warn`/`error` messages |
| `browser_back` | Navigate back in browser history |
| `browser_press` | Press a keyboard key (e.g. `Enter`, `Tab`, `Escape`) |
| `browser_close` | Close the browser and release the CDP connection |
| `browser_get_images` | Return all images visible on the page as base64 |
| `browser_vision` | Take a screenshot and analyze it with the vision model |

---

## Configuration

```yaml
browser:
  command_timeout: 30          # CDP call timeout in seconds
  record_sessions: false       # record sessions as WebM video
  recording_max_age_hours: 72  # auto-delete recordings older than this
```

---

## Session Recording

Enable session recording to capture everything the agent does in the browser as a WebM video:

```yaml
browser:
  record_sessions: true
  recording_max_age_hours: 72  # auto-delete after 72h
```

Recordings are saved to `~/.edgecrab/browser_recordings/` with timestamps.

---

## Using the Browser Toolset

Enable browser tools:

```bash
edgecrab --toolset browser "research competitors at stripe.com"
edgecrab --toolset research "find the latest release notes for Rust"
```

Browser tools are included in the `core` and `research` toolset aliases.

---

## Example Agent Session

```
❯ Take a screenshot of https://crates.io and tell me what's featured
```

Agent workflow:
1. `browser_navigate("https://crates.io")` — navigates to the page
2. `browser_screenshot()` — captures a PNG
3. `browser_vision({screenshot})` — analyzes with vision model
4. Returns: "The featured crates this week are..."

---

## Vision Analysis

`browser_vision` combines `browser_screenshot` with visual model analysis in one call:

```
❯ Is the login button visible on the current page?
❯ What does the error message say?
❯ Describe the layout of this dashboard
```

The vision call uses the model configured in `model.default` (or `auxiliary.model` if set). Models that don't support vision fall back to `browser_snapshot` (accessibility tree text).

---

## Enabling via Toolset Config

```yaml
tools:
  enabled_toolsets:
    - core        # includes browser (runtime-gated)
    - web         # web_search + web_extract + web_crawl
```

Or to include browser explicitly:

```yaml
tools:
  enabled_toolsets:
    - browser     # only browser tools
    - file        # add file tools
```

---

## Pro Tips

**Use `browser_snapshot` before `browser_screenshot`.** The accessibility tree snapshot is 10-100× smaller than a screenshot and contains the same text content. Use `browser_screenshot` + `browser_vision` only when you need to analyze visual layout.

**Record sessions for debugging.** Enable `browser.record_sessions: true` before running a browser-heavy task. The WebM recording in `~/.edgecrab/browser_recordings/` is invaluable for debugging what went wrong.

**Use CDP_URL for remote browsers.** If you run Chrome headlessly on a remote server:
```bash
# On the server (start Chrome with remote debugging)
google-chrome --headless --remote-debugging-port=9222 &

# In your EdgeCrab config or environment
export CDP_URL=http://your-server:9222
```

---

## Frequently Asked Questions

**Q: Browser tools fail with "no browser found". What do I install?**

On macOS: `brew install --cask google-chrome` or `brew install --cask chromium`
On Linux: `apt install chromium-browser` or `snap install chromium`
On CI: `CDP_URL` pointing to a Playwright-managed Chrome is the cleanest approach.

**Q: Can I use a non-Chrome browser (Firefox, Safari)?**

Only Chrome/Chromium is supported — they implement the Chrome DevTools Protocol (CDP). Firefox has partial CDP support but is not officially supported. Use `CDP_URL` to point at any CDP-compatible browser.

**Q: Is the browser sandboxed? Can the agent access my browser history?**

EdgeCrab opens a fresh Chrome profile by default (no cookies, no saved logins, no history from your personal browser). The `CDP_URL` path allows connecting to an existing browser, which would have access to that browser's session.

**Q: Browser tools are slow. How do I speed them up?**

Reduce `browser.command_timeout` for faster failures on unresponsive pages. For static content, `web_extract` (no browser) is much faster than `browser_navigate` + `browser_snapshot`.

**Q: `browser_vision` returns "model does not support vision". Why?**

Switch to a model that supports vision: `openai/gpt-4o`, `anthropic/claude-sonnet-4`, or `google/gemini-2-flash`. Configure in your session: `/model openai/gpt-4o`.

---

## See Also

- [Tools & Toolsets](/features/tools/) — Browser toolset and custom groups
- [Configuration](/user-guide/configuration/) — Full `browser.*` config reference
- [Security Model](/user-guide/security/) — SSRF protection for browser navigation
