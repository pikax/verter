//! The ACTIVE-occupancy window is optional test/tooling state, absent from
//! every build that enables neither `test-support` nor `soak-support`: an
//! ordinary consumer cannot mint an owner or open one on a host.

use verter_session::VerterHost;

fn measure(host: &VerterHost) {
    let _window = host.mint_active_occupancy_owner();
}

fn main() {}
