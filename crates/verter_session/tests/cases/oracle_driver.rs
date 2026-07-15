//! Source-walk guard for the shared registry driver's snapshot loading
//! (`docs/arch/u0-oracle-harness-design.md` §Q1 / §4 `snapshot_loading_is_runtime_fs`).
//!
//! Snapshots are loaded at test time via runtime `std::fs::read` rooted at the
//! FULL `CARGO_MANIFEST_DIR`-relative infix — NOT `include_str!` /
//! `include_bytes!` / `include_dir!`. A second embedded snapshot artifact would
//! be a shadow registry that drifts from the on-disk tree, and the no-orphan
//! guard must enumerate the on-disk tree regardless. This guard PINS the runtime
//! `fs::read` mechanism: switching the driver to a compile-time embed FAILS it.

use std::fs;
use std::path::Path;

const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

#[test]
fn snapshot_loading_is_runtime_fs() {
    let driver = Path::new(MANIFEST_DIR).join("src/typeinfo/oracle_core/driver.rs");
    let src =
        fs::read_to_string(&driver).unwrap_or_else(|e| panic!("read {}: {e}", driver.display()));

    // The driver MUST load snapshots through runtime `std::fs::read` …
    assert!(
        src.contains("std::fs::read("),
        "the oracle driver must load snapshots via runtime `std::fs::read`"
    );
    // … rooted at the FULL manifest-relative infix (joining only
    // `oracle_snapshots/` to the manifest dir would read the wrong directory).
    assert!(
        src.contains("src/typeinfo/typeinfo_tests/oracle_snapshots"),
        "the oracle driver must root the snapshot path at the FULL \
         `src/typeinfo/typeinfo_tests/oracle_snapshots` infix"
    );
    assert!(
        src.contains("env!(\"CARGO_MANIFEST_DIR\")"),
        "the snapshot path must be CARGO_MANIFEST_DIR-rooted (hermetic, \
         absolute-path-free)"
    );

    // … and MUST NOT embed snapshot bytes at compile time (a shadow registry
    // that drifts). The needles match actual macro INVOCATIONS (`name!(`), so a
    // doc-comment that merely names the construct does not trip the guard while a
    // real `include_str!(…)` switch FAILS it. Discriminating.
    for needle in ["include_str!(", "include_bytes!(", "include_dir!("] {
        assert!(
            !src.contains(needle),
            "the oracle driver must NOT embed snapshots via `{needle}…)` — snapshot \
             loading is runtime `std::fs::read`, not a compile-time embed"
        );
    }
}
