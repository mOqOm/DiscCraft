//! ディスク書き込みの OS API（unsafe を許す唯一の crate）。
//!
//! 層: L5（docs/architecture.md §5）。
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
