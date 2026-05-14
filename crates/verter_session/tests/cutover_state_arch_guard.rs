//! RED test: `.cutover-state` config governs which `block-<N> RED` ignore
//! tokens are accepted. Reading the file at the repo root must return
//! `active_block = "0"`.
//!
//! This test reads `.cutover-state` from the workspace root (located via the
//! `CARGO_MANIFEST_DIR` of verter_session and walking up to the repo root) and
//! asserts that `active_block` is "0", matching the B0.2 dispatch state.

use std::path::PathBuf;

/// Resolve the repo root from verter_session's manifest directory.
///
/// `CARGO_MANIFEST_DIR` for this test binary is
/// `.../crates/verter_session`. The repo root is three levels up.
fn repo_root() -> PathBuf {
    let manifest =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo test");
    let p = PathBuf::from(manifest);
    // crates/verter_session → crates → repo_root
    p.parent()
        .and_then(|p| p.parent())
        .expect("could not resolve repo root from CARGO_MANIFEST_DIR")
        .to_path_buf()
}

#[test]
fn cutover_state_active_block_is_zero() {
    let state_path = repo_root().join(".cutover-state");
    assert!(
        state_path.exists(),
        ".cutover-state must exist at repo root {state_path:?}"
    );

    let content =
        std::fs::read_to_string(&state_path).expect("must be able to read .cutover-state");

    // Parse just enough to verify active_block = "0".
    let active_block_line = content
        .lines()
        .find(|l| l.trim_start().starts_with("active_block"))
        .expect(".cutover-state must contain an active_block line");

    assert!(
        active_block_line.contains('"') && active_block_line.contains('0'),
        "active_block must be \"0\" in .cutover-state; got line: {active_block_line:?}"
    );

    // Also verify landed_blocks is present (even if empty).
    let has_landed = content.contains("landed_blocks");
    assert!(
        has_landed,
        ".cutover-state must contain a landed_blocks entry; got:\n{content}"
    );
}

#[test]
fn cutover_state_has_expected_schema_comment() {
    let state_path = repo_root().join(".cutover-state");
    let content =
        std::fs::read_to_string(&state_path).expect("must be able to read .cutover-state");

    // The file must have the header comment documenting its purpose.
    assert!(
        content.contains("Stage 7 cutover state"),
        ".cutover-state must contain the header comment; got:\n{content}"
    );
    assert!(
        content.contains("cutover-state xtask"),
        ".cutover-state header must mention the owning xtask; got:\n{content}"
    );
}

#[test]
fn active_block_governs_accepted_red_tokens() {
    // When active_block = "0", only `block-0 RED` ignore annotations are
    // expected to appear in the test suite. This test verifies the invariant
    // by checking that the state file constrains the active block to exactly "0".
    let state_path = repo_root().join(".cutover-state");
    let content =
        std::fs::read_to_string(&state_path).expect("must be able to read .cutover-state");

    // Extract active_block value.
    let active = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find(|l| l.trim_start().starts_with("active_block"))
        .and_then(|l| {
            let after_eq = l.split('=').nth(1)?;
            let trimmed = after_eq.trim();
            // Strip quotes if present.
            let inner = trimmed.trim_matches('"').trim_matches('\'');
            Some(inner.to_string())
        })
        .expect(".cutover-state must have a parseable active_block value");

    assert_eq!(
        active, "0",
        "active_block must be \"0\" for B0.2; the state file said \"{active}\""
    );
}
