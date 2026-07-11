# 记忆和技能 🦀

> **已验证来源：** `crates/edgecrab-core/src/prompt_builder.rs` ·
> `crates/edgecrab-tools/src/tools/memory.rs` ·
> `crates/edgecrab-tools/src/tools/skills.rs` ·
> `crates/edgecrab-tools/src/tools/honcho.rs`

---

## 为什么持久化记忆很重要

没有记忆，每个会话都从零开始。EdgeCrab 使用相同的模型，但对您的项目结构、代码风格偏好或过往决策一无所知。记忆为智能体提供了跨会话存活的持久上下文。

没有技能，每次您想让智能体"按照我们的发布清单执行"时，您都需要重新解释一遍。技能将可复用的工作流编码在 Markdown 中，智能体会读取并执行这些工作流。

🦀 *`hermes-agent`（EdgeCrab 的前身）在每个会话都会重置记忆，除非您在启动前手动编辑其 MEMORY.md。OpenClaw ([TypeScript/Node.js](https://github.com/openclaw)) 会持久化会话记录，但没有自动的跨会话记忆注入到系统提示中。EdgeCrab 会记住 — 即使螃蟹去睡觉了也会记住。*

---

## 三种记忆系统

```
  ┌────────────────────────────────────────────────────────────────┐
  │  1. 基于文件的记忆    (~/.edgecrab/memories/)                 │
  │     ■ 纯 Markdown 文件                                        │
  │     ■ 在会话开始时加载到系统提示                               │
  │     ■ 通过 memory_read / memory_write 工具读写               │
  │     ■ 跨所有会话存活                                          │
  │     ■ 加载前进行注入检查                                      │
  └────────────────────────────────────────────────────────────────┘
  ┌────────────────────────────────────────────────────────────────┐
  │  2. 技能              (~/.edgecrab/skills/)                    │
  │     ■ 包含 SKILL.md 文件的目录                                │
  │     ■ 在系统提示中列出（仅摘要）                               │
  │     ■ 按名称调用："use the git-release skill"                  │
  │     ■ 通过 skills_list / skill_manage / skills_hub 管理       │
  └────────────────────────────────────────────────────────────────┘
  ┌────────────────────────────────────────────────────────────────┐
  │  3. Honcho            (外部服务)                                 │
  │     ■ 由 Honcho API 管理的用户级记忆                            │
  │     ■ 对过去会话进行语义搜索                                   │
  │     ■ 需要 HONCHO_APP_ID 环境变量                             │
  │     ■ 工具：honcho_conclude, honcho_search, honcho_profile   │
  └────────────────────────────────────────────────────────────────┘
```

**参考：** [Honcho 文档](https://honcho.dev/docs)

---

## 基于文件的记忆

### 布局

```
  ~/.edgecrab/memories/             (默认；因配置而异)
    MEMORY.md                       ← 主要记忆文件（始终加载）
    USER.md                         ← 用户档案事实
    <any-other>.md                  ← 自定义记忆章节
```

memories 目录中的所有 `.md` 文件都会被加载。它们按字母顺序注入到系统提示中，每个文件在一个单独的章节中：

```
  [memory:MEMORY.md]
  (MEMORY.md 的内容)

  [memory:USER.md]
  (USER.md 的内容)
```

### 安全门控

每个记忆文件在加载前都会经过 `check_memory_content()` 检查：

```
  check_memory_content(content)
    │
    ├── check_injection(content)
    │     ↳ 阻止： "ignore previous", "you are now", 等
    │
    ├── 不可见 Unicode 检查
    │     ↳ 阻止：零宽空格、方向覆盖字符
    │
    └── 数据泄露模式
          ↳ 阻止：带 $SECRET 的 curl、cat ~/.ssh/id_rsa 等
```

未通过检查的文件会被**跳过**并发出警告 — 不会被加载。

### 从智能体写入记忆

```
  memory_write tool:
    path: "memories/my-project.md"
    content: "## Project facts\n- Uses SQLite for persistence\n..."

  → 写入到 ~/.edgecrab/memories/my-project.md
  → 内容在持久化前经过安全检查
  → 下一个会话会自动获取
```

---

## 技能

### 技能 vs 插件

从第一原理出发：

- `skill` 是提示级别的程序性知识。
- `plugin` 是 EdgeCrab 安装和管理的运行时包。

当您需要可复用的指令或技能本地的辅助文件和脚本捆绑包，让智能体通过常规工具使用时，使用技能。当您需要可执行的扩展行为（如工具、钩子、子进程、Python Hermes 兼容性、就绪门控或经过审计的安装/更新生命周期）时，使用插件。

重要重叠：

- 一个插件可以打包一个 `SKILL.md`。
- 这并不使独立技能和插件成为相同的东西。
- 独立技能位于 `~/.edgecrab/skills/` 下，通过 `edgecrab skills ...` 管理。
- 插件位于 `~/.edgecrab/plugins/` 下，通过 `edgecrab plugins ...` 管理。
- 独立技能也可以在目录（如 `scripts/`、`references/`、`templates/` 和 `assets/`）下打包辅助文件。

快速规则：

- 如果制品只需要告诉智能体做什么，将其制作成技能。
- 如果制品需要在运行时让 EdgeCrab 做新的事情，将其制作成插件。

### 布局

```
  ~/.edgecrab/skills/
    git-release/
      SKILL.md            ← 必需
      release-steps.md    ← 通过 read_files frontmatter 引用
    python-test/
      SKILL.md
    my-custom-workflow/
      SKILL.md
```

可以在配置中添加外部技能目录：

```yaml
# ~/.edgecrab/config.yaml
skills:
  external_dirs:
    - /Users/me/shared-skills/
    - /work/team-skills/
```

### 会话启动时加载什么

`PromptBuilder` 包含所有已安装技能的**摘要**（不是完整内容）：

```
  Available skills:
  - git-release: Automated git tag, changelog, and crates.io publish workflow
  - python-test: Run pytest with coverage, lint, and type checking
  - my-custom-workflow: Deploy to staging and run smoke tests
```

完整技能内容在模型调用技能时按需加载（通过 `skill_view`）或当 `preloaded_skills` 配置指定时加载。

独立技能不会创建新的工具、钩子、进程或插件运行时。它们仍然可以携带辅助文件，EdgeCrab 现在会在加载技能时解析 Claude 风格的 `${CLAUDE_SKILL_DIR}` 和 `${CLAUDE_SESSION_ID}` 占位符，但执行仍通过正常的工具界面进行。

### 查看技能

```sh
# 列出所有技能
edgecrab skills list

# 查看特定技能
edgecrab skills view git-release

# 按关键字搜索
edgecrab skills search "deploy"

# 从中心安装
edgecrab skills install docker-build
```

---

## 技能运行时激活

当调用技能时，智能体：
1. 读取 `SKILL.md`（完整内容）
2. 加载 `read_files` frontmatter 中列出的任何文件
3. 作为其正常任务循环的一部分遵循技能的指令

条件激活（来自 frontmatter）：

```yaml
# SKILL.md frontmatter
requires_tools: [terminal, write_file]
requires_toolsets: [coding]
platforms: [linux, windows]
```

如果所需工具不在活动工具集中，技能将从摘要中隐藏 — 它不会出现为模型的建议。

Claude 风格的独立技能包部分兼容：

- EdgeCrab 渲染 `${CLAUDE_SKILL_DIR}` 和 `${CLAUDE_SESSION_ID}`。
- EdgeCrab 加载 `read_files` 并从 `references/`、`templates/`、`scripts/` 和 `assets/` 列出辅助文件。
- EdgeCrab 解析元数据，如 `when_to_use`、`arguments`、`argument-hint`、`allowed-tools`、`user-invocable`、`disable-model-invocation`、`context` 和 `shell`。
- EdgeCrab 不会自动执行 Claude prompt-shell 块或派生子智能体，因为这些是 Claude 运行时语义，而不是可移植的技能包语义。

---

## Honcho 集成

Honcho 是一个独立的云服务，提供用户级记忆和个性化：

```
  会话结束：
    honcho_conclude → 将会话摘要发送到 Honcho API
                    → Honcho 为其索引以便未来的语义搜索

  新会话：
    honcho_context  → 从 Honcho 获取相关的过去经验
                    → 注入到智能体的当前上下文

  显式搜索：
    honcho_search("how did I solve the auth problem?")
    → 对所有索引的过去会话进行语义搜索
```

Honcho 完全是可选的。基于文件的记忆不需要它也能工作。

---

## 提示

> **提示：保持 `MEMORY.md` 简洁。** 它每个会话都会加载并占用 token。
> 每个事实一行比段落更好：
> ```markdown
> - Project: edgecrab, Rust 2024, MSRV 1.86.0
> - Test command: cargo test --workspace
> - Deploy: cargo publish crates in dependency order
> ```

> **提示：使用技能处理带有失败模式的多步骤工作流。**
> 既记录成功路径又记录常见失败情况的技能比只显示成功路径的技能有用 10 倍。

> **提示：`--skill git-release` 预加载技能用于会话，无需 `/slash` 调用。**
> ```sh
> edgecrab --skill git-release "prepare the next minor release"
> ```

---

## 常见问题

**问：记忆文件会影响所有配置文件吗？**
不会。每个配置文件都有自己的 `memories/` 目录。`~/.edgecrab/memories/` 是默认配置文件的记忆。`~/.edgecrab/profiles/work/memories/` 是 `work` 配置文件的记忆。

**问：如果记忆文件损坏或被注入检查拦截会怎样？**
它会跳过并记录 `tracing::warn!` 日志条目。会话在没有该记忆章节的情况下继续正常运行。

**问：智能体可以删除记忆吗？**
可以，通过 `memory_write` 传入空内容或 `skill_manage` 使用 `delete` 操作。没有自动过期机制。

---

## 交叉引用

- 记忆如何加载到提示 → [提示构建器](../003_agent_core/003_prompt_builder.md)
- 记忆内容的安全检查 → [安全](../011_security/001_security.md)
- 技能文件格式 → [创建技能](./002_creating_skills.md)
- 技能工具目录 → [工具目录](../004_tools_system/002_tool_catalogue.md)
