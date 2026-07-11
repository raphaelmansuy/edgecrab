# 创建技能 🦀

> **已验证来源：** `crates/edgecrab-tools/src/tools/skills.rs`

---

## 为什么要写技能

技能是最廉价的智能体定制形式。您不需要修改源代码，而是编写一个 Markdown 文件，智能体在运行时读取它。然后智能体作为其正常响应循环的一部分遵循技能的指令。

一个写得好的技能编码了通用智能体和了解 *您的* 项目如何工作的智能体之间的差异。

🦀 *技能是 EdgeCrab 的肌肉记忆。螃蟹学会正确的招式并执行它们，而无需您每次都教它。*

---

## 最小结构

```
  ~/.edgecrab/skills/my-skill/
    SKILL.md
```

一个目录，一个文件。这是完整的要求。如果未提供 `name:` frontmatter，目录名将成为默认技能名称。

您也可以将辅助文件与 `SKILL.md` 一起打包，例如：

```text
~/.edgecrab/skills/my-skill/
  SKILL.md
  scripts/
    helper.py
  references/
    api.md
  templates/
    output.md
```

---

## SKILL.md 格式

```markdown
---
name: my-skill               # 显示名称（可选；默认为目录名）
description: One-line summary for the skills list prompt injection
category: devops             # Groups skills in skills_categories output
version: 1.0.0
license: MIT
platforms:                   # Omit to show on all supported operating systems
  - linux
  - windows
read_files:                  # Additional files loaded when skill is invoked
  - references/release.yml
requires_tools:              # Skill hidden if these tools are absent
  - terminal
  - write_file
requires_toolsets:           # Skill hidden if these toolsets aren't active
  - coding
required_environment_variables:
  - name: GITHUB_TOKEN
    prompt: GitHub token
    help: https://github.com/settings/tokens
when_to_use: Use when preparing a release or validating release state.
arguments:
  - version
  - channel
argument-hint: <version> <channel>
allowed-tools:
  - read_file
  - run_terminal
user-invocable: true
disable-model-invocation: false
context: fork
shell: bash
---

# My Skill

## When to use this skill
(tell the model exactly when this skill is appropriate)

## Prerequisites
(what must be true before starting)

## Workflow
1. Step one
2. Step two
   - important note
3. Step three

## Common failures
- **If X happens**: do Y instead
- **Error "Z not found"**: check that W is configured

## Example
(a concrete example of the workflow in action)
```

---

## Frontmatter 字段参考

| 字段 | 类型 | 效果 |
|---|---|---|
| `name` | string | 列表中的显示名称；默认为目录名称 |
| `description` | string | 注入到系统提示摘要中 |
| `category` | string | 在 `skills_categories` 输出中分组 |
| `version` | string | 在技能视图中显示；无版本强制执行 |
| `license` | string | 仅元数据 |
| `platforms` | list | 如果设置，在其他操作系统上隐藏技能（`darwin`、`linux`、`windows`） |
| `read_files` | list | 调用时随 SKILL.md 加载的相对路径 |
| `requires_tools` | list | 如果所有列出的工具都不可用则隐藏技能 |
| `requires_toolsets` | list | 如果所有列出的工具集不活动则隐藏技能 |
| `required_environment_variables` | list of objects | 环境变量传递 + 缺失凭据的指导 |
| `when_to_use` | string | 当 `description` 缺失时的 Claude 风格回退摘要 |
| `arguments` | list | Claude 风格的声明参数名称；在 `skill_view` 中显示 |
| `argument-hint` | string | Claude 风格的调用提示；在 `skill_view` 中显示 |
| `allowed-tools` | list | Claude 风格的建议性元数据；在 `skill_view` 中显示 |
| `user-invocable` | bool | 当 `false` 时从 `skills_list` 中隐藏 |
| `disable-model-invocation` | bool | 解析并显示；EdgeCrab 不强制执行 |
| `context` | string | 解析并显示；`fork` 不会自动执行 |
| `shell` | string | 解析并显示；prompt-shell 块不会自动执行 |

Frontmatter 是**可选的**。没有 frontmatter 且只有正文文本的 `SKILL.md` 是一个有效的技能。

---

## 编写有效的技能内容

### 做：明确陈述触发条件

```markdown
## When to use
Use this skill when asked to create a new release, bump the version,
publish to crates.io, or update the CHANGELOG.
```

没有这个，模型可能不会激活技能，即使它是合适的。

### 做：显示确切的命令

```markdown
## Steps
1. `cargo test --workspace` — verify all tests pass
2. `git tag v$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')`
3. `git push --tags`
4. Run `cargo publish` for each crate in dependency order
```

### 做：记录失败路径

```markdown
## Failures
- If `cargo publish` returns "crate already exists": increment patch version
- If tests fail on integration tests: check `docker ps` — the test DB may be down
```

### 不做：编写理想化的步骤

如果某个步骤需要模型没有的工具或未配置的服务，请注明前置条件。理想化的步骤会混淆智能体。

---

## 调用技能

```sh
# 从 CLI 标志（在会话开始前预加载）
edgecrab --skill git-release "prepare the 1.2.0 release"

