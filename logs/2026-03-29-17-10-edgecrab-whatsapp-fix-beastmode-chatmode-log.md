# Task Log: EdgeCrab WhatsApp Response Fix

## Actions
- Fixed Bug 1 (CRITICAL): `run.rs` dispatch loop used `self.delivery_router` (always empty) instead of local `delivery_router` (with adapters registered). Added `Arc::new(delivery_router)` after adapter registration loop; changed dispatch to use local Arc.
- Fixed Bug 2: `whatsapp.rs` `start_managed_bridge()` strips `+` prefix before passing to `WHATSAPP_ALLOWED_USERS` env var.
- Fixed Bug 3: `whatsapp.rs` `WhatsappAdapterConfig::from()` strips `+` from `allowed_users` at config conversion time.
- Killed all stale gateway/bridge processes; restarted cleanly (gateway PID 74484, bridge PID 74515).
- Verified: `cargo check` clean; `cargo test` 773 tests 0 failed.

## Decisions
- Seal local delivery_router into Arc after adapter registration loop, use it (not self.delivery_router) in dispatch.
- Normalize `+` at both layers (env var injection AND config conversion) defensively.

## Next Steps
- Send WhatsApp message to +33614251689 to verify end-to-end response now works.
- Monitor `~/.edgecrab/logs/gateway.log` for new delivery errors.

## Lessons/Insights
- Local Rust variable and a same-named `self` field silently diverge: adapters registered into local `delivery_router`, but dispatch cloned the empty `self.delivery_router`.
