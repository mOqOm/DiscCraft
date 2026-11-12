//! UDF 1.02 descriptor builders.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod desc;
pub mod image;
pub mod iso9660;
pub mod read;
pub mod tree;
mod udf250;
