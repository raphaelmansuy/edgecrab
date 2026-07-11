# 🦀 GitHub Pages 和 DNS

> **为什么：** 存储在仓库中的文档在每次合并到 `main` 时自动部署 — 无需手动上传，没有托管的旧副本，代码和文档之间没有分歧。

**来源：** `.github/workflows/deploy-site.yml`, `site/public/CNAME`, `site/astro.config.mjs`

---

## 部署流程

```
推送到 main
(触及 site/)
      │
      ▼
┌─────────────────────┐
│  deploy-site.yml    │  GitHub Actions 工作流
│  被触发              │
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  pnpm install       │  安装 Astro 依赖
│  pnpm build         │  输出 → site/dist/
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  actions/upload-    │  将 site/dist/ 打包为
│  pages-artifact     │  Pages 制品
└──────────┬──────────┘
           │
           ▼
┌─────────────────────┐
│  actions/deploy-    │  部署到 github-pages
│  pages              │  环境
└──────────┬──────────┘
           │
           ▼
  自定义域名提供站点
  (CNAME → GitHub Pages CDN)
```

---

## 工作流权限

```yaml
# deploy-site.yml
permissions:
  contents: read
  pages: write        # 必需以上传 Pages 制品
  id-token: write     # 必需用于基于 OIDC 的 Pages 部署
```

`github-pages` 环境必须在第一次部署之前存在于仓库设置中。如果仓库启用了 Pages，GitHub 会在通过 Actions 进行第一次成功的 Pages 部署时自动创建它。

---

## 必须保持同步的文件

| 文件 | 用途 | 如果错误会发生什么 |
|---|---|---|
| `site/public/CNAME` | 告诉 GitHub Pages 自定义域名 | Pages 回退到 `<org>.github.io/<repo>` URL |
| `site/astro.config.mjs` → `site` 字段 | Astro 使用此进行路径生成 | 如果主机名与 CNAME 不匹配，内部链接断裂 |
| DNS → CNAME 记录 | 将自定义域名指向 GitHub CDN | 自定义域名上站点不可达 |

---

## DNS 设置

GitHub Pages 需要以下之一：

```
# 根域名 (example.com)
@ → 185.199.108.153
@ → 185.199.109.153
@ → 185.199.110.153
@ → 185.199.111.153

# 子域名 (docs.example.com)
docs → CNAME → <org>.github.io
```

更新 DNS 后：
1. 仓库 → 设置 → Pages → 验证自定义域名
2. 启用"强制 HTTPS"（DNS 传播后可用）

> **提示：** DNS 传播可能需要最多 48 小时。即使 DNS 仍在传播，`deploy-site.yml` 工作流也会成功 — `site/public/` 中的 CNAME 对 GitHub 端很重要。

---

## Astro 配置检查清单

```js
// site/astro.config.mjs  — 最少必需字段
export default defineConfig({
  site: 'https://your-custom-domain.com',  // 必须与 CNAME 匹配
  output: 'static',
});
```

如果 `site` 错误，Astro 会生成错误的规范 URL，并且 sitemap 指向错误的域名。

---

## 操作检查清单

| 任务 | 负责人 |
|---|---|
| `site/public/CNAME` 与实际自定义域名匹配 | 仓库维护者 |
| DNS CNAME/A 记录指向 GitHub Pages IP | DNS 管理员 |
| 仓库设置中存在 `github-pages` 环境 | 仓库管理员 |
| 工作流具有 `pages: write` + `id-token: write` 权限 | 在 YAML 中检查 |
| `site/astro.config.mjs` 的 `site` 字段与 CNAME 匹配 | 开发者 |

---

## 常见问题

**问：工作流成功但站点显示旧内容。**
答：GitHub Pages CDN 有短缓存。等待 2-3 分钟并硬刷新。如果仍然过时，检查制品上传步骤是否上传了正确的 `dist/` 目录。

**问：我收到"Page build failed"错误。**
答：这通常意味着 `github-pages` 环境不存在或仓库未启用 Pages。转到设置 → Pages → 启用"GitHub Actions"作为源。

**问：我可以在推送前本地预览站点吗？**
答：`cd site && pnpm dev` — Astro 启动本地开发服务器。不需要配置自定义域名。

**问：如何向站点添加新的文档页面？**
答：将 `.md` 或 `.astro` 文件添加到 `site/src/content/`（或 `site/src/pages/`）。下一次触及 `site/` 的 `main` 推送会自动触发重新部署。

---

## 交叉引用

- `github-pages` 环境的 CI/CD 秘密 → [`016_cicd/001_secrets_setup.md`](001_secrets_setup.md)
- 文档索引 → [`INDEX.md`](../INDEX.md)