# 在会话内（斜杠命令）
/skills list
/skills view git-release

# 模型可以自己调用技能
#（在系统提示中读取摘要后）：
"Use the git-release skill to create the patch release"
```

---

## `read_files` — 链接文档

技能可以引用随 `SKILL.md` 加载的其他文件：

```yaml
read_files:
  - ../shared/release-checklist.md   # 相对于 SKILL.md
  - /absolute/path/to/runbook.md
```

这些文件在技能被调用时加载（通过 `skill_view`），它们的内容包含在技能正文中。使用此方法可以在引用详细运行手册的同时保持 `SKILL.md` 简洁。

对于 Claude 风格的辅助脚本，EdgeCrab 还支持：

- `${CLAUDE_SKILL_DIR}` → 替换为具体的技能目录
- `${CLAUDE_SESSION_ID}` → 替换为活动的会话 ID

这意味着技能可以安全地引用打包的 CLI 辅助程序，例如：

```markdown
Run `${CLAUDE_SKILL_DIR}/scripts/helper.py --session ${CLAUDE_SESSION_ID}` with the terminal tool.
```

Claude 兼容性边界：

- 支持：技能目录布局、`SKILL.md`、`read_files`、辅助文件发现、`when_to_use` 回退、`${CLAUDE_SKILL_DIR}` 和 `${CLAUDE_SESSION_ID}`。
- 不自动执行：Claude 内联 prompt-shell 展开和派生技能运行时语义。

---

## 从技能中心安装

```sh
# 浏览可用技能
edgecrab skills hub

# 按名称安装技能
edgecrab skills install docker-build

# 安装到 ~/.edgecrab/skills/docker-build/
```

中心是一个精心策划的社区贡献技能集合。同名的本地技能始终优先于中心技能。

---

## 管理技能

```sh
# 列出所有已安装的技能
edgecrab skills list

# 查看技能的完整内容
edgecrab skills view git-release

# 按关键字搜索
edgecrab skills search "deploy"

# 删除技能
edgecrab skills remove old-skill

# 从本地目录路径安装
edgecrab skills install /path/to/my-skill
```

---

## 示例：完整技能

```markdown
---
name: rust-release
description: Publish Rust workspace crates to crates.io in dependency order
category: release
version: 1.0.0
requires_tools: [terminal]
required_environment_variables:
  - name: CARGO_REGISTRY_TOKEN
    prompt: crates.io token
---

# Rust Release

## When to use
When asked to publish, release, or bump the version of any crate in
the edgecrab workspace.

## Prerequisites
- All tests pass: `cargo test --workspace`
- `CARGO_REGISTRY_TOKEN` environment variable is set
- Working directory is the workspace root

## Publish order
Respect the dependency graph. Publish leaf crates first:

1. edgecrab-types
2. edgecrab-security
3. edgecrab-state
4. edgecrab-cron
5. edgecrab-tools
6. edgecrab-core
7. edgecrab-gateway
8. edgecrab-acp
9. edgecrab-migrate
10. edgecrab-cli

Wait 30 seconds between each publish for crates.io to index.

## Commands
```sh
cargo publish -p edgecrab-types
sleep 30
cargo publish -p edgecrab-security
# ... continue
```

## Failures
- `crate already uploaded` → version already exists; bump the version in Cargo.toml
- `401 Unauthorized` → check CARGO_REGISTRY_TOKEN is valid and not expired
```

---

## 提示

> **提示：每个工作流一个技能，而不是每个项目一个技能。**
> `git-release` 技能可跨项目重用。嵌入项目特定细节的 `my-specific-project`
> 技能更难维护和分享。

> **提示：在保存前交互式测试技能。**
> 在会话中手动运行工作流，记录每个边缘情况，然后根据实际发生的情况编写
> 技能 — 而不是您希望发生的事情。

> **提示：使用 `requires_tools` 防止模型在正确工具不可用时读取技能。**
> 需要 `terminal` 但在 `--toolset safe` 会话中显示的技能会浪费提示 token 并
> 混淆模型。

---

## 常见问题

**问：技能可以调用另一个技能吗？**
不能直接调用 — 没有技能调用语法。但模型可以读取技能（通过 `skill_view`）并遵循其指令，这可能包括"follow the
X workflow"引用另一个技能。然后模型将请求该技能。

**问：我应该如何对我的技能进行版本控制？**
frontmatter 中的 `version` 纯粹是信息性的。没有强制执行。
用于您自己的跟踪；运行时在激活目的上忽略它。

**问：技能可以在团队成员之间共享吗？**
可以。将共享目录添加到每个团队成员的
`~/.edgecrab/config.yaml` 中的 `skills.external_dirs`。或者发布到技能中心。

---

## 交叉引用

- 记忆系统概述 → [记忆和技能](./001_memory_skills.md)
- 系统提示中的技能 → [提示构建器](../003_agent_core/003_prompt_builder.md)
- 技能工具（`skill_manage`、`skills_hub`）→ [工具目录](../004_tools_system/002_tool_catalogue.md)
