//! The soak/tooling runner's opt-in: with only `verter_session`'s
//! default-off `soak-support` forwarding enabled — no `test-support`
//! unification — a plain consumer opens an ACTIVE-occupancy window on a
//! host and reads its attributed peak. Removing the forwarding, or gating
//! the window only on `test-support`, fails this contract.

use verter_session::VerterHost;

fn measure(host: &VerterHost) -> Option<usize> {
    let owner = host.mint_active_occupancy_owner();
    let window = host.open_active_occupancy_window(&owner);
    let report = window.finish();
    report.attributed().ok().map(|peak| peak.peak_active_bytes)
}

fn main() {}
