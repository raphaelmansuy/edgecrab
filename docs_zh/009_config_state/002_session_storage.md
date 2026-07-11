# 🦀 会话存储

> **为什么：** 对话历史、成本跟踪和跨过去会话的搜索都需要一个可靠、可查询的存储。EdgeCrab 使用嵌入式 SQLite — 没有外部数据库进程，没有网络依赖，除了文件路径外没有配置。

**来源：** `crates/edgecrab-state/src/session_db.rs`, `crates/edgecrab-state/src/schema.sql`

---

## 架构概览

```
┌──────────────────────────────────────────────────────────────┐
│                      Agent / Gateway                          │
│   save_session()   get_messages()   list_sessions_rich()      │
└──────────────────────────┬───────────────────────────────────┘
                           │ SessionDb API
                           ▼
┌──────────────────────────────────────────────────────────────┐
│                      SessionDb                                │
│                                                               │
│  ┌─────────────┐   ┌──────────────┐   ┌───────────────────┐  │
│  │  sessions   │   │   messages   │──►│  messages_fts     │  │
│  │  table      │◄──│   table      │   │  (FTS5 virtual)   │  │
│  │             │   │              │   │  auto-synced       │  │
│  └─────────────┘   └──────────────┘   │  via triggers      │  │
│                                        └───────────────────┘  │
│                                                               │
│  WAL mode  •  FK enforcement  •  jitter-retry writes         │
└──────────────────────────────────────────────────────────────┘
                           │
                     state.db  (SQLite file)
```

---

## SQLite 调优

EdgeCrab 以三个在规模上重要的设置打开 SQLite：

| 设置 | 值 | 原因 |
|---|---|---|
| Journal 模式 | `WAL` | 写入期间非阻塞读取；对同一文件上的并发 CLI + 网关使用至关重要 |
| 外键 | `ON` | 如果修剪会话则防止孤立的消息行 |
| Schema 版本 | `6` | 打开时验证；迁移自动运行 |

**写操作防排队**：每个写路径使用指数退避和抖动在 [20, 150) ms 范围内，因此并发写入器（CLI、网关、cron）不会堆积成锁排队。`WRITE_MAX_RETRIES = 15`。

```
Writer 1 ──► locked ──► wait 73ms ──► retry ──► success
Writer 2 ──► locked ──► wait 41ms ──► retry ──► success (different jitter)
```

---

## Schema：Sessions 表

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | UUID |
| `source` | TEXT | 平台：`cli`、`telegram`、`discord`... |
| `user_id` | TEXT | 平台特定的用户标识符 |
| `model` | TEXT | 用于该会话的模型 |
| `system_prompt` | TEXT | 会话开始时的完整系统提示 |
| `parent_id` | TEXT | 谱系 — 如果分支则父会话 ID |
| `root_id` | TEXT | 谱系 — 会话树的根 |
| `prompt_tokens` | INTEGER | 累积输入 token |
| `completion_tokens` | INTEGER | 累积输出 token |
| `estimated_cost_usd` | REAL | 运行成本估算 |
| `title` | TEXT | 自动生成或用户设置的标题 |
| `created_at` | TEXT | ISO8601 UTC |
| `updated_at` | TEXT | ISO8601 UTC |

---

## Schema：Messages 表

消息行以来自 `edgecrab-types` 的标准 `Message` 格式存储对话记录。每行是一个完整的序列化 `Message` — 角色、内容、可选的工具调用、可选的推理 — 因此重放会话只是简单的有序扫描。

FTS5 通过数据库触发器保持 `messages_fts` 同步。触发器在 `INSERT` 和 `UPDATE` 时触发，因此搜索始终与零应用程序级簿记保持最新。

---

## 公共 API

```rust
// Persist a completed session with its messages
SessionDb::save_session(&self, session: &Session) -> Result<()>

// List all sessions, newest first
SessionDb::list_sessions(&self) -> Result<Vec<SessionSummary>>

// List sessions by the source platform
SessionDb::list_sessions_by_source(&self, source: &str) -> Result<Vec<SessionSummary>>

// List sessions with token and cost metadata
SessionDb::list_sessions_rich(&self) -> Result<Vec<RichSessionSummary>>

// Delete sessions older than the given age
SessionDb::prune_sessions(&self, older_than: Duration) -> Result<usize>

// Export a single session as newline-delimited JSON
SessionDb::export_session_jsonl(&self, id: &str) -> Result<String>

// Export all sessions as newline-delimited JSON
SessionDb::export_all_jsonl(&self) -> Result<String>

// Retrieve full message history for a session
SessionDb::get_messages(&self, session_id: &str) -> Result<Vec<Message>>
```

---

## 写入流程

```
Agent loop completes turn
        │
        ▼
serialize Message to JSON
        │
        ▼
INSERT INTO messages  ──► trigger fires ──► FTS5 index updated
        │
        ▼
UPDATE sessions (tokens, cost, updated_at)
        │
        ▼
COMMIT (WAL — readers unblocked immediately)
```

---

## 存储文件位置

| 上下文 | 路径 |
|---|---|
| 默认 | `~/.edgecrab/state.db` |
| 自定义主目录 | `$EDGECRAB_HOME/state.db` |
| 配置文件 | `~/.edgecrab/profiles/<name>/state.db` |

每个配置文件都有自己的隔离 `state.db` — `work` 配置文件的会话从不出现在 `personal` 配置文件的历史中。

---

## 由 SessionDb 支持的斜杠命令

| 命令 | 操作 |
|---|---|
| `/history` | `list_sessions_rich` |
| `/search <query>` | 对 `messages_fts` 进行 FTS5 全文搜索 |
| `/export` | `export_session_jsonl` |
| `/prune` | `prune_sessions` |
| `/cost` | 从 `sessions` 读取 `estimated_cost_usd` |

---

## 提示

- **FTS5 在大历史中搜索很快 —** SQLite FTS5 使用 B 树索引，而不是全扫描。`/search` 跨 10,000 条消息是亚毫秒级的。
- **WAL 模式意味着您可以在 EdgeCrab 运行时从外部查询 `state.db`**（例如，使用 `sqlite3` CLI 或 DBeaver）— 读取永远不会阻塞。
- **在修剪前导出 —** `export_all_jsonl` 将每个会话写入标准输出；在运行 `/prune` 之前将其管道到文件作为备份。

---

## 常见问题

**问：我可以在同一台机器上的多个用户之间共享一个 `state.db` 吗？**
答：技术上可以（WAL 处理并发访问），但会话不在 SQLite 级别进行访问控制。请改用单独的配置文件。

**问：EdgeCrab 会自动运行 schema 迁移吗？**
答：是的。在打开时检查 schema 版本；如果低于 `6`，迁移 SQL 在任何其他查询之前自动运行。

**问：如何从命令行搜索旧对话？**
答：`edgecrab /search "query"` — 由 FTS5 支持，支持短语搜索和前缀通配符。

---

## 交叉引用

- 配置文件路径 → [`009_config_state/001_config_state.md`](001_config_state.md)
- 消息数据模型 → [`010_data_models/001_data_models.md`](../010_data_models/001_data_models.md)
- 并发模型（WAL + jitter）→ [`002_architecture/003_concurrency_model.md`](../002_architecture/003_concurrency_model.md)
- 平台源值 → [`006_gateway/001_gateway_architecture.md`](../006_gateway/001_gateway_architecture.md)
