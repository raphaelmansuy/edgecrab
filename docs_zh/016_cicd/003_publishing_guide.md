# 发布指南 🦀

本指南描述了如何发布 EdgeCrab 的新版本。

## 前置条件

- 对 GitHub 仓库的写入权限
- 在本地安装 `cargo-release`
- 在本地安装 `gh` CLI
- 在本地安装 `jq` 和 `yq`

## 版本管理

EdgeCrab 使用语义化版本控制。版本号格式为 `MAJOR.MINOR.PATCH`。

### 版本升级规则

- **MAJOR**: 重大变更，不兼容的 API 更改
- **MINOR**: 新增功能，向后兼容
- **PATCH**: 修复，向后兼容

## 发布流程

### 1. 更新版本号

使用 `cargo-release` 更新所有 crate 的版本号：

```bash
cargo release --dry-run <version>
```

确认更改后，运行：

```bash
cargo release <version>
```

这会：

- 更新所有 crate 的 `Cargo.toml` 版本号
- 更新 crate 之间的依赖版本
- 创建一个 git commit 和 tag

### 2. 更新变更日志

编辑 `CHANGELOG.md`，添加新版本的条目：

```markdown
## [MAJOR.MINOR.PATCH] - YYYY-MM-DD

### Added

- 新增功能描述

### Changed

- 变更描述

### Fixed

- 修复描述
```

### 3. 推送更改

```bash
git push origin main
git push origin <version-tag>
```

### 4. 创建 GitHub Release

使用 `gh` CLI 创建 release：

```bash
gh release create <version-tag> \
  --title "EdgeCrab v<version>" \
  --notes-file CHANGELOG.md
```

## CI/CD 自动构建

GitHub Actions 会自动构建和发布：

1. 当 tag 被推送到 `main` 分支时触发
2. 构建所有目标平台的二进制文件
3. 将二进制文件上传到 GitHub Release
4. 更新 Homebrew tap
5. 更新 AUR package

### 构建目标

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `aarch64-pc-windows-msvc`

## Homebrew Tap 更新

Homebrew tap 位于 `https://github.com/edgecrab/homebrew-tap`。

CI 会自动：

1. 更新 `Formula/edgecrab.rb` 中的版本号和 SHA256
2. 提交并推送到 tap 仓库

## AUR Package 更新

AUR package 位于 `https://aur.archlinux.org/packages/edgecrab-bin`。

CI 会自动：

1. 更新 `PKGBUILD` 中的版本号和 SHA256
2. 更新 `.SRCINFO`
3. 提交并推送到 AUR

## Docker Image

Docker image 发布到 `ghcr.io/edgecrab/edgecrab`。

CI 会自动：

1. 构建 Docker image
2. 标记为 `latest` 和 `<version>`
3. 推送到 GitHub Container Registry

## 验证发布

发布后，验证以下内容：

1. GitHub Release 包含所有平台的二进制文件
2. Homebrew tap 已更新
3. AUR package 已更新
4. Docker image 已发布

### 验证 Homebrew

```bash
brew update
brew upgrade edgecrab
edgecrab --version
```

### 验证 Docker

```bash
docker pull ghcr.io/edgecrab/edgecrab:<version>
docker run ghcr.io/edgecrab/edgecrab:<version> --version
```

### 验证二进制文件

下载并验证每个平台的二进制文件：

```bash
# Linux x86_64
curl -L https://github.com/edgecrab/edgecrab/releases/download/v<version>/edgecrab-v<version>-x86_64-unknown-linux-gnu.tar.gz | tar xz
./edgecrab --version

# macOS x86_64
curl -L https://github.com/edgecrab/edgecrab/releases/download/v<version>/edgecrab-v<version>-x86_64-apple-darwin.tar.gz | tar xz
./edgecrab --version

# Windows x86_64
curl -L https://github.com/edgecrab/edgecrab/releases/download/v<version>/edgecrab-v<version>-x86_64-pc-windows-msvc.zip -o edgecrab.zip
unzip edgecrab.zip
./edgecrab.exe --version
```

## 发布检查清单

- [ ] 更新版本号
- [ ] 更新变更日志
- [ ] 推送更改和 tag
- [ ] 创建 GitHub Release
- [ ] 验证 CI 构建完成
- [ ] 验证所有二进制文件可用
- [ ] 验证 Homebrew tap 更新
- [ ] 验证 AUR package 更新
- [ ] 验证 Docker image 发布
- [ ] 在 CHANGELOG 中添加发布日期

## 紧急发布

对于紧急修复，可以跳过部分步骤：

1. 更新版本号（仅 PATCH）
2. 更新变更日志（仅修复部分）
3. 推送更改和 tag
4. 创建 GitHub Release

其他步骤（Homebrew、AUR、Docker）会自动完成。

## 回滚发布

如果发布有问题：

1. 删除 GitHub Release
2. 删除 git tag
3. 推送删除：`git push origin :<version-tag>`
4. 创建新的修复版本

## 自动化

发布流程可以通过以下方式自动化：

1. 使用 `cargo-release` 的 `--execute` 选项
2. 创建一个发布脚本
3. 使用 GitHub Actions 的 `workflow_dispatch` 触发发布

### 示例发布脚本

```bash
#!/bin/bash
set -e

VERSION=$1

echo "Releasing EdgeCrab v$VERSION"

# 更新版本号
cargo release $VERSION

# 更新变更日志
echo "## [$VERSION] - $(date +%Y-%m-%d)" >> CHANGELOG.md
echo "" >> CHANGELOG.md
echo "### Fixed" >> CHANGELOG.md
echo "- 紧急修复" >> CHANGELOG.md

# 提交更改
git add CHANGELOG.md
git commit -m "chore: update changelog for v$VERSION"

# 推送
git push origin main
git push origin v$VERSION

# 创建 Release
gh release create v$VERSION \
  --title "EdgeCrab v$VERSION" \
  --notes-file CHANGELOG.md

echo "Released EdgeCrab v$VERSION successfully!"
```

## 注意事项

- 确保所有测试通过后再发布
- 确保变更日志清晰且完整
- 确保版本号正确（语义化版本控制）
- 确保二进制文件签名（如果需要）
- 确保 Docker image 包含正确的标签