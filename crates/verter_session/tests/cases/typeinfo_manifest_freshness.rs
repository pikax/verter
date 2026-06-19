//! Freshness guard for the generated typeinfo manifest data.
//!
//! Every file under `crates/verter_session/tests/manifest_data/`
//! (`typeinfo_ignored_test_manifest_rows.rs`,
//! `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`) is
//! produced from `scripts/gen-typeinfo-ignore-manifest.py`
//! (`pnpm gen:typeinfo-manifest`) — the SOLE writer of all three files.
//! The authoritative §10.4.1 row→block partition feeds ONLY each
//! `IgnoredTestRow`'s `block_id` (joined with the live `#[ignore]`
//! discovery and the Capability Map). The `AdditionalProofRow` table and
//! the `TYPEINFO_PARITY_BLOCKS` block contracts (each block's
//! required_guards/verification_labels/prereqs/mechanisms) come from the
//! generator's own Python maps, NOT from §10.4.1. Whenever the generator
//! or its inputs change, the committed files must be regenerated and
//! committed in the same change.
//!
//! This guard mirrors the proto-bindings freshness pattern
//! (`crates/verter_protocol/tests/typeinfo_proto_ts_freshness.rs`): it
//! invokes the generator in `--check` mode, which regenerates each
//! tracked output in memory and byte-compares against the committed file
//! WITHOUT writing the tree, exiting non-zero (status 6) on any drift and
//! naming the stale file(s). A hand-edit to ANY generated manifest file —
//! or a generator change without regen — makes this test FAIL.
//!
//! The check gracefully skips when `python3` is absent (running `cargo
//! test` on a machine without python), exactly as the proto freshness
//! test skips when `buf` is absent. CI ships python3, so the
//! discrimination holds in CI.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

/// Locate a runnable `python3` interpreter.
///
/// 1. Prefer an explicit `PYTHON3` / `PYTHON` env override (CI hook).
/// 2. Fall back to `python3` / `python` on `PATH`.
/// 3. Return `None` when none resolves — the test then skips gracefully
///    (running on a python-free machine), mirroring how the proto
///    freshness test skips when `buf` is absent.
fn locate_python(workspace_root: &Path) -> Option<PathBuf> {
    for var in ["PYTHON3", "PYTHON"] {
        if let Some(val) = std::env::var_os(var) {
            let candidate = PathBuf::from(&val);
            if candidate.is_file() {
                return Some(candidate);
            }
            // Bare name in the override → resolve via PATH below.
            if let Some(found) = which_on_path(&candidate) {
                return Some(found);
            }
        }
    }
    for name in ["python3", "python"] {
        if let Some(found) = which_on_path(Path::new(name)) {
            return Some(found);
        }
    }
    let _ = workspace_root;
    None
}

