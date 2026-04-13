# ADR-004: Improve Gateway Configuration Security & UX

**Status:** Accepted  
**Date:** 2026-04-12  
**Authors:** EdgeCrab Team  

---

## Context

The EdgeCrab gateway connects an AI agent to 15 messaging platforms (Telegram, Discord, Slack, WhatsApp, Signal, Email, SMS, Matrix, Mattermost, Feishu, WeCom, DingTalk, Home Assistant, Webhook, API Server). Misconfiguration can expose the agent to unauthorized users, leak conversations to unintended groups, or silently fail in ways that leave operators unaware.

A deep audit of both the EdgeCrab (`crates/edgecrab-gateway/`, `crates/edgecrab-cli/src/gateway_setup.rs`, `crates/edgecrab-cli/src/gateway_catalog.rs`) and hermes-agent (`gateway/run.py`, `gateway/pairing.py`, `hermes_cli/config.py`) codebases revealed **10 critical gaps** that this ADR addresses.

---

## First-Principles Analysis

### What is the gateway?

A **message router** that accepts inbound messages from external platforms, dispatches them to an AI agent, and delivers responses back. It is a **trust boundary** — every inbound message is potentially from an untrusted actor.

### What must be true for it to be secure?

1. **Explicit trust**: The operator must consciously decide *who* can talk to the agent, per platform.
2. **Least privilege**: Default must be **deny-all**, not open-access.
3. **Visibility**: The operator must see, at all times, exactly what the current authorization posture is.
4. **No silent failures**: Misconfigurations must produce clear, actionable warnings — not silent open access.
5. **Group isolation**: Group chats introduce multi-party trust — the operator must explicitly opt in.
6. **Platform-aware guidance**: Each platform has different identity models (user IDs vs phone numbers vs MXIDs). The UX must guide the operator through each model.

---

## ADR: Deep Audit Findings & Improvements

### Finding 1: Default-Open Gateway (CRITICAL)

**Current behavior** (edgecrab `run.rs:1495`):
```rust
// 4. If no allowlist is configured → open gateway
if global_list.trim().is_empty() && platform_list.trim().is_empty() {
    return true;  // ← ANYONE CAN USE THE BOT
}
```

**Impact**: A user who enables Telegram, sets a bot token, but forgets to configure an allowlist has an **open gateway**. Any Telegram user who discovers the bot can use it — consuming API credits, accessing tools (file read/write, terminal), and potentially exfiltrating data.

**hermes-agent comparison**: Hermes defaults to **deny** when no allowlists are configured:
```python
if not platform_allowlist and not global_allowlist:
    return os.getenv("GATEWAY_ALLOW_ALL_USERS") in ("true", "1", "yes")
    # ← Returns False unless EXPLICITLY opted in
```

**Fix**: Change default to **deny-all**. Require explicit `GATEWAY_ALLOW_ALL_USERS=true` to opt into open access. Add a loud startup warning when no allowlist is configured.

### Finding 2: No Group Policy Controls (HIGH)

**Current behavior**: EdgeCrab has no group chat policy system. All messages from groups are processed identically to DMs. There is no:
- Enable/disable group ingress per platform
- Require @mention in groups
- Separate group allowlists
- Group-specific session isolation

**hermes-agent comparison**: Hermes has:
- `group_sessions_per_user: true` — isolates group sessions per participant
- `thread_sessions_per_user: false` — configurable thread isolation
- Per-platform group policies (Feishu: `FEISHU_GROUP_POLICY=mentioned|open|disabled`)

**Fix**: Add `GroupPolicy` enum (`disabled`, `mention_only`, `allowed_users_only`, `open`) to `GatewayConfig`. Default to `disabled` for new configurations.

### Finding 3: Missing Pairing Integration in Auth (HIGH)

**Current behavior** (`run.rs:1450`): The `is_user_authorized()` function checks env-var allowlists but does NOT check the `PairingStore`. Pairing codes exist in the codebase but are disconnected from the authorization flow.

**hermes-agent comparison**: Hermes checks pairing store as priority 2, before allowlists:
```python
if self.pairing_store.is_approved(platform_name, user_id):
    return True
```

**Fix**: Integrate `PairingStore::is_approved()` into `is_user_authorized()` as check #3 (after global allow-all, after per-platform allow-all, before allowlists).

### Finding 4: No Unauthorized DM Behavior Policy (MEDIUM)

**Current behavior**: When an unauthorized user sends a message, edgecrab always sends `"⛔ Unauthorized. Contact the bot administrator."` — there is no pairing flow trigger, no platform-configurable behavior.

