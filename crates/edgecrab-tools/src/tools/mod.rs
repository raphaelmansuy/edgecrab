//! # Core tool implementations
//!
//! Each tool is a struct implementing `ToolHandler` + inventory registration.
//!
//! ```text
//!   tools/
//!     ├── mod.rs            — this file
//!     ├── file_read.rs      — read_file
//!     ├── file_write.rs     — write_file
//!     ├── file_patch.rs     — patch (exact string replacement)
//!     ├── file_search.rs    — search_files (regex + glob)
//!     ├── terminal.rs       — shell command execution
//!     ├── process.rs        — background process management
//!     ├── clarify.rs        — ask user for clarification
//!     ├── todo.rs           — task checklist management
//!     ├── web.rs            — web_search, web_extract stubs
//!     ├── memory.rs         — memory_read, memory_write
//!     ├── skills.rs         — skills_list, skill_view
//!     ├── session_search.rs — session full-text search
//!     ├── checkpoint.rs     — filesystem snapshot for rollback
//!     ├── execute_code.rs   — sandboxed code execution
//!     ├── delegate_task.rs  — sub-agent task delegation
//!     └── advanced.rs       — generate_image, send_message stubs
//! ```

pub mod advanced;
pub mod backends;
pub mod browser;
pub mod checkpoint;
pub mod clarify;
pub mod cron;
pub mod delegate_task;
pub mod execute_code;
pub mod file_patch;
pub mod file_read;
pub mod file_search;
pub mod file_write;
pub mod homeassistant;
pub mod honcho;
pub mod mcp_client;
pub mod memory;
pub mod mixture_of_agents;
pub mod process;
pub mod session_search;
pub mod skills;
pub mod skills_guard;
pub mod skills_hub;
pub mod skills_sync;
pub mod terminal;
pub mod todo;
pub mod transcribe;
pub mod tts;
pub mod vision;
pub mod web;
