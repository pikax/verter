//! One-process freshness guard for deterministic Svelte compiler artifacts.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

const RECEIPT_SCHEMA: &str = "verter-compiler-generated-artifacts/v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckerReceipt {
    schema: String,
    artifacts: Vec<ArtifactReceipt>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactReceipt {
    name: String,
    status: String,
    detail: String,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("crate is <workspace>/crates/verter_compiler")
        .to_path_buf()
}

#[test]
fn generated_svelte_artifacts_match_their_authoritative_inputs() {
    let root = workspace_root();
    let checker = root.join("scripts/check-compiler-generated-artifacts.mjs");
    assert!(
        checker.is_file(),
        "generated-artifact checker missing: {}",
        checker.display()
    );

    let output = match Command::new("node")
        .arg(&checker)
        .current_dir(&root)
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            eprintln!(
                "SKIP svelte_generated_artifacts_freshness: `node` not on PATH (node-free machine); \
                 run on a machine with node to exercise the gate"
            );
            return;
        }
        Err(error) => panic!("failed to launch generated-artifact checker: {error}"),
    };

    let receipt: CheckerReceipt = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "generated-artifact checker emitted an invalid receipt: {error}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(
        receipt.schema, RECEIPT_SCHEMA,
        "unexpected checker receipt schema"
    );

    assert_eq!(
        receipt.artifacts.len(),
        2,
        "checker must emit exactly two artifact receipt rows"
    );
    let artifacts: BTreeMap<_, _> = receipt
        .artifacts
        .into_iter()
        .map(|artifact| (artifact.name.clone(), artifact))
        .collect();
    assert_eq!(
        artifacts.len(),
        2,
        "checker emitted duplicate artifact receipt names"
    );

    let bind_contract = artifacts
        .get("svelte-bind-contract")
        .expect("checker omitted svelte-bind-contract result");
    assert_eq!(
        bind_contract.status, "pass",
        "Svelte bind-contract freshness failed: {}",
        bind_contract.detail
    );

    let entity_table = artifacts
        .get("svelte-entity-table")
        .expect("checker omitted svelte-entity-table result");
    match entity_table.status.as_str() {
        "pass" => {}
        "skip" => {
            assert_eq!(
                entity_table.detail, "pinned-svelte-not-installed",
                "entity-table freshness may skip only when its pinned input is absent"
            );
            eprintln!(
                "SKIP svelte-entity-table: pinned svelte@5.56.10 not installed (run `pnpm install`); \
                 run with the pinned dependency to exercise this artifact check"
            );
        }
        status => panic!(
            "Svelte entity-table freshness returned {status}: {}",
            entity_table.detail
        ),
    }

    assert!(
        output.status.success(),
        "generated-artifact checker failed despite non-failing artifact receipts:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