**hermes-agent comparison**: Hermes supports `unauthorized_dm_behavior: "pair" | "ignore"`:
- `pair`: Generates a pairing code and sends instructions
- `ignore`: Silent rejection (no response, denies information leakage)

**Fix**: Add `unauthorized_dm_behavior` to `GatewayConfig`. Default to `"pair"` for DMs, always silent for groups.

### Finding 5: No Security Summary in Config Review (MEDIUM)

**Current behavior**: `print_platform_status()` in `gateway_setup.rs` shows platform state (`enabled`, `needs attention`, etc.) but does NOT show:
- Whether allowlists are configured
- Whether the gateway is open-access
- Whether groups are enabled and what the policy is
- Total approved pairing users

**hermes-agent comparison**: Hermes logs a startup warning when no allowlists are configured but also lacks a comprehensive review screen. EdgeCrab can leapfrog here.

**Fix**: Add a `SecurityPosture` analysis that computes and displays a per-platform security assessment with color-coded warnings.

### Finding 6: Incomplete Platform Coverage in Auth (MEDIUM)

**Current behavior** (`run.rs:1471`): The `platform_allow_all_var` match only covers Telegram and Discord:
```rust
let platform_allow_all_var = match msg.platform {
    Platform::Telegram => "TELEGRAM_ALLOW_ALL_USERS",
    Platform::Discord => "DISCORD_ALLOW_ALL_USERS",
    _ => "",  // ← ALL OTHER PLATFORMS IGNORED
};
```

Similarly, `platform_list_var` only covers Telegram and Discord. This means per-platform allowlists for Slack, Signal, WhatsApp, Matrix, etc. are **silently ignored**.

**hermes-agent comparison**: Hermes has a complete `platform_env_map` covering ALL platforms.

**Fix**: Build a complete platform-to-env mapping covering all 15 platforms.

### Finding 7: No Self-Chat Mode Explanation (MEDIUM)

**Current behavior**: WhatsApp setup mentions "self-chat mode" but never explains what it means operationally:
- Only responds to messages from yourself
- Does not respond to group messages
- Ideal for personal automation

**Fix**: Add platform-specific behavior explanations that clearly describe what each configuration choice means in practice.

### Finding 8: Allowlist Editor UX is Primitive (LOW)

**Current behavior**: Allowlists are edited as raw comma-separated strings. No validation of ID format, no listing of current entries, no add/remove individual entries.

**Fix**: Add interactive allowlist editor with:
- Current entries displayed as numbered list
- Add individual entries with format validation
- Remove individual entries by number
- Platform-specific ID format guidance and validation

### Finding 9: No Configuration Dry-Run / Preview (LOW)

**Current behavior**: Changes are applied immediately. No way to preview "what will happen if I enable this platform with this config?"

**Fix**: After each platform configuration, show a `SecuritySummary` preview before saving.

### Finding 10: Race Condition in Pairing File I/O (LOW)

**Current behavior**: `PairingStore` reads and writes JSON files without file locking. Concurrent gateway instances or rapid pairing requests could corrupt the files.

**Fix**: Add advisory file locking (`flock` on Unix) around read-modify-write cycles. (Already mitigated by MAX_PENDING_PER_PLATFORM=3 and rate limiting — low severity.)

---

## Design Decisions

### D1: Secure-by-Default Authorization

```
New default: no allowlist configured → DENY ALL
Exception: GATEWAY_ALLOW_ALL_USERS=true → explicit opt-in to open access
```

This is a **breaking change** from the current behavior. Operators upgrading must either:
1. Add their user IDs to allowlists, OR
2. Set `GATEWAY_ALLOW_ALL_USERS=true`

We mitigate this by:
- Printing a clear migration warning at startup
- Auto-detecting the upgrade case and printing remediation steps

### D2: SecurityPosture analyzer

A new `SecurityPosture` struct analyzes the current config and produces a summary:

```
┌──────────────────────────────────────────────────────┐
│                 Gateway Security Review              │
├────────────┬────────┬───────────┬────────────────────┤
│ Platform   │ Access │ Groups    │ Users              │
├────────────┼────────┼───────────┼────────────────────┤
│ Telegram   │ ✓ list │ disabled  │ 2 allowed, 1 paired│
│ Discord    │ ⚠ OPEN │ disabled  │ none configured    │
│ WhatsApp   │ ✓ self │ disabled  │ self-chat only     │
│ Email      │ ✓ list │ n/a       │ 3 allowed          │
└────────────┴────────┴───────────┴────────────────────┘
  ⚠ WARNING: Discord has no access restrictions!
    Run: edgecrab gateway configure discord
```

