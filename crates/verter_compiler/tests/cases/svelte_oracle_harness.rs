//! Feature-gated Svelte reference-drift gate (live-compiler).
//!
//! This harness is GATED behind the `svelte-oracle` Cargo feature so the
//! DEFAULT canonical run (`cargo nextest run --workspace` + `cargo test -p
//! verter_session --tests`) NEVER invokes the live svelte compiler — the
//! default suite checks the COMMITTED goldens only (see
//! `crates/verter_compiler/tests/cases/svelte_goldens_in_sync.rs`). Run the live
//! drift gate explicitly:
//!
//! ```bash
//! cargo test -p verter_compiler --features svelte-oracle \
//!   --test main svelte_oracle_harness
//! ```
//!
//! ## What it does (Svelte reference-drift gate)
//!
//! This pins the committed goldens against the PINNED official Svelte compiler:
//! every comparison here is Svelte-reference-vs-Svelte-reference (committed
//! goldens vs the pinned `compiler.compile` output), so it catches a golden that
//! has DRIFTED from the pinned reference. It is NOT yet a Verter-conformance
//! gate — there is no native Svelte codegen to diff against (see the follow-up
//! note below).
//!
//! The NORMALIZED structure + helper-call-TOPOLOGY diff engine (`topology_diff`),
//! the `NormalizedGolden` schema, and the golden loaders are the REUSABLE
//! comparison engine, housed in the importable `verter_compiler::svelte_oracle`
//! module (gated behind the same `svelte-oracle` feature) so every drift consumer
//! diffs a normalized candidate against a committed golden through the SAME
//! engine — the diff engine is the shared seam, one engine rather than a fork per
//! consumer. This harness imports it and adds the live-compiler half:
//!
//! 1. Loads every committed golden (`load_golden`).
//! 2. Re-derives the fresh normalized topology from the PINNED live compiler
//!    (by running `scripts/gen-svelte-goldens.mjs --emit-dir=<tmp>`), and
//! 3. Runs the `topology_diff` engine over `(committed golden, fresh)` and
//!    asserts ZERO divergence — proving (a) the goldens track the pinned
//!    compiler and (b) the diff engine is exercised against real, structurally
//!    rich output.
//!
//! The diff is normalized STRUCTURE + helper-call TOPOLOGY (helper families, the
//! call sequence, the import set, the export shape, the template skeletons, the
//! scope-hash topology), NOT bytes.
//!
//! TODO(svelte-native-conformance): once the native Svelte runtime codegen lands
//! (`svelte-native-compiler-plan.md`), add a Verter-side conformance comparison
//! that normalizes VERTER's emitted output and runs `topology_diff(golden,
//! verter_output)` against these same committed goldens — at which point this
//! becomes a true conformance oracle as well as a reference-drift gate. Until
//! then this file ONLY pins golden-vs-pinned-compiler drift.
#![cfg(feature = "svelte-oracle")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use verter_compiler::svelte_oracle::{
    load_all_goldens, topology_diff, ImportRow, NormalizedGolden, TopologyDivergence,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crate is <ws>/crates/verter_compiler")
        .to_path_buf()
}

fn goldens_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus/goldens")
}

// ---------------------------------------------------------------------------
// Live re-derivation (the live-compiler half of the oracle).
// ---------------------------------------------------------------------------

/// Assert `node` is on PATH and runnable. Opting into the `svelte-oracle`
/// feature ASSERTS the live toolchain is present, so a missing `node` is a HARD
/// FAILURE here — never a silent skip. A skip would make every live-oracle test
/// pass vacuously under the very feature that exists to exercise the live
/// compiler. (The DEFAULT canonical suite never compiles this file — it is
/// `#![cfg(feature = "svelte-oracle")]`-gated and stays node-free.)
fn require_node() {
    let ok = Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        ok,
        "the `svelte-oracle` feature requires `node` on PATH to drive the live \
         svelte compiler, but `node --version` did not run successfully. Opting \
         into `--features svelte-oracle` asserts the live toolchain is present; \
         run on a machine with node (the default node-free suite excludes this \
         feature-gated harness)."
    );
}

