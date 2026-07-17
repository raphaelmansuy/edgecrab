# 016 Discord History Backfill — Implementation Proof

Shipped:

- `crates/edgecrab-gateway/src/backfill.rs` — convert/prune/markers
- `DiscordAdapter::fetch_channel_history`
- Live path in `run.rs`: first-seen channel seeds empty session when `gateway.discord.backfill_on_join`
- Marker file: `~/.edgecrab/channel_backfill.json`
