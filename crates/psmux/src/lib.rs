//! Elementary stream parsers and DVD program stream muxer.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod es;
pub mod mux;
pub mod sink;
