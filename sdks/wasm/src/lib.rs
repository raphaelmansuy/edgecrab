//! # EdgeCrab WASM SDK
//!
//! Lite WASM bindings for the EdgeCrab AI agent runtime.
//! Designed for browser and edge environments (Cloudflare Workers, Deno, Vercel Edge).
//!
//! This is a **lite** variant: it provides the agent core loop with custom JS-native
//! tools only. Built-in tools (file I/O, terminal, browser) are intentionally excluded
//! since they require OS-level access unavailable in browser/edge environments.
//!
//! ## Usage (JavaScript/TypeScript)
//!
//! ```js
//! import init, { Agent, Tool, Message, Role } from "@edgecrab/wasm";
//! await init();
//!
//! const agent = new Agent("openai/gpt-4o", { apiKey: "sk-..." });
//!
//! agent.addTool(Tool.create({
//!   name: "fetch_data",
//!   description: "Fetch data from an API",
//!   parameters: { url: { type: "string" } },
//!   handler: async ({ url }) => {
//!     const res = await fetch(url);
//!     return JSON.stringify(await res.json());
//!   },
//! }));
//!
//! const reply = await agent.chat("Analyze this data");
//! ```

#![allow(dead_code)]


mod agent;
mod memory;
mod tool;
mod types;

pub use agent::Agent;
pub use memory::MemoryManager;
pub use tool::Tool;
pub use types::{Message, Role, StreamEvent};
