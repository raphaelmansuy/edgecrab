# 工具、MCP 和工件系统 🦀

EdgeCrab 支持多种工具扩展机制：原生工具、MCP 工具服务器和工件处理。

## 工具系统

### 原生工具

原生工具直接用 Rust 实现：

```rust
use edgecrab_core::tool::{Tool, ToolResult};

#[derive(Debug, Clone)]
pub struct CalculatorTool;

impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Perform basic arithmetic operations"
    }

    fn arguments(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "subtract", "multiply", "divide"]
                },
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["operation", "a", "b"]
        })
    }

    async fn execute(&self, args: &serde_json::Value) -> ToolResult {
        let operation = args["operation"].as_str().unwrap();
        let a = args["a"].as_f64().unwrap();
        let b = args["b"].as_f64().unwrap();

        let result = match operation {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            "divide" => a / b,
            _ => return Err(ToolError::InvalidArgument("operation")),
        };

        Ok(serde_json::json!({ "result": result }))
    }
}
```

### 工具注册

```rust
use edgecrab_core::tool_registry::ToolRegistry;

let mut registry = ToolRegistry::new();
registry.register(Box::new(CalculatorTool));
```

### 工具发现

```bash
edgecrab tools list
```

### 工具执行

```bash
edgecrab tools call calculator '{"operation": "add", "a": 5, "b": 3}'
```

## MCP 工具服务器

MCP（Model Context Protocol）是一种用于扩展 AI 模型能力的协议。

### MCP 服务器协议

```text
┌─────────────────────────────────────────────────────────────┐
│                    MCP Protocol Flow                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  Client                          Server                     │
│    │                                │                        │
│    │── initialize ────────────────>│                        │
│    │<─ initialized ────────────────│                        │
│    │                                │                        │
│    │── tools/list ────────────────>│                        │
│    │<─ [tool1, tool2, ...] ────────│                        │
│    │                                │                        │
│    │── tools/call tool1 args ─────>│                        │
│    │<─ result ─────────────────────│                        │
│    │                                │                        │
│    │── shutdown ──────────────────>│                        │
│    │<─ shutdown_ack ───────────────│                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### MCP 服务器配置

```yaml
mcp:
  servers:
    - name: "calculator"
      path: "./calculator-mcp"
      env:
        - CALCULATOR_API_KEY=xxx
    - name: "weather"
      path: "./weather-mcp"
      timeout: 30s
```

### MCP 服务器生命周期

```rust
pub struct McpServer {
    name:       String,
    path:       PathBuf,
    process:    Option<Child>,
    stdin:      Option<ChildStdin>,
    stdout:     Option<ChildStdout>,
    timeout:    Duration,
}

impl McpServer {
    pub async fn start(&mut self) -> Result<()> {
        self.process = Some(Command::new(&self.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?);
        self.initialize().await
    }

    pub async fn stop(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            process.kill().ok();
            process.wait().ok();
        }
        Ok(())
    }
}
```

### MCP 工具注册

当 MCP 服务器启动时，其工具会自动注册到工具注册表：

```rust
async fn register_mcp_tools(registry: &mut ToolRegistry, server: &McpServer) -> Result<()> {
    let tools = server.list_tools().await?;
    for tool in tools {
        registry.register(Box::new(McpToolAdapter::new(server.clone(), tool)));
    }
    Ok(())
}
```

## 工件系统

工件是工具执行产生的数据产物。

### 工件类型

| 类型 | 描述 | 扩展名 |
|------|------|--------|
| `file` | 普通文件 | 任意 |
| `image` | 图像文件 | .png, .jpg, .svg |
| `document` | 文档文件 | .pdf, .docx, .md |
| `data` | 数据文件 | .json, .csv, .xml |
| `code` | 代码文件 | .rs, .py, .js |
| `archive` | 归档文件 | .zip, .tar, .gz |

### 工件存储

```yaml
artifacts:
  storage:
    type: "local"
    path: "~/.edgecrab/artifacts"
    max_size: "1GB"
    retention: "30d"
  upload:
    enabled: true
    max_file_size: "50MB"
```

### 工件管理

```bash
# 列出工件
edgecrab artifacts list

# 获取工件
edgecrab artifacts get <artifact-id>

# 删除工件
edgecrab artifacts delete <artifact-id>

# 上传工件
edgecrab artifacts upload <file-path>

# 下载工件
edgecrab artifacts download <artifact-id> <output-path>
```

### 工件引用

工具可以返回工件引用：

```json
{
  "result": {
    "report": {
      "type": "artifact",
      "id": "report-123",
      "name": "security-audit.pdf",
      "type": "document",
      "size": 102400
    }
  }
}
```

## 工具集成

### 工具链

工具可以链式调用：

```rust
let result = registry.call("get_weather", &json!({"location": "Beijing"})).await?;
let location = result["location"].as_str().unwrap();
let forecast = registry.call("get_forecast", &json!({"location": location})).await?;
```

### 工具依赖

工具可以声明依赖：

```toml
[tool]
name = "deploy"
description = "Deploy application"
dependencies = ["build", "test"]
```

### 工具组合

工具可以组合成复合工具：

```rust
pub struct DeployTool {
    build_tool:    BuildTool,
    test_tool:     TestTool,
    deploy_tool:   DeployTool,
}

impl Tool for DeployTool {
    async fn execute(&self, args: &serde_json::Value) -> ToolResult {
        self.build_tool.execute(args).await?;
        self.test_tool.execute(args).await?;
        self.deploy_tool.execute(args).await
    }
}
```

## 工具安全

### 工具权限

```yaml
tools:
  permissions:
    calculator:
      allowed_users: ["admin", "user"]
      allowed_roles: []
    deploy:
      allowed_users: ["admin"]
      allowed_roles: ["admin"]
```

### 工具速率限制

```yaml
tools:
  rate_limit:
    calculator: "100/min"
    deploy: "10/hour"
```

### 工具审计

```yaml
tools:
  audit:
    enabled: true
    log_calls: true
    log_results: false
```

## 验证

### 测试工具注册

```bash
edgecrab tools validate
```

### 测试工具执行

```bash
edgecrab tools test <tool-name>
```

### 测试 MCP 服务器

```bash
edgecrab mcp test <server-name>
```

### 测试工件系统

```bash
edgecrab artifacts test
```

## 未来计划

- 更多 MCP 协议支持
- 工具市场集成
- 工件版本控制
- 工具性能监控
- 工具推荐系统