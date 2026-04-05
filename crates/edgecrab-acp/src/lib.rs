//! # edgecrab-acp
//!
//! ACP (Agent Communication Protocol) server for IDE integration.
//!
//! WHY ACP: Editors (VS Code, Zed, JetBrains) communicate with EdgeCrab
//! via the Agent Communication Protocol — a JSON-RPC 2.0 protocol over
//! stdio. This crate implements the server side.
//!
//! ```text
//! ┌──────────┐   JSON-RPC/stdio   ┌──────────────┐
//! │  Editor  │ ◄─────────────────► │ edgecrab-acp │
//! │          │                     │   ├ protocol  │ ← wire types
//! │  VS Code │                     │   ├ session   │ ← session mgr
//! │  Zed     │                     │   ├ permission│ ← approval bridge
//! │  JBrains │                     │   └ server    │ ← dispatch loop
//! └──────────┘                     └──────────────┘
//! ```

#![deny(clippy::unwrap_used)]

pub mod permission;
pub mod protocol;
pub mod server;
pub mod session;
