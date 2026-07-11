# Homebrew Tap 自动更新设置

EdgeCrab 使用 GitHub Actions 工作流，在每次发布新版本时自动更新 [raphaelmansuy/homebrew-tap](https://github.com/raphaelmansuy/homebrew-tap) 的 `Formula/edgecrab.rb` 文件。

按优先级顺序支持三种认证方法：

| 选项 | 所需密钥 | 属性 |
|--------|-------------------|------------|
| **A — SSH 部署密钥** | `HOMEBREW_TAP_DEPLOY_KEY` | 范围最窄，无过期，已配置 |
| **B — GitHub App** | `HOMEBREW_TAP_APP_ID` + `HOMEBREW_TAP_APP_PRIVATE_KEY` | 短生命周期令牌，无长生命周期密钥 |
| **C — 细粒度 PAT** | `HOMEBREW_TAP_PUSH_TOKEN` | 最简单，但为长生命周期凭据 |

**选项 A（部署密钥）已配置** — 见下方设置方式。

---

## 选项 A — SSH 部署密钥（推荐，已激活）

GitHub App 为每个工作流运行生成一个短生命周期的安装令牌（约1小时）。除了 App 的私钥外，没有任何令牌存储在密钥中，这是标准操作凭据而非用户凭据。

### 设置方式

使用 `gh` CLI 创建并配置了部署密钥：

```bash
# 1. 生成 ed25519 密钥对（无密码）
ssh-keygen -t ed25519 -C "edgecrab-ci-homebrewtap" -f /tmp/edgecrab_tap_deploy -N ""

# 2. 将公钥安装为 tap 仓库的写入权限部署密钥
gh api -X POST /repos/raphaelmansuy/homebrew-tap/keys \
  -f title="edgecrab-ci-homebrewtap" \
  -f key="$(cat /tmp/edgecrab_tap_deploy.pub)" \
  -F read_only=false

# 3. 将私钥存储为 edgecrab 的密钥
gh secret set HOMEBREW_TAP_DEPLOY_KEY \
  --body "$(cat /tmp/edgecrab_tap_deploy)" \
  -R raphaelmansuy/edgecrab

# 4. 删除临时密钥文件（私钥现在仅在 GitHub 密钥中）
rm -f /tmp/edgecrab_tap_deploy /tmp/edgecrab_tap_deploy.pub
```

部署密钥（id `148386829`）可见于：
<https://github.com/raphaelmansuy/homebrew-tap/settings/keys>

密钥（仅名称）可见于：
<https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>

### 轮换部署密钥

再次运行上述4个命令，然后从 tap 仓库中删除旧密钥：

```bash
# 列出密钥 ID
gh api /repos/raphaelmansuy/homebrew-tap/keys --jq '.[] | [.id,.title] | @tsv'
# 删除旧密钥
gh api -X DELETE /repos/raphaelmansuy/homebrew-tap/keys/<OLD_ID>
```

---

## 选项 B — GitHub App（无长生命周期令牌）

1. 转到 <https://github.com/settings/apps/new>（个人账户）或
   Settings → Developer Settings → GitHub Apps（组织）。
2. 设置：
   - **应用名称**：`edgecrab-tap-bot`（或任何您喜欢的名称）
   - **主页 URL**：`https://github.com/raphaelmansuy/edgecrab`
   - **Webhook**：取消勾选 **Active**（不需要 webhook）
3. 在 **Repository permissions** 下，设置 **Contents** → **Read and write**。
4. 在 **Where can this GitHub App be installed?** 下选择 **Only on this account**。
5. 点击 **Create GitHub App**。

### 2. 在 tap 仓库上安装 App

1. 在 App 设置页面，点击 **Install App**。
2. 选择您的账户 → 选择 **Only select repositories** → 选择
   `raphaelmansuy/homebrew-tap`。
3. 点击 **Install**。

### 3. 生成私钥

1. 在 App 设置页面，滚动到 **Private keys**。
2. 点击 **Generate a private key**。下载 `.pem` 文件。
3. 安全保存此文件 — 这是唯一副本。

### 4. 向 `edgecrab` 仓库添加密钥

在 <https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>：

| 密钥名称 | 值 |
|-------------|-------|
| `HOMEBREW_TAP_APP_ID` | App 设置页面上显示的数字 App ID（例如 `123456`） |
| `HOMEBREW_TAP_APP_PRIVATE_KEY` | 下载的 `.pem` 文件的完整内容 |

将整个 PEM 文件（包括 `-----BEGIN RSA PRIVATE KEY-----` 头部和尾部）粘贴为密钥值。

### 5. 验证

通过 <https://github.com/raphaelmansuy/edgecrab/actions/workflows/release-homebrew-tap.yml> 手动触发 **Release — Homebrew Tap** 工作流的运行。**Generate GitHub App token** 步骤应成功，提交应出现在 [raphaelmansuy/homebrew-tap](https://github.com/raphaelmansuy/homebrew-tap/commits/master) 中。

---

## 选项 C — 细粒度个人访问令牌（备用）

如果您不想创建 GitHub App，请使用此选项。

### 1. 创建细粒度 PAT

1. 转到 <https://github.com/settings/tokens?type=beta>。
2. 点击 **Generate new token**。
3. 设置：
   - **令牌名称**：`edgecrab-homebrewtap-push`
   - **过期时间**：设置提醒 — 细粒度 PAT 最多1年后过期。
   - **资源所有者**：您的个人账户
   - **仓库访问**：**Only select repositories** → `homebrew-tap`
   - **权限**：**Contents** → **Read and write**
4. 点击 **Generate token** 并复制值。

### 2. 添加密钥

在 <https://github.com/raphaelmansuy/edgecrab/settings/secrets/actions>：

| 密钥名称 | 值 |
|-------------|-------|
| `HOMEBREW_TAP_PUSH_TOKEN` | 您刚刚创建的细粒度 PAT |

---

## 优先级

工作流按以下顺序检查凭据：

1. `HOMEBREW_TAP_DEPLOY_KEY` → SSH 部署密钥（首选，已配置）
2. `HOMEBREW_TAP_APP_ID` + `HOMEBREW_TAP_APP_PRIVATE_KEY` → GitHub App 令牌
3. `HOMEBREW_TAP_PUSH_TOKEN` → 细粒度 PAT
4. 未配置任何 → 作业记录通知并干净退出（发布继续）

---

## 为什么未配置 Homebrew 时发布不会失败

Homebrew 是一个便利的分发渠道；它不是发布的门控。如果未配置推送凭据，作业会发出 GitHub Actions 通知注解并以代码0退出，因此发布管道是绿色的。您可以随时配置凭据，然后手动重新触发工作流。

---

## 故障排除

| 症状 | 原因 | 修复 |
|---------|-------|-----|
| `Generate GitHub App token` 步骤失败，显示 `Not found` | App 未安装在 `homebrew-tap` 上 | 在 tap 仓库上重新安装 App（上面的步骤2） |
| `Generate GitHub App token` 步骤失败，显示 `Bad credentials` | 私钥错误或 App ID 不匹配 | 重新生成私钥并重新添加两个密钥 |
| `Commit and push` 失败，返回 403 | PAT 没有写入权限或已过期 | 检查令牌过期时间和仓库权限范围 |
| Formula 已经是最新版本 | 无操作；正确版本已在 tap 中 | 无需修复 |