/// Minimal `which`: returns the first existing entry for `name` on `PATH`
/// (honouring Windows executable extensions). When `name` is already an
/// absolute existing file it is returned as-is.
fn which_on_path(name: &Path) -> Option<PathBuf> {
    if name.is_absolute() && name.is_file() {
        return Some(name.to_path_buf());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let base = dir.join(name);
        if base.is_file() {
            return Some(base);
        }
        if cfg!(windows) {
            for ext in ["exe", "bat", "cmd"] {
                let candidate = base.with_extension(ext);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Byte-equality freshness discriminator: run the manifest generator in
/// `--check` mode (regenerate-in-memory + byte-compare, no tree write) and
/// assert it reports NO drift across EVERY tracked output file
/// (`typeinfo_ignored_test_manifest_rows.rs`,
/// `typeinfo_additional_proof_rows.rs`, `typeinfo_parity_blocks.rs`). Any
/// divergence — a hand-edit to a generated file, or a generator change
/// committed without regenerating — surfaces as a non-zero exit (status 6)
/// and the generator's named-file diff is echoed in the panic message.
#[test]
fn typeinfo_manifest_files_are_byte_equal_to_regenerated_generator_output() {
    let root = workspace_root();
    let script = root.join("scripts").join("gen-typeinfo-ignore-manifest.py");
    assert!(
        script.is_file(),
        "manifest generator script missing at {}",
        script.display(),
    );

    let Some(python) = locate_python(&root) else {
        // Skip gracefully when python3 isn't installed (e.g. running
        // `cargo test` on a python-free machine), exactly as the proto
        // freshness test skips when `buf` is absent. CI ships python3.
        eprintln!(
            "skipping manifest freshness check: no `python3`/`python` found via \
             $PYTHON3/$PYTHON or on `PATH`. Install python3 (CI ships it) to run \
             `python3 scripts/gen-typeinfo-ignore-manifest.py --check`."
        );
        return;
    };

    let output = Command::new(&python)
        .arg(&script)
        .arg("--check")
        .current_dir(&root)
        .output()
        .unwrap_or_else(|err| {
            panic!(
                "invoke `{} {} --check`: {err}",
                python.display(),
                script.display(),
            )
        });

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "the committed typeinfo manifest data is STALE w.r.t. \
         `scripts/gen-typeinfo-ignore-manifest.py`. The generator is the SOLE \
         writer of `crates/verter_session/tests/cases/manifest_data/*.rs`; regenerate \
         with `pnpm gen:typeinfo-manifest` and commit the result.\n\
         generator exit: {status}\n\
         --- stderr ---\n{stderr}\n--- stdout ---\n{stdout}",
        status = output.status,
    );
}

/// Discriminating per-block-count pin for the lifted rows. Each assertion is
/// pinned to the exact committed lift partition — so reverting (or
/// mis-counting) any lift's manifest re-partition breaks this test. The
/// committed partition: `U2.QUERY_VALUE_DOMAIN` owns 21 rows (all lifted —
/// the 2 index-signature publications, the 8 utility-reducer lifts, the
/// 10 class-surface-era pure-reduction lifts, and the 1 module-augmentation
/// namespace alias-chain lift, whose measured trace terminates at
/// {ResolveDecl, Instantiate(, TypeOf)}); `U2.INDEXED_ACCESS` owns 23 (14
/// lifted — incl. the 2 JSX parametric intrinsic-lookup rows re-homed in from
/// `U2.JSX_FOUNDATIONS`); `U2.UTILITIES` owns 32; `U2.MAPPED_TEMPLATE` owns 19 (6 lifted);
/// `U2.CLASS_SURFACES` owns 38 (5 lifted — the class typeof-path rows whose
/// trace dispatches `ResolveClassSurface` + `ProjectPath`);
/// `U2.MODULE_AUGMENTATION` owns 4 (0 lifted — the 4 remaining rows whose
/// `as const` / bare `typeof` / `typeof import` / `import("…").X` source
/// bodies are gate-rejected by the oracle source-walk stay `Ignored`; the
/// `typeof import(...)["default"]` row RE-LIFTED to `U2.INDEXED_ACCESS` once
/// the readonly was fixed by decoupling per-property `as const` from
/// readonly).
#[test]
fn manifest_block_counts_reflect_lifts() {
    let rows = workspace_root().join(
        "crates/verter_session/tests/cases/manifest_data/typeinfo_ignored_test_manifest_rows.rs",
    );
    let src =
        std::fs::read_to_string(&rows).unwrap_or_else(|e| panic!("read {}: {e}", rows.display()));
    let count = |needle: &str| src.matches(needle).count();

    // Per-block generated row counts (the honest override distribution).
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2QueryValueDomain,"),
        21,
        "U2.QUERY_VALUE_DOMAIN must own 21 rows: the 2 lifted index-signature \
         publication rows, the 8 U2.UTILITIES reducer rows, the 10 \
         class-surface-era pure-reduction rows, and the 1 module-augmentation \
         namespace alias-chain row — every row whose measured trace terminates \
         at {{ResolveDecl, Instantiate(, TypeOf)}}",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2IndexedAccess,"),
        23,
        "U2.INDEXED_ACCESS must own 23 rows after the 2 brand-tag index chains \
         and the 2 decoration-invariance indexed-access rows moved IN from \
         U2.CLASS_SURFACES (14 → 18), the 3 module-augmentation indexed-member \
         projection rows (the `as const` typeof indexed member + the `typeof \
         import(...)[\"default\"]` default-export value projection + the `typeof \
         import(...)[\"leafName\"]` named-value projection) moved IN from \
         U2.MODULE_AUGMENTATION (18 → 21), and the 2 JSX parametric \
         intrinsic-lookup rows (`IntrinsicPropsFor<\"div\">` / \
         `IntrinsicPropsFor<\"span\">`) moved IN from U2.JSX_FOUNDATIONS on their \
         measured-trace lifts (21 → 23)",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2Utilities,"),
        32,
        "U2.UTILITIES must own 32 rows after the 2 built-in modifier-utility rows \
         moved to U2.MAPPED_TEMPLATE (42 → 40) and the 8 utility-reducer lifts \
         moved to their measured production block U2.QUERY_VALUE_DOMAIN (40 → 32)",
    );
    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2MappedTemplate,"),
        19,
        "U2.MAPPED_TEMPLATE must own 19 rows after the 2 built-in modifier-utility \
         rows arrived lifted (16 → 18) and `wide_deep_projected_token` moved in on \
         the IndexedAccess-reduction lift (18 → 19)",
    );

    assert_eq!(
        count("block_id: TypeInfoParityBlockId::U2ModuleAugmentation,"),
        4,
        "U2.MODULE_AUGMENTATION must own 4 rows after the 4 lifted rows (the \
         `as const` typeof indexed member + the `typeof import(...)[\"default\"]` \
         default-export value projection + the `typeof import(...)[\"leafName\"]` \
         named-value projection + the namespace alias-chain row) re-homed OUT on \
         their measured-trace lifts (8 → 4); the remaining 4 rows stay `Ignored` \
         (all gate-rejected at the oracle source-walk: `Reject(ConstAssertion)`, \
         two `Reject(DeferredConstruct(\"typeof\"/\"typeof-import\"))`, and \
         `Reject(DeferredConstruct(\"import-type\"))`)",
    );

    // Lifted-status counts.
    assert_eq!(
        count("status: IgnoreStatus::Lifted {"),
        46,
        "exactly 46 IgnoredTestRows must carry `status: Lifted` (the 19 \
         pre-class-surface lifts + the 19 class-surface-era lifts + the 4 \
         module-augmentation-era lifts + the 2 JSX-era lifts + the 2 \
         mapped-template-era lifts)",
    );
    assert_eq!(
        count(
            "status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2QueryValueDomain }"
        ),
        21,
        "the 2 index-signature lifts, the 8 utility-reducer lifts, the 10 \
         class-surface-era pure-reduction lifts, and the 1 module-augmentation \
         namespace alias-chain lift must record their lifting block \
         as U2.QUERY_VALUE_DOMAIN",
    );
    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2IndexedAccess }"),
        14,
        "the 2 terminal indexed-access projection lifts, the 3 keyof-expansion \
         lifts, the 2 brand-tag index-chain lifts, the 2 \
         decoration-invariance lifts, the 3 module-augmentation \
         indexed-member projection lifts, and the 2 JSX parametric \
         intrinsic-lookup lifts must record their lifting block as \
         U2.INDEXED_ACCESS",
    );
    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2MappedTemplate }"),
        6,
        "the 2 built-in modifier-utility lifts + the wide/deep literal-union \
         projection lift + the `-?` optional-remover (`mapped_modifier_minus_optional`) \
         lift + the 2 mapped-template-era lifts (the `RecordTemplateRootSlot` \
         string-literal index-chain row + the `CounterHandlers` key-remap \
         mapped-type row) must record their lifting block as U2.MAPPED_TEMPLATE",
    );

    assert_eq!(
        count("status: IgnoreStatus::Lifted { block_id: TypeInfoParityBlockId::U2ClassSurfaces }"),
        5,
        "the 5 class typeof-path lifts (static inheritance ×2, static-generic \
         instantiation, `.prototype` extraction ×2 — the rows whose measured \
         trace dispatches ResolveClassSurface + ProjectPath) must record their \
         lifting block as U2.CLASS_SURFACES",
    );

    // Total ignored (status: Ignored) rows after 46 lifts.
    assert_eq!(
        count("status: IgnoreStatus::Ignored"),
        316,
        "exactly 316 IgnoredTestRows must remain `Ignored` (362 total − 46 lifted)",
    );
}
