//! `.cutover-state` schema and lifecycle test.
//!
//! Verifies that the `.cutover-state` config at repo root exists, has the
//! expected TOML schema (`active_block` string + `landed_blocks` array),
//! carries the documented header comment, and respects the
//! active-vs-landed invariant: a block cannot simultaneously be the active
//! block AND appear in landed_blocks. The arch guard
//! `tests/cases/g_misc2/no_post_cutover_deferrals.rs` reads the same file to govern which
//! `block-<N> RED` ignore tokens are accepted at any moment in the cutover
//! lifecycle.

use std::path::PathBuf;

/// Resolve the repo root from verter_session's manifest directory.
///
/// `CARGO_MANIFEST_DIR` for this test binary is
/// `.../crates/verter_session`. The repo root is two levels up.
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

/// Extract the active_block string value from a `.cutover-state` body.
/// Returns `None` if the key is absent or unparseable; returns
/// `Some("")` if the key is present but the value is empty.
fn parse_active_block(content: &str) -> Option<String> {
    content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find(|l| l.trim_start().starts_with("active_block"))
        .and_then(|l| l.split('=').nth(1))
        .map(|rhs| rhs.trim().trim_matches('"').trim_matches('\'').to_string())
}

/// Extract `landed_blocks = ["a", "b", ...]` as a Vec<String>.
/// Returns `Some(vec![])` for an empty array; `None` if the key is absent.
fn parse_landed_blocks(content: &str) -> Option<Vec<String>> {
    let line = content
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .find(|l| l.trim_start().starts_with("landed_blocks"))?;
    let after_eq = line.split('=').nth(1)?.trim();
    let inner = after_eq.trim_start_matches('[').trim_end_matches(']');
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    Some(
        inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

#[test]
fn cutover_state_file_exists_and_has_schema_keys() {
    let state_path = repo_root().join(".cutover-state");
    assert!(
        state_path.exists(),
        ".cutover-state must exist at repo root {state_path:?}"
    );

    let content =
        std::fs::read_to_string(&state_path).expect("must be able to read .cutover-state");

    assert!(
        parse_active_block(&content).is_some(),
        ".cutover-state must contain a parseable `active_block` line; got:\n{content}"
    );
    assert!(
        parse_landed_blocks(&content).is_some(),
        ".cutover-state must contain a parseable `landed_blocks` line; got:\n{content}"
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
fn active_block_and_landed_blocks_are_disjoint() {
    // Lifecycle invariant: a block cannot simultaneously be the active block
    // AND appear in `landed_blocks`. The cutover-state xtask enforces this
    // on every `land` subcommand (clearing `active_block` before appending);
    // this test guards that the on-disk file always reflects that invariant.
    let state_path = repo_root().join(".cutover-state");
    let content =
        std::fs::read_to_string(&state_path).expect("must be able to read .cutover-state");

    let active = parse_active_block(&content)
        .expect(".cutover-state must have a parseable active_block value");
    let landed = parse_landed_blocks(&content)
        .expect(".cutover-state must have a parseable landed_blocks value");

    if !active.is_empty() {
        assert!(
            !landed.iter().any(|b| b == &active),
            "active_block {active:?} must not appear in landed_blocks {landed:?}; \
             the cutover-state xtask should clear active_block before \
             appending to landed_blocks"
        );
    }
}
