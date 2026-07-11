# EdgeCrab 发布流程

## 快速开始 — 一条命令

```bash
./scripts/release-version.sh set <version>
```

或通过 GitHub Actions（无需本地工具）：
**Actions → Release — Coordinator → Run workflow → 输入版本号**

两种方法执行完全相同的操作，是每次发布的推荐方式。

规范的发布版本存储在 [`Cargo.toml`](/Users/raphaelmansuy/Github/03-working/edgecrab/Cargo.toml) 的 `[workspace.package].version` 下。
每个发布的包版本都由 `./scripts/release-version.sh` 从该源派生。

---

## 自动执行的操作

推送 `v*.*.*` 标签会并行触发所有下游工作流：

| 工作流 | 发布到 | 运行器 |
|--------|--------|--------|
| `release-binaries.yml` | GitHub Release（5 个原生归档） | ubuntu / macos / windows |
| `release-docker.yml` | `ghcr.io/raphaelmansuy/edgecrab` | ubuntu-latest + ubuntu-24.04-arm（无 QEMU） |
| `release-npm-cli.yml` | npm `edgecrab-cli` | ubuntu-latest |
| `release-pypi-cli.yml` | PyPI `edgecrab-cli` | ubuntu-latest |
| `release-rust.yml` | crates.io `edgecrab-cli` | ubuntu-latest |
| `release-node.yml` | npm `edgecrab`（Node SDK） | ubuntu-latest |
| `release-python.yml` | PyPI `edgecrab`（Python SDK） | ubuntu-latest |

二进制归档首先构建；npm/pip 包装器在安装时延迟下载它们，因此工作流之间没有顺序依赖关系。

对于手动重新运行，通过 `workflow_dispatch` 传递确切的标签。
发布工作流现在显式检出该标签，因此针对 `vX.Y.Z` 的重新运行会重新构建标记的源代码，而不是移动的 `main` 分支。

---

## 版本权威

所有发布自动化现在将 [`Cargo.toml`](/Users/raphaelmansuy/Github/03-working/edgecrab/Cargo.toml) 中的工作区版本视为唯一的事实来源。
派生的包版本由 [`scripts/release-version.sh`](/Users/raphaelmansuy/Github/03-working/edgecrab/scripts/release-version.sh) 同步，CI 拒绝偏差。

| 文件 | 字段 |
|------|------|
| `Cargo.toml` | 规范的 `[workspace.package] version` |
| `sdks/node/package.json` | 派生的 `"version"` |
| `sdks/npm-cli/package.json` | 派生的 `"version"` |
| `sdks/pypi-cli/edgecrab_cli/_version.py` | 派生的 `__version__` |
| `sdks/pypi-cli/pyproject.toml` | 动态版本源（`edgecrab_cli._version.__version__`） |
| `sdks/python/pyproject.toml` | 派生的 `version` |

### 命令

```bash
./scripts/release-version.sh print
./scripts/release-version.sh sync
./scripts/release-version.sh check
./scripts/release-version.sh set <version>
```

> npm CLI 包装器从 `package.json` 派生其二进制标签，而 PyPI
> CLI 包装器从 `edgecrab_cli._version.__version__` 派生包元数据和二进制标签。这些文件是派生状态，不是独立的发布权威。

---

## 分步指南（手动回退）

如果无法使用脚本或协调器工作流：

```bash
# 1. 确保 main 分支干净且最新
git checkout main && git pull

# 2. 升级规范版本并同步所有派生的包元数据
VERSION=<version>

./scripts/release-version.sh set "$VERSION"
./scripts/release-version.sh check

# 3. 提交、标记、推送 — 让 release-version.sh sync 处理所有派生文件
git add Cargo.toml \
        sdks/npm-cli/package.json \
        sdks/node/package.json sdks/node/package-lock.json \
        sdks/pypi-cli/edgecrab_cli/_version.py \
        sdks/python/pyproject.toml sdks/python/edgecrab/_version.py
git commit -m "chore: bump version to $VERSION"
git tag "v$VERSION"
git push origin main
git push origin "v$VERSION"
```

---

## 发布后

crates.io 工作流按依赖顺序发布 crates，并在依赖发布之间保持有意的传播延迟。它探测确切的 `crates.io/api/v1/crates/<crate>/<version>` 端点并设置硬超时，然后在可见性后保持短暂的稳定缓冲区，以便我们不会比注册表传播更快地发布。如果 crates.io 保持缓慢，工作流会回退到有限的发布重试，而不是无限期挂起。

### 更新 Homebrew 公式

