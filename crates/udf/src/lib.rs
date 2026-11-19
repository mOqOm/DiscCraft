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

use std::sync::atomic::{AtomicBool, Ordering};

/// What an image writer says when it was asked to stop ([`request_cancel`]).
pub const CANCELLED: &str = "the image was stopped";

static CANCEL: AtomicBool = AtomicBool::new(false);

/// Asks the image being written (in this process) to stop: its writer gives up at the next chunk of a file with
/// an error that says [`CANCELLED`]. It stays asked until [`clear_cancel`].
pub fn request_cancel() {
    CANCEL.store(true, Ordering::Relaxed);
}

/// Takes the request of [`request_cancel`] back (before an image is begun).
pub fn clear_cancel() {
    CANCEL.store(false, Ordering::Relaxed);
}

/// Whether the image being written was asked to stop.
pub(crate) fn cancelled() -> bool {
    CANCEL.load(Ordering::Relaxed)
}
pub mod image;
pub mod iso9660;
pub mod read;
mod rom;
pub mod tree;
mod udf250;
