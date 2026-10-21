//! Compile contracts for the soak/tooling runner's explicit opt-in: a
//! feature profile carrying only `verter_session/soak-support`.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root");
    let fixture_root = root.join("crates/verter_session/tests/cases/compile-fail");
    let tests = trybuild::TestCases::new();
    // A positive consumer of the seam through the forwarding alone: the
    // fixture names and arms the rendezvous with `soak-support` as the only
    // enabling feature.
    tests.pass(fixture_root.join("flight_rendezvous_opt_in_through_soak_support.rs"));
    // The retention account's ACTIVE-occupancy window, opened on a host and
    // read through the same forwarding alone.
    tests.pass(fixture_root.join("active_occupancy_window_opt_in_through_soak_support.rs"));
    // The production close and release boundary the runner's closed-retained
    // cell reads after, reached with capture off in the same profile.
    tests.pass(fixture_root.join("host_release_completion_through_soak_support.rs"));
}