一旦二进制文件在 GitHub Release 上上线，首选路径是自动化的 `release-homebrew-tap.yml` 工作流。它下载 `edgecrab-checksums.txt`，更新 `raphaelmansuy/homebrew-tap`，并使用 **GitHub App**（推荐）或 `HOMEBREW_TAP_PUSH_TOKEN`（传统 PAT）推送公式更改。

**🔒 推荐：使用 GitHub App（参见 [Homebrew Tap 认证](#-homebrew-tap-authentication-security-best-practice) 部分）**

GitHub App 方法更安全，因为：
- 令牌是短期的，权限范围最小化
- 每个工作流运行自动生成令牌
- 不在仓库中存储持久机密
- 更好的审计追踪
- 需要时可立即撤销

**设置 GitHub App：**
1. 创建一个具有 tap 仓库 `contents: write` 权限的 GitHub App
2. 生成并存储私钥和应用 ID 作为机密
3. 更新工作流以使用 `actions/create-github-app-token@v1`

有关详细设置说明，请参阅下面的 **[Homebrew Tap 认证](#-homebrew-tap-authentication-security-best-practice)** 部分。

**如果使用传统 PAT 方法（不推荐）：**

如果无法使用 GitHub App，可以在仓库机密中配置 `HOMEBREW_TAP_PUSH_TOKEN`，但这应该只是临时解决方案。遵循 [Fallback](#fallback-if-using-github-pat) 部分中记录的安全实践

```bash
gh release download "v${VERSION}" \
  --repo raphaelmansuy/edgecrab \
  --pattern edgecrab-checksums.txt
cat edgecrab-checksums.txt

# 下载两个 macOS 归档并计算 SHA256
ARM_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-aarch64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')
X86_SHA=$(curl -sL https://github.com/raphaelmansuy/edgecrab/releases/download/v${VERSION}/edgecrab-x86_64-apple-darwin.tar.gz | shasum -a 256 | awk '{print $1}')

echo "ARM SHA256:   $ARM_SHA"
echo "x86_64 SHA256: $X86_SHA"
```

然后使用以下命令更新公式：

```bash
./scripts/update-homebrew-formula.sh \
  /path/to/homebrew-tap/Formula/edgecrab.rb \
  "$VERSION" \
  "$ARM_SHA" \
  "$X86_SHA"
```

验证差异后提交并推送 tap 仓库。

### 验证所有安装方法

```bash
# Docker（在 Apple Silicon 上应拉取 arm64 镜像）
docker pull ghcr.io/raphaelmansuy/edgecrab:latest
docker run --rm --entrypoint /bin/sh ghcr.io/raphaelmansuy/edgecrab:latest -lc 'which edgecrab && edgecrab --version'

# npm（全新安装，无缓存）
npm install -g edgecrab-cli
which edgecrab
edgecrab --version

# pip（Python SDK）
pip install --force-reinstall edgecrab
python -c "import edgecrab; print('edgecrab SDK ok')"

# pip（CLI 包装器）
pip install --force-reinstall edgecrab-cli
which edgecrab
edgecrab --version

# cargo
cargo install edgecrab-cli --locked --force
which edgecrab
edgecrab --version

# Homebrew
brew upgrade edgecrab
which edgecrab
edgecrab --version
```

如果 Homebrew 仍然落后，而 npm、PyPI、crates.io 和 Docker 都是最新的，那么 tap 同步就是缺失的步骤。

---

## 必需的机密 / 环境

| 机密 | 位置 | 被使用 | 类型 |
|------|------|--------|------|
| `NPM_TOKEN` | `npm` 环境 | `release-npm-cli.yml` | npm Bearer token |
| `CARGO_REGISTRY_TOKEN` | 仓库机密 | `release-rust.yml` | Cargo API token |
| PyPI OIDC 可信发布者 | `pypi` 环境 | `release-pypi-cli.yml` | OIDC 联合凭证 |
| `PYPI_API_TOKEN` | `pypi` 环境 + 仓库机密 | `release-python.yml` | PyPI API token — 当 OIDC 可信发布者尚未为新项目名称注册时需要（例如 `edgecrab` 的首次发布）；一旦项目在 PyPI 上存在，OIDC 将接管 `release-pypi-cli.yml` |
| `HOMEBREW_TAP_DEPLOY_KEY` | 仓库机密 | `release-homebrew-tap.yml` | 具有对 `raphaelmansuy/homebrew-tap` 写入权限的 ed25519 SSH 部署密钥（密钥 ID 148386829）；**自 v0.4.1 起的主要认证方法** |
| `HOMEBREW_TAP_PUSH_TOKEN` | 仓库机密 | `release-homebrew-tap.yml` | **已弃用** — 传统 GitHub PAT；被 `HOMEBREW_TAP_DEPLOY_KEY` 取代 |
| `GITHUB_TOKEN` | 自动配置 | 所有工作流 | GitHub Actions 自动令牌 |

### Homebrew Tap 认证 — 当前设置（v0.4.1+）

Tap 通过存储为 `HOMEBREW_TAP_DEPLOY_KEY` 的 **SSH 部署密钥**自动更新。

**工作原理：**
1. `release-homebrew-tap.yml` 在运行时解码密钥（`base64 -d`）并将其添加到 `ssh-agent`
2. 通过 SSH 克隆 `raphaelmansuy/homebrew-tap`，更新公式，并推送
3. 工作流中的三层回退：部署密钥 → GitHub App 令牌（如果配置）→ PAT → 跳过（非致命）

**关键细节：**
- 密钥 ID：`148386829`，类型：`ed25519`
- 授权范围：仅对 `raphaelmansuy/homebrew-tap` 的写入权限
- 存储方式：作为仓库机密 `HOMEBREW_TAP_DEPLOY_KEY` 中的 base64 编码 PEM

**轮换部署密钥：**
```bash
# 1. 生成新的 ed25519 密钥对
ssh-keygen -t ed25519 -C "edgecrab-homebrew-deploy" -f homebrew_deploy_key -N ""

# 2. 将公钥添加到 raphaelmansuy/homebrew-tap → Settings → Deploy keys
#    标题：edgecrab-homebrew-deploy，允许写入权限：✅

# 3. 将 base64 编码的私钥存储为仓库机密
gh secret set HOMEBREW_TAP_DEPLOY_KEY -R raphaelmansuy/edgecrab < <(base64 < homebrew_deploy_key)

# 4. 删除本地密钥文件
rm homebrew_deploy_key homebrew_deploy_key.pub

# 5. 从 raphaelmansuy/homebrew-tap → Settings → Deploy keys 中删除旧部署密钥
```

#### 回退：GitHub App（可选升级）

如果想要短期应用令牌而不是静态部署密钥，可以配置在 tap 仓库上具有 `contents: write` 的 GitHub App。将 `HOMEBREW_TAP_APP_ID` 和 `HOMEBREW_TAP_APP_PRIVATE_KEY` 存储为仓库机密，然后更新工作流以使用 `actions/create-github-app-token@v1`。部署密钥方法更简单，目前足够。

#### 回退：GitHub PAT（已弃用）

`HOMEBREW_TAP_PUSH_TOKEN`（细粒度 PAT，对 `raphaelmansuy/homebrew-tap` 的 `contents: write`，90 天有效期）仍作为最后手段检查，但不建议使用。一旦确认部署密钥工作正常，将其移除。

---

## 经验教训

### PyPI 包命名：首次发布需要令牌

**背景（v0.4.1）：** Python SDK 最初以 `edgecrab-sdk` 发布到 PyPI。这意味着 `pip install edgecrab` 总是失败 — 项目 `edgecrab` 在 PyPI 上不存在。

**修复：** 在 `sdks/python/pyproject.toml` 中将包从 `edgecrab-sdk` 重命名为 `edgecrab`。

**陷阱 — OIDC 无法创建全新项目：** PyPI OIDC 可信发布者按项目名称注册。当 `edgecrab` 还不存在于 PyPI 上时，工作流的 OIDC 凭证（已注册用于 `edgecrab-sdk`）无法创建它。解决方案：

1. 本地构建：`cd sdks/python && python3 -m build --outdir dist/`
2. 使用 PyPI API 令牌上传（存储在 `~/.pypirc`）：`python3 -m twine upload dist/*`
3. 一旦项目存在，未来的 CI 通过 `PYPI_API_TOKEN` 机密发布就可以正常工作。
4. 最终在 pypi.org 上为 `edgecrab` 注册 OIDC 可信发布者以允许无密码 CI。

**注意：** `edgecrab-sdk` 仍然存在于 PyPI 上（0.4.1）且无法删除，但所有未来版本将仅以 `edgecrab` 发布。Node.js SDK 在 npm 上仍然是 `edgecrab-sdk`（有意的）。

---

### 版本同步：始终使用 `release-version.sh sync`

**背景（v0.4.1）：** 一个热修复提交手动升级了大多数版本文件，但遗漏了 `sdks/pypi-cli/edgecrab_cli/_version.py`，导致 PyPI CLI 工作流的 CI 失败。

**规则：** 切勿手动编辑单个文件中的版本字符串。始终运行：

```bash
./scripts/release-version.sh sync
```

这是从 `Cargo.toml` 更新所有派生版本的**唯一**权威方式。如果您已经提交但未同步，请运行 sync 并在标记前将其作为 amend 或 fixup 提交添加。

---

### Homebrew tap：部署密钥是当前的主要认证

**背景：** tap 最初配置了 GitHub PAT（`HOMEBREW_TAP_PUSH_TOKEN`），它过期了，导致 v0.3.4 tap 更新被静默跳过。当时的 `RELEASING.md` 将 GitHub App 记录为"推荐"路径，但实际上两者都没有配置。

**当前设置（v0.4.1+）：** 具有对 `raphaelmansuy/homebrew-tap` 写入权限的 ed25519 SSH 部署密钥（`HOMEBREW_TAP_DEPLOY_KEY`）作为仓库机密存储。这是实际运行的方式。

`release-homebrew-tap.yml` 工作流按顺序尝试：部署密钥 → GitHub App 令牌 → PAT → 跳过。

---

EdgeCrab 遵循 [语义版本控制](https://semver.org)：

- **PATCH**（`0.1.x`）— bug 修复、依赖更新、文档
- **MINOR**（`0.x.0`）— 新功能、向后兼容的更改
- **MAJOR**（`x.0.0`）— 破坏性 CLI / 配置 / API 更改

---

## 发布清单摘要

推送发布标签后，所有工作流应在 10-15 分钟内完成。以下是预期状态：

| 工作流 | 状态 | 说明 |
|--------|------|------|
| **Release — Native Binaries** | ✅ Success | 5 个原生归档（macOS arm64/x86_64、Linux、Windows） |
| **Release — Docker (GHCR)** | ✅ Success | 发布到 `ghcr.io/raphaelmansuy/edgecrab:vX.Y.Z` |
| **Release — Node.js (npm)** | ✅ Success | 发布到 npm 注册表 |
| **Release — Python (PyPI)** | ✅ Success | 发布到 PyPI |
| **Release — Rust (crates.io)** | ✅ Success | 按依赖顺序发布工作区 crates |
| **Release — npm CLI (edgecrab-cli)** | ✅ Success | 二进制文件的 npm 包装器 |
| **Release — PyPI CLI (edgecrab-cli)** | ✅ Success | 二进制文件的 PyPI 包装器 |
| **Release — Homebrew Tap** | ⚠️ 如果令牌缺失则手动 | 参见上面的 [配置机密](#configuring-the-secret) |

### 工作流失败时怎么办

1. **检查 GitHub Actions 日志** — 点击 Actions 选项卡中的红色/黄色工作流
2. **常见失败：**
   - `HOMEBREW_TAP_PUSH_TOKEN` 缺失或无效 → 迁移到 GitHub App（推荐）
   - Homebrew tap 工作流上的 `Insufficient permissions` → 验证 GitHub App 在 tap 仓库上具有 `contents: write`
   - crates.io 超时 → 通常是暂时的；手动重试通常成功
   - 二进制构建失败 → 检查日志中的编译错误；修复并重新标记
3. **重新运行失败的工作流** — 使用 `workflow_dispatch` 重播相同的标签
4. **手动发布步骤** — 每个工作流在此文件中都有记录的回退过程

### 示例：v0.3.4 发布（2026-04-12）

所有关键发布工作流成功：
- ✅ 原生二进制文件构建并发布到 GitHub Release
- ✅ Docker 镜像构建并发布到 GHCR
- ✅ npm 包发布
- ✅ PyPI 包发布
- ✅ Rust crates.io 发布完成
- ⚠️ Homebrew Tap 由于缺少认证失败

**Homebrew Tap 失败原因：**
工作流配置为使用 `HOMEBREW_TAP_PUSH_TOKEN`（传统 GitHub PAT），但未在仓库机密中配置。工作流干净退出，显示：
```
HOMEBREW_TAP_PUSH_TOKEN is not configured; automatic tap push cannot proceed.
```

**解决方案：**
为未来发布修复此问题，迁移到 **GitHub App** 方法（推荐）：
1. 遵循 [Homebrew Tap 认证](#-homebrew-tap-authentication-security-best-practice) 中的设置步骤
2. 更新 `release-homebrew-tap.yml` 工作流以使用 `actions/create-github-app-token@v1`
3. 使用手动工作流调度测试

**GitHub App 迁移后：**
未来发布将自动更新 Homebrew 公式，无需手动干预。

**目前（如果需要手动）：**
`raphaelmansuy/homebrew-tap` 中的 Homebrew 公式可以使用上面记录的回退过程手动更新。所有其他分发渠道已上线可用。