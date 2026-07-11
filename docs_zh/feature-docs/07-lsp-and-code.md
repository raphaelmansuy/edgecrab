# LSP 和代码系统 🦀

EdgeCrab 集成了语言服务器协议（LSP）以提供高级代码理解和编辑功能。

## LSP 集成

### LSP 客户端

EdgeCrab 使用 `lsp-client` crate 来连接语言服务器：

```rust
use lsp_client::Client;
use lsp_types::*;

pub struct LspService {
    client:        Client,
    servers:       HashMap<String, ServerConnection>,
    capabilities:  ServerCapabilities,
}

impl LspService {
    pub async fn new() -> Self {
        let client = Client::new();
        LspService {
            client,
            servers: HashMap::new(),
            capabilities: ServerCapabilities::default(),
        }
    }

    pub async fn connect(&mut self, language: &str) -> Result<()> {
        let server = self.spawn_server(language).await?;
        self.servers.insert(language.to_string(), server);
        Ok(())
    }
}
```

### 支持的语言

| 语言 | LSP 服务器 | 状态 |
|------|-----------|------|
| Rust | rust-analyzer | ✅ 支持 |
| Python | pyright | ✅ 支持 |
| TypeScript | typescript-language-server | ✅ 支持 |
| JavaScript | typescript-language-server | ✅ 支持 |
| Go | gopls | ✅ 支持 |
| Java | eclipse.jdt.ls | ✅ 支持 |
| C/C++ | clangd | ✅ 支持 |
| JSON | json-lsp | ✅ 支持 |
| Markdown | marksman | ✅ 支持 |

### LSP 配置

```yaml
lsp:
  enabled: true
  auto_start: true
  servers:
    rust:
      command: ["rust-analyzer"]
      args: []
      env: {}
    python:
      command: ["pyright-langserver", "--stdio"]
      args: []
      env: {}
    typescript:
      command: ["typescript-language-server", "--stdio"]
      args: []
      env: {}
```

## 代码分析

### 符号查找

```rust
pub async fn find_symbols(
    &self,
    document_uri: &Url,
    query: &str,
) -> Result<Vec<SymbolInformation>> {
    let params = WorkspaceSymbolParams {
        query: query.to_string(),
        work_done_progress_options: Default::default(),
    };
    self.client.workspace_symbol(params).await
}
```

### 定义跳转

```rust
pub async fn find_definition(
    &self,
    document_uri: &Url,
    position: Position,
) -> Result<Option<LocationLink>> {
    let params = DefinitionParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        position,
        work_done_progress_options: Default::default(),
    };
    self.client.definition(params).await
}
```

### 引用查找

```rust
pub async fn find_references(
    &self,
    document_uri: &Url,
    position: Position,
) -> Result<Vec<Location>> {
    let params = ReferenceParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        position,
        context: ReferenceContext {
            include_declaration: true,
        },
        work_done_progress_options: Default::default(),
    };
    self.client.references(params).await
}
```

### 代码诊断

```rust
pub async fn get_diagnostics(
    &self,
    document_uri: &Url,
) -> Result<Vec<Diagnostic>> {
    self.client
        .diagnostics()
        .await
        .into_iter()
        .filter(|d| d.uri == *document_uri)
        .collect()
}
```

## 代码补全

### 智能补全

```rust
pub async fn complete(
    &self,
    document_uri: &Url,
    position: Position,
) -> Result<Option<CompletionList>> {
    let params = CompletionParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        position,
        context: None,
        work_done_progress_options: Default::default(),
    };
    self.client.completion(params).await
}
```

### 补全配置

```yaml
lsp:
  completion:
    enabled: true
    trigger_characters:
      - "."
      - ":"
      - "("
      - "["
      - "\""
    auto_import: true
    snippets: true
```

## 代码格式化

### 格式化文档

```rust
pub async fn format_document(
    &self,
    document_uri: &Url,
) -> Result<Option<Vec<TextEdit>>> {
    let params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..Default::default()
        },
        work_done_progress_options: Default::default(),
    };
    self.client.formatting(params).await
}
```

### 格式化范围

