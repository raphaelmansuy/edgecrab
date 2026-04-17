//! # EdgeCrab Node.js SDK — napi-rs bindings
//!
//! Native Node.js bindings for the EdgeCrab AI agent runtime.
//!
//! This crate exposes `edgecrab-sdk-core` types to Node.js/TypeScript via napi-rs,
//! providing a high-performance embedded agent runtime (no HTTP server required).

#[macro_use]
extern crate napi_derive;

mod agent;
mod config;
mod types;
