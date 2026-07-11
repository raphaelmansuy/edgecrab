# 🦀 CI/CD Secrets 设置

> **为什么：** 一个发布 12 个 crates 到 crates.io、两个 SDK 包（npm + PyPI）、一个 Docker 镜像和一个文档站点的 Rust 工作区需要严格的秘密卫生 — 在错误的工作流中放错令牌就是供应链事件。

**来源：** `.github/workflows/`

---

## 工作流清单

| 文件 | 触发条件 | 用途 |
|---|---|---|
| `ci.yml` | 推送 / PR | 构建、测试、clippy、fmt 检查 |
| `release-binaries.yml` | 标签推送（`v*`） | 构建原生二进制文件，上传校验和，发布 GitHub Release |
| `release-rust.yml` | 标签推送（`v*`） | 按依赖顺序发布所有 12 个 crates 到 crates.io |
| `release-node.yml` | 标签推送（`v*`） | 发布 npm 包（JS/TS SDK） |
| `release-python.yml` | 标签推送（`v*`） | 发布 Python SDK 到 PyPI |
| `release-npm-cli.yml` | `release-binaries.yml` 成功完成 | 在二进制文件公开后发布 `edgecrab-cli` npm 包装器 |
| `release-pypi-cli.yml` | `release-binaries.yml` 成功完成 | 在二进制文件公开后发布 `edgecrab-cli` PyPI 包装器 |
| `release-docker.yml` | 标签推送（`v*`） | 构建并推送 Docker 镜像到 GHCR |
| `deploy-site.yml` | 推送到 `main` 触及 `site/` | 构建 Astro 文档站点 → GitHub Pages |

---

## 每个工作流的秘密和环境变量

```
ci.yml
  └──（无秘密 — 隐式使用 GITHUB_TOKEN 只读）

release-rust.yml
  └── CARGO_REGISTRY_TOKEN     (仓库秘密)

release-binaries.yml
  └── GITHUB_TOKEN             (内置 — contents:write 用于发布上传/发布)

release-node.yml
  └── environment: npm
      └── NPM_TOKEN             (环境秘密 — npm 环境)

release-python.yml
  └── environment: pypi
      └── (OIDC 可信发布 — 不需要长期令牌)

release-npm-cli.yml
  └── environment: npm
      └── NPM_TOKEN             (环境秘密 — npm 环境)

release-pypi-cli.yml
  └── environment: pypi
      └── (OIDC 可信发布 — 不需要长期令牌)

release-docker.yml
  └── GITHUB_TOKEN              (内置 — packages:write 权限)

deploy-site.yml
  └── environment: github-pages
      └── GITHUB_TOKEN          (内置 — pages:write + id-token:write)
```

> **提示：** 对 `npm` 和 `pypi` 使用 GitHub **环境**秘密，而不是仓库秘密。环境保护规则添加必需的审查者门控，因此没有工作流可以在未经批准的情况下发布。

---

## Rust Crate 发布顺序

`release-rust.yml` 按严格的依赖顺序发布 crates，并在每次发布之间等待，以便 crates.io 有时间在下一个 crate 依赖它之前索引每个 crate：

```
edgecrab-types
      │
      ▼
edgecrab-security
      │
      ▼
edgecrab-state
      │
      ▼
edgecrab-cron
      │
      ▼
edgecrab-tools
      │
      ▼
edgecrab-lsp
      │
      ▼
edgecrab-core
      │
      ▼
edgecrab-gateway
      │
      ▼
edgecrab-acp
      │
      ▼
edgecrab-migrate
      │
      ▼
edgecrab-cli          ← 最后发布；依赖所有内容
```

此顺序与 [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md) 中的 DAG 匹配。如果添加新 crate，请在此链的正确位置插入。

---

## 设置秘密（新仓库）

### `CARGO_REGISTRY_TOKEN`

1. 登录 [crates.io](https://crates.io)
2. 帐户设置 → API 令牌 → 新建令牌（范围：`publish-new` + `publish-update`）
3. GitHub 仓库 → 设置 → 秘密和变量 → Actions → 新建仓库秘密
4. 名称：`CARGO_REGISTRY_TOKEN`，值：粘贴令牌

### npm OIDC 可信发布（不需要令牌）

1. 登录 [npmjs.com](https://npmjs.com)
2. 对于每个包（`edgecrab-sdk`、`edgecrab-cli`）→ 设置 → 可信发布者 → GitHub Actions
3. 填写：所有者 `raphaelmansuy`，仓库 `edgecrab`，工作流文件名（`release-node.yml` 或 `release-npm-cli.yml`），环境 `npm`
4. GitHub 仓库 → 设置 → 环境 → 确保 `npm` 环境存在（可选必需审查者）
5. 工作流需要 `permissions: id-token: write` — 不需要 `NPM_TOKEN` 秘密

传统回退：作为仓库/环境秘密的自动化令牌 `NPM_TOKEN`（配置 OIDC 时不使用）。

### PyPI OIDC 可信发布（不需要令牌）

1. 登录 [pypi.org](https://pypi.org)
2. 项目 → 发布 → 添加新发布者 → GitHub Actions
3. 填写：仓库所有者、仓库名称、工作流文件名（`release-python.yml`）、环境名称（`pypi`）
4. GitHub 仓库 → 设置 → 环境 → 创建 `pypi` 环境
5. 不需要秘密 — PyPI 通过 OIDC 生成短期令牌

### Docker / GHCR

使用内置的 `GITHUB_TOKEN` 和 `packages: write` 权限。除了工作流 YAML 中的权限声明外，不需要其他设置：

```yaml
permissions:
  contents: read
  packages: write
```

---

## `ci.yml` — 关键检查

```
push / PR
     │
     ├── cargo fmt --check
     ├── cargo clippy -- -D warnings
     ├── cargo test --workspace
     └── cargo build --workspace --release
```

所有四个门控必须通过然后 PR 才能合并。`release-*` 工作流只在版本标签上触发，因此损坏的构建永远不会到达发布步骤。

---

## 提示

- **永远不要将 `CARGO_REGISTRY_TOKEN` 放在环境中 —** 它给每个 crate 发布权限。将其保留为仓库级秘密并限制在 `release-rust.yml` 工作流上，使用 `if: github.ref_type == 'tag'`。
- **发布之间的等待是承重的 —** crates.io 具有最终一致性。如果删除 `sleep` 步骤，下游 crates 将无法解析刚发布的依赖。
- **标签格式很重要 —** 发布工作流使用 `v*` 通配符匹配。名为 `release-1.0` 的标签不会触发它们。

---

## 常见问题

**问：如何进行试运行发布？**
答：本地运行 `cargo publish --dry-run -p edgecrab-types`。CI 工作流不支持试运行模式。

**问：如果 crate 发布在链中间失败怎么办？**
答：工作流不是事务性的。修复故障并从失败的步骤重新运行工作流。`cargo publish` 对于相同版本是幂等的 — 它会跳过已发布的 crates 并发出警告。

**问：我可以在不运行完整链的情况下发布单个 crate 吗？**
答：本地可以。在 CI 中，`release-rust.yml` 工作流总是运行完整链以保持所有 crate 的版本同步。

---

## 交叉引用

- Crate 依赖顺序（为什么这个发布顺序）→ [`002_architecture/002_crate_dependency_graph.md`](../002_architecture/002_crate_dependency_graph.md)
- GitHub Pages 部署 → [`016_cicd/002_github_pages_dns.md`](002_github_pages_dns.md)
