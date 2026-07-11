# 安全系统 🦀

EdgeCrab 采用多层安全架构来保护用户数据和系统资源。

## 安全层次

### 1. 认证层

```yaml
auth:
  enabled: true
  providers:
    - password
    - oauth2
    - api-key
  session_timeout: 1h
  max_login_attempts: 5
  lockout_duration: 15m
```

### 2. 授权层

```yaml
authorization:
  enabled: true
  roles:
    admin:
      permissions:
        - "*"
    user:
      permissions:
        - "chat:read"
        - "chat:write"
        - "tools:execute"
    guest:
      permissions:
        - "chat:read"
```

### 3. 加密层

```yaml
encryption:
  enabled: true
  algorithm: "AES-256-GCM"
  key_rotation: "90d"
  tls:
    enabled: true
    min_version: "TLS 1.3"
```

### 4. 审计层

```yaml
audit:
  enabled: true
  level: "detailed"
  retention: "90d"
  log_file: "~/.edgecrab/logs/audit.log"
```

## 安全特性

### API Key 管理

```rust
pub struct ApiKeyManager {
    keys:        DashMap<String, ApiKey>,
    key_length:  usize,
    max_keys:    usize,
}

pub struct ApiKey {
    id:              String,
    secret:          String,
    created_at:      DateTime<Utc>,
    expires_at:      Option<DateTime<Utc>>,
    permissions:     Vec<String>,
    last_used_at:    Option<DateTime<Utc>>,
    usage_count:     usize,
}
```

#### 创建 API Key

```bash
edgecrab auth api-key create --name "my-key" --permissions "chat:read,tools:execute"
```

#### 列出 API Key

```bash
edgecrab auth api-key list
```

#### 撤销 API Key

```bash
edgecrab auth api-key revoke <key-id>
```

### 密码策略

```yaml
password:
  min_length: 12
  require_uppercase: true
  require_lowercase: true
  require_numbers: true
  require_symbols: true
  max_age: "90d"
  history_size: 5
```

### 会话安全

```rust
pub struct SessionManager {
    sessions:      DashMap<String, Session>,
    timeout:       Duration,
    max_sessions:  usize,
}

pub struct Session {
    id:             String,
    user_id:        String,
    token:          String,
    created_at:     DateTime<Utc>,
    last_access_at: DateTime<Utc>,
    expires_at:     DateTime<Utc>,
}
```

### 输入验证

```rust
pub fn sanitize_input(input: &str) -> Result<String> {
    let sanitized = input
        .replace("<script>", "")
        .replace("</script>", "")
        .replace("javascript:", "");
    
    if sanitized.len() > 10000 {
        return Err(Error::InputTooLong);
    }
    
    Ok(sanitized)
}
```

### 输出过滤

```rust
pub fn filter_output(output: &str) -> String {
    output
        .replace("sk-", "sk-***")
        .replace("Bearer ", "Bearer ***")
        .replace("API_KEY", "***")
}
```

## 安全配置

### 全局安全配置

```yaml
security:
  enabled: true
  audit:
    enabled: true
    level: detailed
  api_key:
    enabled: true
    max_keys: 10
  password:
    enabled: true
    min_length: 12
  tls:
    enabled: true
    certificate: ~/.edgecrab/certs/server.crt
    key: ~/.edgecrab/certs/server.key
```

### 工具安全配置

```toml
[tool.security]
allowed_origins = []
allowed_methods = ["GET", "POST"]
require_auth = true
rate_limit = "100/min"
```

## 安全最佳实践

1. **最小权限原则**：只为用户分配完成任务所需的最小权限
2. **定期轮换密钥**：定期轮换 API Key 和密码
3. **启用审计日志**：记录所有重要操作
4. **使用 HTTPS**：始终使用 TLS 加密通信
5. **验证输入**：对所有用户输入进行验证和清理
6. **过滤输出**：从日志和响应中移除敏感信息
7. **限制速率**：实施速率限制防止滥用
8. **备份数据**：定期备份重要数据

## 安全审计

### 运行安全审计

```bash
edgecrab security audit
```

### 审计报告

```text
Security Audit Report
=====================

Checked: 25 security controls
Passed:  20
Failed:  5
Warning: 3

Failures:
- [HIGH] API Key with full permissions found
- [HIGH] TLS certificate expires in 7 days
- [MEDIUM] Password policy too weak
- [MEDIUM] No rate limiting configured
- [LOW] Audit log not encrypted

Warnings:
- API Key hasn't been rotated in 180 days
- Session timeout exceeds recommended 1 hour
- No 2FA configured for admin accounts
```

### 修复安全问题

```bash
edgecrab security fix <issue-id>
```

## 安全监控

### 实时监控

```yaml
security:
  monitoring:
    enabled: true
    alerts:
      - type: "suspicious_login"
        threshold: 5
        action: "lockout"
      - type: "unauthorized_access"
        threshold: 3
        action: "alert"
      - type: "data_exfiltration"
        threshold: 100
        action: "block"
```

### 告警通知

```yaml
security:
  notifications:
    email:
      enabled: true
      recipients:
        - admin@example.com
    webhook:
      enabled: true
      url: https://api.example.com/alerts
    slack:
      enabled: true
      webhook_url: https://hooks.slack.com/services/xxx
```

## 安全测试

### 单元测试

```bash
cargo test -p edgecrab-core security -- --nocapture
```

### 集成测试

```bash
cargo test -p edgecrab-gateway security -- --nocapture
```

### 渗透测试

```bash
edgecrab security test --mode penetration
```

## 安全更新

### 自动更新

```yaml
security:
  updates:
    auto: true
    check_interval: "24h"
    notify: true
```

### 手动更新

```bash
edgecrab security update
```

## 安全合规

### SOC 2 合规

EdgeCrab 符合 SOC 2 Type II 要求：

- **安全性**：保护系统免受未授权访问
- **可用性**：确保系统可靠运行
- **处理完整性**：确保处理准确、完整和及时
- **保密性**：保护敏感信息
- **隐私**：保护个人信息

### GDPR 合规

- 数据最小化
- 数据主体权利
- 数据保护影响评估
- 数据泄露通知

## 未来计划

- 多因素认证支持
- 硬件安全模块集成
- 零信任架构
- AI 驱动的威胁检测
- 安全开发生命周期集成