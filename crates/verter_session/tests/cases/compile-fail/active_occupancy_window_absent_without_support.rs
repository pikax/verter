//! The ACTIVE-occupancy window is optional test/tooling state, absent from
//! every build that enables neither `test-support` nor `soak-support`: an
//! ordinary consumer cannot open one on a host.

use verter_session::VerterHost;

fn measure(host: &VerterHost) {
    let _window = host.open_active_occupancy_window();
}

fn main() {}
