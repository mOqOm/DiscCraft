//! DiscCraft automation (L5).
//!
//! This crate provides an MCP stdio server that drives a DiscCraft engine
//! session through JSON-RPC 2.0 messages.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::cognitive_complexity,
    clippy::indexing_slicing
)]

mod mcp;

pub use mcp::McpServer;
