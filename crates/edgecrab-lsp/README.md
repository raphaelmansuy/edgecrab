# edgecrab-lsp

> **Why this crate?** A coding agent that can only read files is flying blind. LSP integration  
> unlocks go-to-definition, find-references, diagnostics, and hover documentation — the same  
> signals your IDE uses to understand code. `edgecrab-lsp` is the async LSP client that lets  
> EdgeCrab query language servers at runtime, giving the agent IDE-grade code intelligence  
> without opening an editor.

Part of [EdgeCrab](https://www.edgecrab.com) — the Rust SuperAgent.

---

## What's inside

| Component | Purpose |
|-----------|---------|
| `LspClient` | Async LSP client (stdio transport, `async-lsp`) |
| `LspSession` | Manages `textDocument/didOpen` lifecycle per file |
| `LspQuery` | High-level helpers: `goto_definition`, `find_references`, `hover`, `diagnostics` |
| `LanguageId` | Maps file extensions to LSP language identifiers |

## Add to your crate

```toml
[dependencies]
edgecrab-lsp = { path = "../edgecrab-lsp" }
```

## Usage

```rust
use edgecrab_lsp::{LspClient, LspQuery};

// Start rust-analyzer for the current workspace
let client = LspClient::start("rust-analyzer", &workspace_root).await?;

// Ask for the definition of a symbol at a specific location
let defs = LspQuery::goto_definition(&client, "src/main.rs", 42, 10).await?;
for def in defs {
    println!("{}:{}:{}", def.path, def.line, def.col);
}

// Pull diagnostics (warnings, errors) for a file
let diags = LspQuery::diagnostics(&client, "src/lib.rs").await?;
for d in diags {
    eprintln!("[{}] {}", d.severity, d.message);
}

// Graceful shutdown
client.shutdown().await?;
```

## Supported servers

Any LSP-compliant server works. Tested with:

| Language | Server |
|----------|--------|
| Rust | `rust-analyzer` |
| TypeScript / JS | `typescript-language-server` |
| Python | `pyright`, `pylsp` |
| Go | `gopls` |

The server binary must be on `$PATH` (or pass an absolute path to `LspClient::start`).

---

> Full docs, guides, and release notes → [edgecrab.com](https://www.edgecrab.com)
