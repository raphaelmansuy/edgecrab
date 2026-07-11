# 仓库映射系统 🦀

EdgeCrab 使用仓库映射来理解项目结构和上下文。

## 仓库映射格式

仓库映射文件位于 `.edgecrab/repo-map.yaml`：

```yaml
name: edgecrab
description: AI-powered development tool
version: 0.1.0
license: MIT
authors:
  - EdgeCrab Team

structure:
  crates/:
    description: Rust crates
    include:
      - edgecrab-core/
      - edgecrab-cli/
      - edgecrab-plugins/
      - edgecrab-gateway/
    exclude:
      - target/
      - .git/

  docs/:
    description: Documentation
    include:
      - README.md
      - CHANGELOG.md
      - docs_zh/

  scripts/:
    description: Utility scripts
    include:
      - build.sh
      - test.sh

  config/:
    description: Configuration files
    include:
      - .edgecrab/
      - config.yaml

patterns:
  - pattern: "**/*.rs"
    description: Rust source files
    language: rust

  - pattern: "**/*.md"
    description: Markdown documentation
    language: markdown

  - pattern: "**/*.yaml"
    description: YAML configuration
    language: yaml

  - pattern: "**/*.toml"
    description: TOML configuration
    language: toml

important_files:
  - README.md
  - CHANGELOG.md
  - Cargo.toml
  - crates/edgecrab-core/src/main.rs
```

## 映射发现

EdgeCrab 自动发现和构建仓库映射：

```text
项目根目录
    ↓
查找 .edgecrab/repo-map.yaml
    ↓
如果不存在，自动生成
    ↓
扫描目录结构
    ↓
识别语言和文件类型
    ↓
标记重要文件
    ↓
缓存映射结果
```

### 自动生成规则

1. **语言检测**：根据文件扩展名识别项目语言
2. **目录分类**：根据常见模式分类目录
3. **重要文件标记**：标记 README、CHANGELOG、配置文件等
4. **排除规则**：排除 `target/`、`.git/`、`node_modules/` 等

## 映射使用

### 上下文注入

仓库映射用于在系统提示中注入项目上下文：

```text
Project Context:
- Name: edgecrab
- Version: 0.1.0
- License: MIT
- Structure:
  - crates/ (Rust crates)
  - docs/ (Documentation)
  - scripts/ (Utility scripts)

Important Files:
- README.md
- CHANGELOG.md
- Cargo.toml
```

### 文件搜索

映射用于优化文件搜索：

```rust
pub fn search_files(query: &str, map: &RepoMap) -> Vec<FileMatch> {
    map.patterns
        .iter()
        .filter(|p| p.language == "rust")
        .flat_map(|p| glob::glob(&p.pattern))
        .filter(|path| path.contains(query))
        .collect()
}
```

### 代码理解

映射帮助模型理解项目结构：

```text
The project has the following structure:
- crates/edgecrab-core/ - Core functionality
- crates/edgecrab-cli/ - Command line interface
- crates/edgecrab-plugins/ - Plugin system
- crates/edgecrab-gateway/ - Gateway service

You should look at the following files for context:
1. README.md - Project overview
2. crates/edgecrab-core/src/agent.rs - Main agent logic
3. crates/edgecrab-cli/src/cli.rs - CLI entry point
```

## 映射配置

### 全局配置

```yaml
repo_map:
  enabled: true
  auto_generate: true
  cache_enabled: true
  cache_ttl: 1h
  max_depth: 5
  excluded_patterns:
    - target/
    - .git/
    - node_modules/
    - __pycache__/
    - .DS_Store
```

### 项目特定配置

```yaml
repo_map:
  name: my-project
  description: My custom project
  structure:
    src/:
      description: Source code
      include:
        - main.py
        - utils/
    tests/:
      description: Test files
      include:
        - test_*.py
```

## 映射 API

### 获取映射

```bash
edgecrab repo-map show
```

### 更新映射

```bash
edgecrab repo-map update
```

### 验证映射

```bash
edgecrab repo-map validate
```

### 导出映射

```bash
edgecrab repo-map export > repo-map.yaml
```

## 映射结构

### RepoMap 结构体

```rust
pub struct RepoMap {
    pub name:             String,
    pub description:      Option<String>,
    pub version:          Option<String>,
    pub license:          Option<String>,
    pub authors:          Vec<String>,
    pub structure:        DirectoryStructure,
    pub patterns:         Vec<FilePattern>,
    pub important_files:  Vec<String>,
    pub last_updated:     DateTime<Utc>,
}

pub struct DirectoryStructure {
    pub directories: HashMap<String, DirectoryInfo>,
}

pub struct DirectoryInfo {
    pub description: String,
    pub include:     Vec<String>,
    pub exclude:     Vec<String>,
}

pub struct FilePattern {
    pub pattern:    String,
    pub description: String,
    pub language:   String,
}
```

## 映射缓存

为了提高性能，仓库映射会被缓存：

```yaml
repo_map:
  cache_enabled: true
  cache_ttl: 1h
  cache_file: ~/.edgecrab/cache/repo-map.json
```

### 缓存策略

1. **首次访问**：生成并缓存映射
2. **后续访问**：使用缓存的映射
3. **TTL 过期**：重新生成映射
4. **文件变更**：检测并重新生成

### 缓存失效

```rust
pub fn invalidate_cache(map: &RepoMap) {
    let cache_file = PathBuf::from("~/.edgecrab/cache/repo-map.json");
    if cache_file.exists() {
        std::fs::remove_file(&cache_file).ok();
    }
}
```

## 映射扩展

### 自定义模式

```yaml
patterns:
  - pattern: "**/*.proto"
    description: Protocol buffers
    language: protobuf
    tools:
      - protobuf-compiler
```

### 自定义目录

```yaml
structure:
  proto/:
    description: Protocol definitions
    include:
      - *.proto
    tools:
      - buf
      - protoc
```

## 最佳实践

1. **保持映射更新**：定期运行 `edgecrab repo-map update`
2. **标记重要文件**：将关键文件添加到 `important_files`
3. **自定义模式**：为项目特定的文件类型添加模式
4. **排除无关文件**：排除构建目录、依赖等
5. **使用缓存**：启用缓存提高性能

## 验证

### 测试映射生成

```bash
edgecrab repo-map update --dry-run
```

### 测试文件搜索

```bash
edgecrab repo-map search "agent"
```

### 验证映射结构

```bash
edgecrab repo-map validate
```

## 未来计划

- 支持更多语言和文件类型
- 智能文件重要性评分
- 依赖图集成
- 代码复杂度分析
- 团队协作映射