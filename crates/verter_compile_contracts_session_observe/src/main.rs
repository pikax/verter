//! Compile contracts for the coordinated observation profile: a foreign
//! consumer whose normal dependency carries `verter_session/soak-support`
//! and `verter_session/semantic-observe` together, so the capture-available
//! agreement proof and the off/on behavior twin really build the full
//! snapshots alongside the narrow work readers.

use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root");
    let fixture_root = root.join("crates/verter_session/tests/cases/compile-fail");
    let tests = trybuild::TestCases::new();
    // With capture compiled in, the narrow work readers and the full
    // snapshots must observe the same boundaries exactly once.
    tests.pass(fixture_root.join("narrow_work_agreement_under_semantic_observe.rs"));
    // The off/on behavior twin, executed unchanged in this configuration
    // after the narrow run proves the other side.
    tests.pass(fixture_root.join("work_observation_behavior.rs"));
}
