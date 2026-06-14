//! Byte-pin freshness guard for the GENERATED Svelte bind-contract table.
//!
//! `crates/verter_compiler/src/svelte/ide/bind_contract_data.rs` is generated
//! from the CLOSED binding-vocabulary registry in
//! `scripts/generate-svelte-bind-contract.mjs` (the SOURCE OF TRUTH for the wide
//! `bind:` family, F4). This test regenerates the table into a temp file and
//! byte-compares it against the committed file — a registry edit without a regen
//! (or a hand-edit of the generated data) fails the gate, mirroring the
//! `typeinfo_proto_ts_freshness` discipline.
//!
//! It SKIPS gracefully when `node` is not on `PATH` (a node-free machine) rather
//! than failing spuriously; on CI with node present it runs for real.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_compiler")
        .to_path_buf()
}

fn committed_data_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/svelte/ide/bind_contract_data.rs")
}

/// Whether `node` is runnable (`node --version` succeeds).
fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn generated_bind_contract_table_is_byte_equal_to_a_regen() {
    if !node_available() {
        eprintln!(
            "SKIP svelte_bind_contract_freshness: `node` not on PATH (node-free \
             machine); run on a machine with node to exercise the gate"
        );
        return;
    }

    let root = workspace_root();
    let generator = root.join("scripts/generate-svelte-bind-contract.mjs");
    assert!(
        generator.exists(),
        "generator missing: {}",
        generator.display()
    );

    let tmp = tempfile::tempdir().expect("temp dir");
    let regen = tmp.path().join("bind_contract_data.regen.rs");

    let output = Command::new("node")
        .arg(&generator)
        .env("VERTER_BIND_CONTRACT_OUT", &regen)
        .current_dir(&root)
        .output()
        .expect("run the bind-contract generator");
    assert!(
        output.status.success(),
        "generator failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let regenerated = std::fs::read_to_string(&regen).expect("read regenerated table");
    let committed = std::fs::read_to_string(committed_data_path()).expect("read committed table");

    assert_eq!(
        committed, regenerated,
        "the committed Svelte bind-contract table drifted from a regen. The \
         registry in `scripts/generate-svelte-bind-contract.mjs` is the SOURCE \
         OF TRUTH — regenerate with `node scripts/generate-svelte-bind-contract.mjs` \
         and commit. Do NOT hand-edit `bind_contract_data.rs`."
    );
}