### D3: Group Policy per Platform

```rust
pub enum GroupPolicy {
    Disabled,        // Never process group messages (DEFAULT)
    MentionOnly,     // Only respond when @mentioned in groups
    AllowedOnly,     // Only respond to allowed users in groups
    Open,            // Respond to all group messages from authorized users
}
```

### D4: DRY Platform-Env Mapping

Replace scattered `match` arms with a single `platform_env_map()` function used by both setup and runtime:

```rust
fn platform_env_prefix(platform: Platform) -> Option<&'static str> {
    // Single source of truth for ALL platform env var prefixes
}
```

---

## Implementation Plan

### Phase 1: Security Core (auth hardening)

**Files changed:**
- `crates/edgecrab-gateway/src/run.rs`
- `crates/edgecrab-core/src/config.rs`

**Tasks:**
1. Add `GroupPolicy` enum and `group_policy` field to per-platform configs
2. Add `unauthorized_dm_behavior` field to `GatewayConfig`
3. Build complete `platform_env_map()` (DRY, Single Responsibility)
4. Rewrite `is_user_authorized()` to:
   - Default deny when no allowlists configured
   - Check pairing store
   - Support all 15 platforms
   - Respect group policy
5. Add unauthorized DM behavior dispatch (pair or ignore)
6. Add startup security posture logging

### Phase 2: Security Review UX

**Files changed:**
- `crates/edgecrab-cli/src/gateway_catalog.rs`
- `crates/edgecrab-cli/src/gateway_setup.rs`

**Tasks:**
1. Add `SecurityPosture` struct with per-platform analysis
2. Extend `PlatformDiagnostic` with security fields (access_mode, group_policy, user_count)
3. Add security review panel to `print_platform_status()`
4. Add security warnings with color-coded severity
5. Add post-configuration security preview

### Phase 3: Allowlist Editor & Group Config UX

**Files changed:**
- `crates/edgecrab-cli/src/gateway_setup.rs`

**Tasks:**
1. Interactive allowlist editor (list, add, remove, validate)
2. Group policy configuration per platform
3. Platform-specific behavior explanations
4. WhatsApp self-chat mode explanation
5. Format validation per platform (Telegram: numeric, Signal: E.164, Discord: snowflake, etc.)

---

## Roadblocks, Edge Cases, and Mitigations

### Breaking Change: Default-Deny

**Risk**: Existing users who upgrade without configuring allowlists will be locked out.
**Mitigation**: 
- Startup log prints explicit remediation steps
- `edgecrab gateway configure` re-prompts for allowlists
- `GATEWAY_ALLOW_ALL_USERS=true` is a one-line escape hatch

### WhatsApp Self-Chat + Groups

**Edge case**: In self-chat mode, the user IS the bot. Group messages appear to come from the user's own number. 
**Mitigation**: Self-chat mode automatically sets group policy to `Disabled` and only accepts messages from the paired phone number. Setup wizard explains this explicitly.

### Pairing vs Allowlist Priority

**Edge case**: A user is in the allowlist but also has a pairing entry. What happens if the allowlist entry is removed?
**Mitigation**: Pairing store acts as an independent authorization source. Removing from allowlist does not revoke pairing. Explicit `/revoke` removes pairing.

### Concurrent Gateway Instances

**Edge case**: Two gateways with different configs polling the same bot token.
**Mitigation**: Token lock mechanism (already exists) prevents duplicate pollers. Security posture is per-instance.

### Email Platform Group Semantics

**Edge case**: Email has no "groups" in the platform sense. CC/BCC recipients are not group participants.
**Mitigation**: `GroupPolicy` for email defaults to `Disabled` (n/a) and is hidden in the setup wizard.

### Platform-Specific ID Format Validation

| Platform | Format | Validation Rule |
|----------|--------|----------------|
| Telegram | Numeric | `^[0-9]+$` |
| Discord | Snowflake | `^[0-9]{17,20}$` |
| Slack | Member ID | `^[UW][A-Z0-9]+$` |
| Signal | E.164 | `^\+[0-9]{7,15}$` |
| WhatsApp | Phone | `^[0-9]+(@[a-z.]+)?$` |
| Matrix | MXID | `^@[^:]+:.+$` |
| Email | Address | Contains `@` |

**Mitigation**: Validation is advisory (warning, not blocking) to avoid rejecting valid but unusual IDs.
