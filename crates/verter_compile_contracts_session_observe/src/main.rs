//! Compile contracts for the coordinated observation profile: a foreign
//! consumer whose normal dependency carries `verter_session/soak-support`
//! and `verter_session/semantic-observe` together, so the capture-available
//! agreement proof and the off/on behavior twin really build the full
//! snapshots alongside the narrow work readers.

use std::path::Path;

fn main() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("compile-contract crate must live below the workspace root");
    let fixture_root = root.join("crates/verter_session/tests/cases/compile-fail");
    // This crate is excluded from the repository workspace, so trybuild must
    // resolve cargo metadata HERE, not in the caller's working directory:
    // resolving this crate's own (single-member) workspace keeps the bin-only
    // package in the metadata package list (no runner-as-dependency is
    // injected into the fixture project) and makes the fixture's offline
    // build copy THIS crate's committed Cargo.lock, which already pins the
    // observation profile's dependency graph.
    std::env::set_current_dir(manifest_dir).expect("could not enter the runner crate directory");
    let tests = trybuild::TestCases::new();
    // With capture compiled in, the narrow work readers and the full
    // snapshots must observe the same boundaries exactly once.
    tests.pass(fixture_root.join("narrow_work_agreement_under_semantic_observe.rs"));
    // The off/on behavior twin, executed unchanged in this configuration
    // after the narrow run proves the other side.
    tests.pass(fixture_root.join("work_observation_behavior.rs"));
}
