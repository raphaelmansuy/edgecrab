# Task Log: Session Parity with hermes-agent

## Actions
- Added 15+ new DB methods to session_db.rs (resolve, prune, end/reopen, rich listing, export, stats, title hygiene, lineage)
- Added `--continue`/`-C` and `--resume`/`-r` CLI flags to cli_args.rs
- Added `sessions rename` and `sessions prune` CLI subcommands
- Added `/session rename` and `/session prune` slash commands
- Added JSONL export format (`--format jsonl`) to sessions export
- Added conversation recap on session resume (styled panel with recent turns)
- Added `Validation` error variant, `SessionExport`/`SessionRichSummary`/`SessionStats` types
- Updated schema.sql with unique title index
- Fixed `-c` short flag conflict (changed to `-C` since `-c` was taken by `--config`)
- Fixed insights test for unique title constraint

## Decisions
- Used `-C` (uppercase) for `--continue` since `-c` was taken by `--config`
- Conversation recap shows last 10 exchanges with user/assistant truncation (300/200 chars)
- Title lineage uses `#N` suffix pattern matching hermes-agent
- JSONL export serializes full SessionExport struct (session record + all messages)

## Next steps
- Integration testing with real sessions
- Wire title lineage into split_session for auto-naming on compression
- Consider auto-titling via LLM after first exchange (larger feature, deferred)

## Lessons/insights
- Unique indexes on nullable columns need `WHERE col IS NOT NULL` in SQLite
- clap short flags must be globally unique across all args (not just within same arg group)
