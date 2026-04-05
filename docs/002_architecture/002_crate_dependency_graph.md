# 002.002 — Crate Dependency Graph

> **Cross-refs**: [→ INDEX](../INDEX.md) | [→ 002.001 Architecture](001_system_architecture.md)

## 1. Workspace Crate DAG

```
                             edgecrab-types
                           (shared types, no deps)
                                  ^
                                  |
              +-------------------+-------------------+
              |                   |                   |
        edgecrab-security   edgecrab-state      edgecrab-cron
        (injection scan,    (SQLite, config,    (cron scheduler,
         redaction)          memory, skills)     jobs.json)
              ^                   ^                   ^
              |                   |                   |
              +--------+----------+----+              |
                       |               |              |
                 edgecrab-tools        |              |
                 (registry, all        |              |
                  tool impls)          |              |
                       ^               |              |
                       |               |              |
                 edgecrab-core         |              |
                 (Agent, loop,         |              |
                  prompts,             |              |
                  compression)         |              |
                       ^               |              |
                       |               |              |
       +----------+----+-----+---------+------+-------+
       |          |          |                |
 edgecrab-cli  edgecrab-  edgecrab-    edgecrab-
 (TUI, clap)   gateway    acp          migrate
               (14 plat)  (editor)     (hermes/OC)
```

## 2. Crate Responsibilities

| Crate | Type | Dependencies | Purpose |
|-------|------|-------------|---------|
| `edgecrab-types` | lib | serde, chrono | Shared types: Message, ToolSchema, ToolResult, Config types |
| `edgecrab-security` | lib | types, regex, aho-corasick | Injection scanning, secret redaction, command approval, URL safety |
| `edgecrab-state` | lib | types, rusqlite, serde_yaml | SessionDB (FTS5), ConfigManager, MemoryStore, SkillStore |
| `edgecrab-cron` | lib | types, state, tokio-cron-scheduler | Cron job persistence (jobs.json), scheduler, gateway-triggered jobs |
| `edgecrab-tools` | lib | types, security, state, edgequake-llm | ToolRegistry, ToolHandler trait, all 50+ tool implementations |
| `edgecrab-core` | lib | types, tools, state, security, edgequake-llm | Agent struct, conversation loop, PromptBuilder, ContextCompressor, ModelRouter |
| `edgecrab-cli` | bin | core, tools, state, ratatui, clap, crossterm | Interactive TUI, subcommands, skin engine, slash commands |
| `edgecrab-gateway` | lib+bin | core, tools, state, cron, platform crates | GatewayRouter, 14 platform adapters, session management, delivery |
| `edgecrab-acp` | lib+bin | core, tools, axum | ACP server for VS Code / Zed / JetBrains |
| `edgecrab-migrate` | lib | state, serde_yaml, serde_json | hermes-agent / OpenClaw config + memory + skills migration |

## 3. External Dependency Map

```
edgecrab-types
  ├── serde (+ serde_json, serde_yaml)
  ├── chrono
  └── uuid

edgecrab-security
  ├── edgecrab-types
  ├── regex
  ├── aho-corasick (multi-pattern scan)
  └── unicode-segmentation

edgecrab-state
  ├── edgecrab-types
  ├── rusqlite { features = ["bundled", "functions"] }
  ├── serde_yaml
  ├── tokio (fs, sync)
  └── cron (cron expression parsing)

edgecrab-tools
  ├── edgecrab-types
  ├── edgecrab-security
  ├── edgecrab-state
  ├── edgequake-llm (LLM operations)
  ├── tokio (process, net, fs)
  ├── reqwest (HTTP client)
  ├── scraper (HTML parsing)
  ├── inventory (compile-time tool registration)
  ├── bollard [docker-backend]
  ├── russh [ssh-backend]
  └── mcp-rust-sdk [mcp]

edgecrab-core
  ├── edgecrab-types
  ├── edgecrab-tools
  ├── edgecrab-state
  ├── edgecrab-security
  ├── edgequake-llm
  ├── tokio
  └── tracing

edgecrab-cli
  ├── edgecrab-core
  ├── ratatui
  ├── crossterm
  ├── clap { features = ["derive"] }
  └── directories

edgecrab-gateway
  ├── edgecrab-core
  ├── teloxide [telegram]
  ├── serenity [discord]
  └── axum [api-server]
```

## 4. Build Order

```
1. edgecrab-types      (leaf — no internal deps)
2. edgecrab-security   (depends on types)
3. edgecrab-state      (depends on types)
4. edgecrab-cron       (depends on types, state)
5. edgecrab-tools      (depends on types, security, state)
6. edgecrab-core       (depends on types, tools, state, security)
7. edgecrab-cli        (depends on core)           } These four
8. edgecrab-gateway    (depends on core, cron)     } build in
9. edgecrab-acp        (depends on core)           } parallel
10. edgecrab-migrate   (depends on state)          }
```

Cargo builds crates 1-3 in parallel, then 4, then 5, then 6-9 in parallel.
Typical clean build time target: <90s on M1 Mac, <180s on CI.
