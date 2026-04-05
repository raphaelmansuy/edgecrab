//! # edgecrab-migrate
//!
//! Migration tool: hermes-agent / OpenClaw → EdgeCrab.
//!
//! WHY this crate exists:
//! ┌─────────────────────────────────────────────┐
//! │  hermes-agent / OpenClaw   ──migrate──►  EdgeCrab  │
//! │  config, state, memories, skills, .env     │
//! └─────────────────────────────────────────────┘
//!
//! Provides:
//! - `hermes::HermesMigrator` — migrate from hermes-agent
//! - `report::MigrationReport` — structured migration reporting
//! - `compat` — env var compatibility layer

#![deny(clippy::unwrap_used)]

pub mod compat;
pub mod hermes;
pub mod report;