/// Re-derive the fresh normalized topology from the PINNED live compiler by
/// running the generator's `--emit-dir` mode into a temp dir, then loading the
/// emitted normalized JSON. This is the live-compiler half of the oracle.
fn rederive_live_topology() -> BTreeMap<String, NormalizedGolden> {
    let root = workspace_root();
    let generator = root.join("scripts/gen-svelte-goldens.mjs");
    assert!(
        generator.exists(),
        "generator missing: {}",
        generator.display()
    );

    let tmp = tempfile::tempdir().expect("temp dir");
    let emit_dir = tmp.path().join("fresh");

    let output = Command::new("node")
        .arg(&generator)
        .arg(format!("--emit-dir={}", emit_dir.display()))
        .current_dir(&root)
        .output()
        .expect("run gen-svelte-goldens --emit-dir");
    assert!(
        output.status.success(),
        "generator --emit-dir failed:\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    load_all_goldens(&emit_dir)
}

/// Recursively copy `src` into `dst`, preserving the relative layout. Used by
/// the live-drift discrimination self-test to obtain a mutable temp copy of the
/// committed goldens that `--check --goldens-dir=<dst>` can be pointed at.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap_or_else(|e| panic!("mkdir {}: {e}", dst.display()));
    for entry in
        std::fs::read_dir(src).unwrap_or_else(|e| panic!("read dir {}: {e}", src.display()))
    {
        let entry = entry.expect("dir entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to)
                .unwrap_or_else(|e| panic!("copy {} -> {}: {e}", from.display(), to.display()));
        }
    }
}

