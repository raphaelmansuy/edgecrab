# Proof: VisualUx preview lifecycle (game002)

**Date:** 2026-07-17  
**Meter:** Task success (create + verify HTML demo under Indexed / VisualUx)  
**Failure signature:** session thrash after asset writes — `terminal http.server` blocked by visual storm, then localhost port shopping (`8080` / `5050` / `5000`).

## Capability law

```text
write_file assets → terminal preview server (session-recorded port)
  → browser_navigate exact URL → browser_snapshot → complete
```

Perception is only valid against a **session-recorded** listening port (or allowlisted preview after serve). The harness must never require port guessing.

## Root cause

`visual_storm_block` forbade `terminal` after ≥5 act tools without successful `browser_navigate`, but navigate requires an HTTP server started via `terminal` — a capability paradox. Recovery copy said “start a dev server” while the serve tool was blocked.

## Fixes

| Fix | Anchor |
|-----|--------|
| Exempt preview-server starts from visual storm | `is_preview_server_command` + `visual_storm_block_result_with_args` |
| Empty `known_ports` → `CallToolFirst(terminal)` exact recipe | `preview_serve_then_navigate_recipe` / `browser_navigate_blocked` |
| Halt loopback port shopping after one failure | `maybe_loopback_port_shopping_block` |
| VisualUx turn-start materialize browser verify tools | `conversation.rs` Indexed path |
| Connection-refused → serve-first recovery | `browser_navigate_no_server` |

## Verification

```bash
cargo test -p edgecrab-tools --lib is_preview_server
cargo test -p edgecrab-tools --lib ha16_browser_blocked
cargo test -p edgecrab-core --lib visual_storm
cargo test -p edgecrab-core --lib loopback_port_shopping
cargo test -p edgecrab-core --test visual_preview_lifecycle_e2e
```

## Related

- [create-path-disclosure.md](./create-path-disclosure.md) (game001 write path)
- [tool-progressive-load.md](./tool-progressive-load.md)
