//! Phase 6b sub-plan §6b.D2b — T11 (REGRESSION) compile-fail fixture.
//!
//! `VerterHost::workspace()` was demoted from `pub` to `pub(crate)` so
//! external crates cannot reach the workspace mutator surface directly.
//! External read consumers go through `VerterHost::workspace_read()`
//! which returns the narrower `Arc<dyn WorkspaceRead>` trait object.
//!
//! This fixture lives outside the `verter_session` crate (compiled as
//! a trybuild integration test) and attempts to call the demoted
//! method. The compile must FAIL with a privacy error.

use verter_session::{HostConfig, VerterHost};

fn main() {
    let host = VerterHost::new_standalone(HostConfig::default());
    // The compile-fail discrimination point: `workspace()` is gated
    // behind `pub(crate)`. trybuild captures the privacy error.
    let _ws = host.workspace();
}