/// Run `node scripts/gen-svelte-goldens.mjs --check [--goldens-dir=<dir>]` and
/// return the process exit status (the live drift gate). The generator loads
/// the PINNED compiler, recompiles the corpus, and exits non-zero on any
/// drifted / missing / stale golden.
fn run_live_check(goldens_dir: Option<&Path>) -> std::process::Output {
    let root = workspace_root();
    let generator = root.join("scripts/gen-svelte-goldens.mjs");
    assert!(
        generator.exists(),
        "generator missing: {}",
        generator.display()
    );
    let mut cmd = Command::new("node");
    cmd.arg(&generator).arg("--check");
    if let Some(dir) = goldens_dir {
        cmd.arg(format!("--goldens-dir={}", dir.display()));
    }
    cmd.current_dir(&root)
        .output()
        .expect("run gen-svelte-goldens --check")
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

/// The live golden-vs-pinned-compiler drift gate. It lives behind the
/// `svelte-oracle` feature so the DEFAULT canonical run never shells `node` /
/// loads the live svelte compiler. The default suite keeps the hermetic
/// committed-golden structural guard (`svelte_goldens_in_sync.rs`); this is the
/// live `--check`. This whole feature-gated harness runs in CI via the
/// dedicated `Svelte Oracle (live, feature-gated)` job
/// (`cargo test -p verter_compiler --features svelte-oracle`) in
/// `.github/workflows/ci.yml`, and the JS `--check` is its other CI home.
#[test]
fn svelte_goldens_in_sync_with_pinned_compiler() {
    require_node();

    let output = run_live_check(None);
    assert!(
        output.status.success(),
        "the committed Svelte goldens drifted from the pinned svelte compiler. \
         Regenerate with `node scripts/gen-svelte-goldens.mjs` and review the diff \
         as the oracle delta. Do NOT hand-edit the goldens.\n{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// DISCRIMINATION proof for the live `--check` gate: a CLEAN temp copy of the
/// committed goldens passes `--check`, and a DRIFTED copy (one golden's helper
/// topology perturbed) makes `--check` exit NON-ZERO. This proves the live
/// drift gate actually recompiles + compares against the goldens — it is not a
/// vacuous pass.
#[test]
fn live_check_exits_nonzero_on_a_drifted_golden() {
    require_node();

    let tmp = tempfile::tempdir().expect("temp dir");
    let temp_goldens = tmp.path().join("goldens");
    copy_tree(&goldens_dir(), &temp_goldens);

    // A clean copy passes `--check` (the gate agrees the committed goldens
    // track the pinned compiler).
    let clean = run_live_check(Some(&temp_goldens));
    assert!(
        clean.status.success(),
        "a clean copy of the committed goldens must PASS the live --check; got \
         a failing exit.\n{}{}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr),
    );

    // Drift one golden: perturb its helper sequence/set so the regenerated
    // (pinned-compiler) output no longer matches.
    let mut victim = None;
    let mut stack = vec![temp_goldens.clone()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).expect("read temp goldens") {
            let entry = entry.expect("dir entry");
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("json") && victim.is_none() {
                victim = Some(p);
            }
        }
    }
    let victim = victim.expect("at least one golden in the temp copy");
    let raw = std::fs::read_to_string(&victim).expect("read victim golden");
    let mut value: serde_json::Value = serde_json::from_str(&raw).expect("victim parses as JSON");
    value
        .get_mut("helperSet")
        .and_then(|v| v.as_array_mut())
        .expect("victim has a `helperSet` array")
        .push(serde_json::Value::String("__phantom_helper__".to_string()));
    std::fs::write(
        &victim,
        serde_json::to_string_pretty(&value).expect("serialize drifted golden") + "\n",
    )
    .expect("write drifted golden");

    let drifted = run_live_check(Some(&temp_goldens));
    assert!(
        !drifted.status.success(),
        "the live --check MUST exit non-zero when a golden drifts from the pinned \
         compiler; it returned success, which would let a drifted golden pass.\n{}{}",
        String::from_utf8_lossy(&drifted.stdout),
        String::from_utf8_lossy(&drifted.stderr),
    );
}

#[test]
fn committed_goldens_match_live_pinned_compiler_topology() {
    require_node();

    let committed = load_all_goldens(&goldens_dir());
    assert!(
        !committed.is_empty(),
        "no committed goldens found under {}",
        goldens_dir().display()
    );
    let fresh = rederive_live_topology();

    // Key parity: the committed set and the live re-derivation must cover the
    // exact same fixture/backend identities.
    let committed_keys: Vec<&String> = committed.keys().collect();
    let fresh_keys: Vec<&String> = fresh.keys().collect();
    assert_eq!(
        committed_keys, fresh_keys,
        "committed golden key set differs from the live re-derivation set"
    );

    let mut failures = Vec::new();
    for (key, expected) in &committed {
        let actual = fresh.get(key).expect("key parity checked above");
        let divergences = topology_diff(expected, actual);
        if !divergences.is_empty() {
            failures.push(format!("{key}: {divergences:#?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "the committed Svelte goldens diverged from the live pinned-compiler topology:\n{}",
        failures.join("\n")
    );
}

#[test]
fn topology_diff_reports_parity_for_identical_goldens() {
    let committed = load_all_goldens(&goldens_dir());
    let golden = committed
        .values()
        .next()
        .expect("at least one committed golden");
    assert!(
        topology_diff(golden, golden).is_empty(),
        "topology_diff must report ZERO divergence for an identical golden"
    );
}

#[test]
fn topology_diff_discriminates_a_helper_set_perturbation() {
    // DISCRIMINATION proof: a perturbed helper set MUST produce a divergence.
    // This pins that the diff engine actually compares topology (not a nop) —
    // it is the property the runtime diff relies on to catch an emitted drift.
    let committed = load_all_goldens(&goldens_dir());
    let base = committed
        .values()
        .find(|g| g.backend == "client" && !g.helper_set.is_empty())
        .expect("a client golden with a non-empty helper set");

    let mut perturbed = base.clone();
    perturbed.helper_set.push("__phantom_helper__".to_string());
    perturbed
        .helper_sequence
        .push("__phantom_helper__".to_string());
    *perturbed
        .helper_counts
        .entry("__phantom_helper__".to_string())
        .or_insert(0) += 1;

    let divergences = topology_diff(base, &perturbed);
    assert!(
        divergences
            .iter()
            .any(|d| matches!(d, TopologyDivergence::HelperSet { .. })),
        "topology_diff must report a HelperSet divergence for a perturbed helper \
         set; got {divergences:#?}"
    );
    assert!(
        divergences
            .iter()
            .any(|d| matches!(d, TopologyDivergence::HelperSequence { .. })),
        "topology_diff must report a HelperSequence divergence for a perturbed \
         sequence; got {divergences:#?}"
    );
}

#[test]
fn topology_diff_discriminates_an_import_set_perturbation() {
    let committed = load_all_goldens(&goldens_dir());
    let base = committed
        .values()
        .find(|g| !g.imports.is_empty())
        .expect("a golden with a non-empty import set");

    let mut perturbed = base.clone();
    perturbed.imports.push(ImportRow {
        source: "svelte/internal/flags/__phantom__".to_string(),
        kind: "sideEffect".to_string(),
        names: Vec::new(),
    });

    let divergences = topology_diff(base, &perturbed);
    assert!(
        divergences
            .iter()
            .any(|d| matches!(d, TopologyDivergence::ImportSet { .. })),
        "topology_diff must report an ImportSet divergence for a perturbed import \
         set; got {divergences:#?}"
    );
}

#[test]
fn topology_diff_discriminates_identity_mismatches() {
    // DISCRIMINATION proof for the IDENTITY axis: a candidate that is
    // structurally identical to the expected golden but carries the WRONG
    // fixture identity (`slug`) or a MISMATCHED oracle stamp
    // (`oracle_version`) must still diverge — otherwise a wrong-fixture or
    // stale-stamp candidate could false-parity on its topology alone. This
    // pins that `topology_diff` checks identity, not just structure.
    let committed = load_all_goldens(&goldens_dir());
    let base = committed
        .values()
        .next()
        .expect("at least one committed golden");

    // Baseline: an identical pair is FULL parity (identity + topology).
    assert!(
        topology_diff(base, base).is_empty(),
        "an identical golden pair must report ZERO divergence"
    );

    // Mismatched slug, everything else (topology + oracle_version) identical:
    // the ONLY divergence is `Slug`. The exclusivity proves the slug check
    // fires independently of the topology comparison (a structurally identical
    // candidate is NOT silently accepted under the wrong fixture identity).
    let mut wrong_slug = base.clone();
    wrong_slug.slug = format!("{}__wrong_fixture__", base.slug);
    let slug_div = topology_diff(base, &wrong_slug);
    assert_eq!(
        slug_div.len(),
        1,
        "a slug-only mismatch must produce exactly one divergence; got {slug_div:#?}"
    );
    assert!(
        matches!(
            &slug_div[0],
            TopologyDivergence::Slug { expected, actual }
                if expected == &base.slug && actual == &wrong_slug.slug
        ),
        "a mismatched slug must produce a Slug divergence carrying the \
         expected/actual slugs; got {slug_div:#?}"
    );

    // Mismatched oracle_version, everything else identical: the ONLY
    // divergence is `OracleVersion`. This catches a candidate stamped by a
    // stale/mismatched oracle even when its topology matches.
    let mut wrong_version = base.clone();
    wrong_version.oracle_version = format!("{}-stale", base.oracle_version);
    let version_div = topology_diff(base, &wrong_version);
    assert_eq!(
        version_div.len(),
        1,
        "an oracle_version-only mismatch must produce exactly one divergence; \
         got {version_div:#?}"
    );
    assert!(
        matches!(
            &version_div[0],
            TopologyDivergence::OracleVersion { expected, actual }
                if expected == &base.oracle_version && actual == &wrong_version.oracle_version
        ),
        "a mismatched oracle_version must produce an OracleVersion divergence \
         carrying the expected/actual stamps; got {version_div:#?}"
    );
}
