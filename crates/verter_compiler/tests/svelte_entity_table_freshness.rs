//! Byte-pin freshness guard for the GENERATED Svelte HTML5 named-entity table.
//!
//! `crates/verter_compiler/src/svelte/runtime/entity_table.rs` is the canonical
//! HTML5 named-character-reference table (~2231 entries) VENDORED from the pinned
//! official `svelte@5.56.3` compiler's `entities.js` by
//! `scripts/generate-svelte-entities.mjs` (the SOURCE OF TRUTH). The runtime IR's
//! static-attribute serializer decodes against it so the `$.from_html` skeleton
//! matches official EXACTLY.
//!
//! This test runs the generator's `--check` mode, which re-reads the pinned
//! svelte table, re-renders the Rust module, and byte-compares it against the
//! committed file — a hand-edit of the generated data, or a `svelte` bump without
//! a regen, fails the gate (mirroring `svelte_bind_contract_freshness`).
//!
//! It SKIPS gracefully when `node` is not on `PATH`, or when the pinned svelte is
//! not installed (a node-free / deps-free machine), rather than failing
//! spuriously; on CI with node + the pinned dep present it runs for real.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_compiler")
        .to_path_buf()
}

/// Whether `node` is runnable (`node --version` succeeds).
fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Whether the pinned svelte `entities.js` (the generator's input) is installed.
fn pinned_svelte_entities_present(root: &std::path::Path) -> bool {
    root.join("node_modules/.pnpm/svelte@5.56.3/node_modules/svelte/src/compiler/phases/1-parse/utils/entities.js")
        .exists()
}

#[test]
fn generated_entity_table_is_byte_equal_to_a_regen() {
    let root = workspace_root();
    if !node_available() {
        eprintln!(
            "SKIP svelte_entity_table_freshness: `node` not on PATH (node-free \
             machine); run on a machine with node to exercise the gate"
        );
        return;
    }
    if !pinned_svelte_entities_present(&root) {
        eprintln!(
            "SKIP svelte_entity_table_freshness: pinned svelte@5.56.3 not installed \
             (run `pnpm install`); run on a machine with the pinned dep to exercise the gate"
        );
        return;
    }

    let generator = root.join("scripts/generate-svelte-entities.mjs");
    assert!(
        generator.exists(),
        "generator missing: {}",
        generator.display()
    );

    let output = Command::new("node")
        .arg(&generator)
        .arg("--check")
        .current_dir(&root)
        .output()
        .expect("run the svelte-entities generator --check");

    assert!(
        output.status.success(),
        "the committed Svelte entity table drifted from a regen. The pinned svelte \
         `entities.js` is the SOURCE OF TRUTH — regenerate with \
         `node scripts/generate-svelte-entities.mjs` and commit. Do NOT hand-edit \
         `entity_table.rs`.\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
