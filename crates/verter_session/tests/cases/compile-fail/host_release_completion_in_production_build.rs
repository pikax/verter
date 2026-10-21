//! The host lifecycle a soak runner and an ordinary embedder share is
//! production API: with neither `test-support` nor `soak-support`, a plain
//! consumer closes a host, awaits its release boundary and reads the
//! retention figures a closed host is compared with. Gating any of them on
//! a support feature fails this contract.

use verter_session::{HostConfig, ParseReleaseError, VerterHost};

fn closed_footprint(host: &VerterHost) -> Result<(usize, usize), ParseReleaseError> {
    host.close();
    host.release_completion().wait()?;
    let snapshot = host.retention_snapshot();
    Ok((snapshot.pinned_bytes, snapshot.identity_pool_entries))
}

fn main() {
    let _ = closed_footprint(&VerterHost::new_standalone(HostConfig::default()));
}
