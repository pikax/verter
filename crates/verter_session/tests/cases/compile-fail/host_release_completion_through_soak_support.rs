//! The soak/tooling runner's profile — only `verter_session`'s default-off
//! `soak-support` forwarding, capture off — reaches the same production
//! close and release boundary its closed-retained cell reads after, next to
//! the ACTIVE-occupancy window it measures with.

use verter_session::{HostConfig, ParseReleaseError, VerterHost};

fn closed_retained(host: &VerterHost) -> Result<usize, ParseReleaseError> {
    let owner = host.mint_active_occupancy_owner();
    let _ = host.open_active_occupancy_window(&owner).finish();
    host.close();
    host.release_completion().wait()?;
    Ok(host.retention_snapshot().retained_bytes)
}

fn main() {
    let _ = closed_retained(&VerterHost::new_standalone(HostConfig::default()));
}
