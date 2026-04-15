---
title: Backup & Import
description: Export and import EdgeCrab configuration, sessions, memories, and skills. Grounded in crates/edgecrab-cli/src/backup.rs.
sidebar:
  order: 13
---

EdgeCrab provides backup and import functionality for migrating between machines, creating snapshots, or disaster recovery.

---

## Backup

Export your EdgeCrab state to a compressed archive:

```bash
edgecrab backup
# Creates ~/.edgecrab/backups/edgecrab-backup-YYYY-MM-DD.tar.gz
```

### What's Included

| Asset | Path | Backup |
|-------|------|--------|
| Configuration | `~/.edgecrab/config.yaml` | ✅ |
| Environment | `~/.edgecrab/.env` | ✅ |
| Memories | `~/.edgecrab/memories/` | ✅ |
| Skills | `~/.edgecrab/skills/` | ✅ |
| Sessions DB | `~/.edgecrab/sessions.db` | ✅ |
| Skins | `~/.edgecrab/skins/` | ✅ |
| MCP tokens | `~/.edgecrab/mcp-tokens/` | ✅ |

---

## Import

Restore from a backup archive:

```bash
edgecrab import path/to/edgecrab-backup.tar.gz
```

The import extracts files into `~/.edgecrab/`, preserving the directory structure. Existing files are overwritten.

---

## Migration from Hermes Agent

If migrating from [hermes-agent](https://github.com/raphaelmansuy/hermes-agent):

```bash
edgecrab migrate --dry-run    # preview what will be imported
edgecrab migrate              # live migration
```

| Asset | Source | Destination |
|-------|--------|-------------|
| Config | `~/.hermes/config.yaml` | `~/.edgecrab/config.yaml` |
| Memories | `~/.hermes/memories/` | `~/.edgecrab/memories/` |
| Skills | `~/.hermes/skills/` | `~/.edgecrab/skills/` |
| Env vars | `~/.hermes/.env` | `~/.edgecrab/.env` |

---

## Debug Dump

For diagnostics, use `/dump` or `/debug` in the TUI to inspect current session state, loaded tools, and configuration.
