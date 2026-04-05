# Memory / Skills / State Gap Analysis

## Bottom line

EdgeCrab exceeds Hermes on migration direction, typed state boundaries, and local auditability.

That statement needed re-validation before it was credible.

Previously, the migration claim was incomplete because EdgeCrab imported config, memory, skills, and environment data from Hermes, but not Hermes' persisted session state.

That gap is now closed in code.

From first principles, memory, skills, and state only earn trust when five things are true:

- persistence is inspectable
- state survives restart without semantic loss
- migration paths preserve switching cost
- skill inventory is observable instead of implied
- duplicate or partial imports fail safely

## Re-assessed facts

### Hermes still has a raw skill-count lead

The repos do not currently have the same raw skill inventory:

- EdgeCrab: 111 skill definitions
- Hermes: 116 skill definitions

That matters, but it does not overturn the state verdict in this document.

Raw count is a weaker signal than migration quality, auditability, and state fidelity.

### Both systems ship durable local state

Hermes and EdgeCrab both ship:

- SQLite-backed session persistence
- FTS5-backed recall over past sessions
- local memory files
- skill discovery and skill loading

The comparison is no longer about whether state exists.

It is about how inspectable, portable, and lossless that state is.

## Code-backed EdgeCrab advantages

### 1. Hermes-to-EdgeCrab migration now includes session state, not just files

`crates/edgecrab-migrate/src/hermes.rs` now migrates:

- `config.yaml`
- `state.db`
- `memories/`
- `skills/`
- `.env`

The important change is `migrate_state()`: Hermes sessions and messages are imported into EdgeCrab's `state.db` with duplicate-safe semantics instead of being silently abandoned.

That implementation now does two details that matter under adversarial review:

- parent-linked session chains import in dependency order instead of assuming `started_at` order
- unresolved parent chains fail the migration transaction cleanly instead of partially importing broken lineage

That is the decisive portability edge. A migration path that drops conversation history is not a serious migration path.

### 2. State round-trips with less semantic loss

`crates/edgecrab-state/src/session_db.rs` now persists tool-result `tool_name` on write and restores it on read.

That matters because a restored tool message without the originating tool identity is degraded state, not faithful state.

Hermes already preserved that field. EdgeCrab now does too.

### 3. State boundaries are still easier to audit

EdgeCrab keeps the relevant surfaces separated across crates:

- `edgecrab-state`
- `edgecrab-tools`
- `edgecrab-core`
- `edgecrab-migrate`

That separation is still an architectural advantage over reasoning about memory, skills, migration, and runtime state as one broad subsystem.

### 4. Local persistence remains directly inspectable

EdgeCrab's persistence model is still straightforward to inspect locally:

- `state.db` for sessions and messages
- `memories/` for curated durable notes
- `skills/` for installed or authored skills

That keeps the source of truth readable without requiring a hosted control plane.

## Remaining Hermes strengths

Hermes still appears stronger in two narrower areas:

- memory mutation hardening has more battle-tested production mileage
- public evidence of sustained learning-loop exercise is broader

Those are runtime confidence advantages, not architecture advantages.

## Gap verdict

After re-assessment, the original claim is now supportable, but for a narrower and more defensible reason:

EdgeCrab exceeds Hermes here because it now combines:

- duplicate-safe Hermes state migration
- parent-chain-aware session import ordering
- fail-safe rollback on invalid or partial parent lineage
- auditable local persistence
- explicit crate-level state boundaries
- session-state fidelity that no longer drops tool provenance on round-trip

Hermes still retains some maturity advantages in long-run operational exercise, but the portability and inspectability edge is now materially on the EdgeCrab side.

## Sources audited

- `edgecrab/crates/edgecrab-state/src/session_db.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/session_search.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/memory.rs`
- `edgecrab/crates/edgecrab-tools/src/tools/skills.rs`
- `edgecrab/crates/edgecrab-migrate/src/hermes.rs`
- `edgecrab/crates/edgecrab-migrate/tests/hermes_migration_e2e.rs`
- `hermes-agent/hermes_state.py`
- `hermes-agent/tools/memory_tool.py`
- `hermes-agent/tools/skills_tool.py`