```rust
pub async fn format_range(
    &self,
    document_uri: &Url,
    range: Range,
) -> Result<Option<Vec<TextEdit>>> {
    let params = DocumentRangeFormattingParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        range,
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..Default::default()
        },
        work_done_progress_options: Default::default(),
    };
    self.client.range_formatting(params).await
}
```

## 代码重构

### 重命名

```rust
pub async fn rename(
    &self,
    document_uri: &Url,
    position: Position,
    new_name: &str,
) -> Result<Option<WorkspaceEdit>> {
    let params = RenameParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        position,
        new_name: new_name.to_string(),
        work_done_progress_options: Default::default(),
    };
    self.client.rename(params).await
}
```

### 代码操作

```rust
pub async fn execute_code_action(
    &self,
    document_uri: &Url,
    range: Range,
) -> Result<Option<WorkspaceEdit>> {
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        range,
        context: CodeActionContext {
            diagnostics: self.get_diagnostics(document_uri).await?,
            only: None,
        },
        work_done_progress_options: Default::default(),
    };
    let actions = self.client.code_action(params).await?;
    
    if let Some(action) = actions.into_iter().find(|a| a.is_edit()) {
        Ok(action.as_edit())
    } else {
        Ok(None)
    }
}
```

## 代码上下文

### 文档符号

```rust
pub async fn get_document_symbols(
    &self,
    document_uri: &Url,
) -> Result<Vec<DocumentSymbol>> {
    let params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier { uri: document_uri.clone() },
        work_done_progress_options: Default::default(),
    };
    self.client.document_symbol(params).await
}
```

### 代码结构

```text
┌─────────────────────────────────────────────────────────────┐
│  src/main.rs                                                │
│  ─────────────────────────────────────────────────────────  │
│                                                             │
│  [+] mod utils                                              │
│  │   ├─ fn helper()                                        │
│  │   └─ struct Config                                      │
│  ├─ fn main()                                               │
│  ├─ struct App                                             │
│  │   ├─ field: name                                        │
│  │   ├─ field: version                                     │
│  │   └─ fn run()                                           │
│  └─ impl App                                               │
│      ├─ fn new()                                           │
│      └─ fn process()                                       │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## 代码理解

### 语义分析

```rust
pub struct CodeAnalyzer {
    lsp_service: LspService,
    repo_map:    RepoMap,
}

impl CodeAnalyzer {
    pub async fn analyze_project(&self) -> Result<ProjectAnalysis> {
        let symbols = self.get_all_symbols().await?;
        let dependencies = self.get_dependencies().await?;
        let diagnostics = self.get_all_diagnostics().await?;
        
        Ok(ProjectAnalysis {
            symbols,
            dependencies,
            diagnostics,
        })
    }
}
```

### 代码摘要

```text
Project: edgecrab
Language: Rust
Files: 42
Lines: 5,234
Functions: 156
Structs: 42
Enums: 18
Traits: 12
Dependencies: 24
```

## LSP 工具

### 代码查询工具

```bash
edgecrab code find "calculate"
edgecrab code definition src/main.rs:42
edgecrab code references src/utils.rs:15
edgecrab code diagnostics src/main.rs
```

### 代码编辑工具

```bash
edgecrab code format src/main.rs
edgecrab code rename src/utils.rs:10 "new_name"
edgecrab code refactor src/main.rs:42 --extract-function
```

### 代码分析工具

```bash
edgecrab code analyze
edgecrab code structure
edgecrab code summary
```

## 性能优化

### LSP 缓存

```yaml
lsp:
  cache:
    enabled: true
    ttl: 5m
    max_entries: 1000
```

### 增量更新

```rust
pub async fn update_document(
    &self,
    document_uri: &Url,
    changes: &[TextDocumentContentChangeEvent],
) -> Result<()> {
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri: document_uri.clone(),
            version: Some(self.get_version(document_uri) + 1),
        },
        content_changes: changes.to_vec(),
    };
    self.client.did_change(params).await
}
```

## 验证

### 测试 LSP 连接

```bash
edgecrab lsp test <language>
```

### 测试代码分析

```bash
edgecrab code test analyze
```

### 测试代码补全

```bash
edgecrab code test completion
```

## 未来计划

- 更多语言支持
- AI 驱动的代码理解
- 代码生成和修复
- 代码审查集成
- 实时协作编辑