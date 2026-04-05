# Session Storage

Verified against:
- `crates/edgecrab-state/src/session_db.rs`
- `crates/edgecrab-state/src/schema.sql`

EdgeCrab persists conversation state in SQLite through `SessionDb`.

## Storage model

```text
+-----------------------------+
| conversation updates        |
+-----------------------------+
               |
               v
+-----------------------------+
| SessionDb writes session row|
+-----------------------------+
               |
               v
+-----------------------------+
| SessionDb writes messages   |
+-----------------------------+
               |
               v
+-----------------------------+
| FTS5 stays in sync through  |
| schema triggers             |
+-----------------------------+
               |
               v
+-----------------------------+
| list, search, export, and   |
| insights read the same store|
+-----------------------------+
```

## Properties visible in code

- SQLite is opened in WAL mode.
- foreign keys are enabled.
- writes use retry with jitter to avoid lock convoy behavior.
- the current schema version constant is `6`.

## Main APIs

- `save_session`
- `list_sessions`
- `list_sessions_by_source`
- `list_sessions_rich`
- `prune_sessions`
- `export_session_jsonl`
- `export_all_jsonl`
- `get_messages`

## Stored data

Session rows track:

- id
- source
- user id
- model
- system prompt
- lineage fields
- token counts
- estimated cost
- title

Message rows store the conversation transcript in the shared `Message` format from `edgecrab-types`.

## Naming note

The current runtime uses `state.db` as the main SQLite file in the EdgeCrab home and in profile homes.
