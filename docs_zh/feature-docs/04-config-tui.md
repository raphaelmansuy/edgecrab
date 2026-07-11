# 配置 TUI 系统 🦀

EdgeCrab 提供一个交互式终端用户界面（TUI）用于配置管理。

## 启动配置 TUI

```bash
edgecrab config
```

或者使用别名：

```bash
edgecrab config edit
```

## TUI 界面

配置 TUI 具有以下布局：

```text
┌─────────────────────────────────────────────────────────────┐
│  EdgeCrab Configuration                                      │
│  ─────────────────────────────────────────────────────────  │
│                                                             │
│  ┌──────────┬────────────────────────────────────────────┐  │
│  │  Model   │  OpenAI (gpt-4o-mini)                      │  │
│  │  Sandbox │  Enabled (strict)                          │  │
│  │  Plugins │  3 enabled, 2 disabled                     │  │
│  │  Skills  │  5 loaded                                  │  │
│  └──────────┴────────────────────────────────────────────┘  │
│                                                             │
│  ┌────────────────────────────────────────────────────────┐  │
│  │  [Model]                                               │  │
│  │  ├─ Provider: openai                                   │  │
│  │  ├─ API Key: ********                                  │  │
│  │  ├─ Base URL: https://api.openai.com/v1               │  │
│  │  ├─ Model: gpt-4o-mini                                │  │
│  │  ├─ Temperature: 0.7                                  │  │
│  │  └─ Max Tokens: 4096                                  │  │
│  │                                                        │  │
│  │  [Sandbox]                                             │  │
│  │  ├─ Enabled: true                                     │  │
│  │  ├─ Network: blocked                                  │  │
│  │  └─ Filesystem: read-only                             │  │
│  └────────────────────────────────────────────────────────┘  │
│                                                             │
│  [Save] [Reset] [Exit]                                      │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 导航

| 按键 | 功能 |
|------|------|
| `↑` / `↓` | 在配置项之间移动 |
| `←` / `→` | 在配置部分之间切换 |
| `Enter` | 编辑当前配置项 |
| `s` | 保存配置 |
| `r` | 重置为默认值 |
| `q` | 退出 |
| `?` | 显示帮助 |

## 配置部分

### Model 部分

```text
[Model]
├─ Provider: openai
├─ API Key: ********
├─ Base URL: https://api.openai.com/v1
├─ Model: gpt-4o-mini
├─ Temperature: 0.7
├─ Max Tokens: 4096
└─ Streaming: true
```

### Sandbox 部分

```text
[Sandbox]
├─ Enabled: true
├─ Network: blocked
├─ Filesystem: read-only
└─ Environment: whitelist
```

### Plugins 部分

```text
[Plugins]
├─ calculator (enabled)
├─ json-toolbox (enabled)
├─ weather (enabled)
├─ telemetry (disabled)
└─ status (disabled)
```

### Skills 部分

```text
[Skills]
├─ release
├─ debugging
├─ documentation
├─ testing
└─ security
```

## 编辑配置

### 选择提供程序

```text
Provider: [openai]
           anthropic
           gemini
           ollama
```

使用 `↑` / `↓` 选择，按 `Enter` 确认。

### 输入 API Key

```text
API Key: [________________]
```

输入完成后按 `Enter`。密码会被隐藏显示为 `*`。

### 切换开关

```text
Enabled: [X]
```

按 `Enter` 切换状态（`[X]` = 启用，`[ ]` = 禁用）。

### 数字输入

```text
Temperature: [0.7]
```

使用键盘输入数字，按 `Enter` 确认。

## 配置验证

TUI 实时验证配置：

- **API Key 格式验证**：确保 API Key 格式正确
- **URL 验证**：确保 Base URL 格式正确
- **数字范围验证**：确保温度在 0-1 之间
- **必填字段检查**：确保必填字段不为空

### 验证错误

```text
API Key: [invalid-key]
         ^^^^^^^^^^^^^^
         Error: API Key must start with 'sk-'
```

## 配置保存

配置保存在 `~/.edgecrab/config.yaml`：

```yaml
model:
  provider: openai
  api_key: env:OPENAI_API_KEY
  base_url: https://api.openai.com/v1
  model: gpt-4o-mini
  temperature: 0.7
  max_tokens: 4096
  streaming: true

sandbox:
  enabled: true
  network: blocked
  fs:
    read_only: true

plugins:
  enabled: true
  auto_enable: true
  disabled: []
```

## 配置管理

### 加载配置

```rust
pub fn load_config() -> Config {
    let path = PathBuf::from("~/.edgecrab/config.yaml");
    let content = fs::read_to_string(&path)?;
    serde_yaml::from_str(&content)?
}
```

### 保存配置

```rust
pub fn save_config(config: &Config) -> Result<()> {
    let path = PathBuf::from("~/.edgecrab/config.yaml");
    let content = serde_yaml::to_string(config)?;
    fs::write(&path, content)?;
    Ok(())
}
```

### 重置配置

```bash
edgecrab config reset
```

### 导出配置

```bash
edgecrab config export > my-config.yaml
```

### 导入配置

```bash
edgecrab config import my-config.yaml
```

## TUI 主题

### 浅色主题

```bash
edgecrab config --theme light
```

### 深色主题

```bash
edgecrab config --theme dark
```

### 自定义主题

```yaml
tui:
  theme:
    background: "#1e1e2e"
    foreground: "#cdd6f4"
    primary: "#89b4fa"
    secondary: "#cba6f7"
    success: "#a6e3a1"
    error: "#f38ba8"
    warning: "#f9e2af"
```

## TUI 快捷键

### 全局快捷键

| 按键 | 功能 |
|------|------|
| `Ctrl+C` | 强制退出 |
| `Ctrl+S` | 保存配置 |
| `Ctrl+R` | 重置配置 |

### 编辑快捷键

| 按键 | 功能 |
|------|------|
| `Backspace` | 删除前一个字符 |
| `Delete` | 删除当前字符 |
| `Home` | 移动到行首 |
| `End` | 移动到行尾 |
| `Tab` | 自动补全 |

## 最佳实践

1. **使用环境变量**：对于敏感信息（如 API Key），使用 `env:` 前缀
2. **定期备份配置**：使用 `edgecrab config export` 备份配置
3. **验证配置**：保存前使用 TUI 的验证功能检查配置
4. **使用版本控制**：将配置文件纳入版本控制（排除敏感信息）
5. **共享配置**：使用 `edgecrab config import/export` 共享配置

## 验证

### 测试 TUI 启动

```bash
edgecrab config --dry-run
```

### 验证配置加载

```bash
edgecrab config validate
```

### 测试配置保存

```bash
edgecrab config save
```

## 未来计划

- 配置模板支持
- 配置对比功能
- 配置历史记录
- 团队配置共享
- 配置加密存储