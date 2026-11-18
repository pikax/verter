//! `--watch`: engine sessions and projections kept between runs, the change
//! source, the debounce window and the loop.

mod debounce;
mod run;
mod session;
mod source;

pub use debounce::Debouncer;
pub use run::run_watch;
pub use session::WatchSession;
pub use source::{ChangeSource, FileSystemChangeSource, Rewatcher, WatchError};

/// The loop over a scripted change source, for the real-engine tests.
#[cfg(test)]
pub(crate) use run::tests as loop_harness;
