# Proof: P4 port-bind truth

**Date:** 2026-07-18  
**Status:** landed (session forensics game005)  
**Session seed:** `a63fd17c` — spawn `ok:true` then `Address already in use`

## Law

1. Session HTTP ports are recorded on **bind-ready** (`Serving HTTP…`) or
   successful probe — not solely at command spawn.
2. `Address already in use` / background exit ≠ 0 for a preview-server command
   **unrecords** the inferred port and emits structured port-heal recovery.
3. Optimistic “Dev server expected at :PORT” must not imply a listening socket.

## Anchors

| Piece | Location |
|-------|----------|
| Record / unrecord | `dev_server::{record,unrecord}_session_http_*` |
| Ready notice | `maybe_http_server_ready_notice` + process_table ready path |
| EADDRINUSE heal | `recovery_catalog::terminal_port_in_use` |
| Spawn honesty | `append_spawn_hint` marks expected, not ready |

## Verify

```bash
cargo test -p edgecrab-tools --lib port_bind
cargo test -p edgecrab-tools --lib unrecord_session_http
cargo test -p edgecrab-tools --lib address_already_in_use
```
