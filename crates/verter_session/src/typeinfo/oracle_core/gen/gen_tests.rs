//! Discriminating guard for the snapshot GENERATOR (`oracle_gen_is_idempotent`,
//! `docs/arch/u0-oracle-harness-design.md` §4). It drives the per-spec pipeline
//! end-to-end against the PINNED tsgo over a SYNTHETIC spec + a temp vendored
//! corpus, and asserts:
//!
//! 1. the generator produces a schema-COMPLETE, strictly-decodable snapshot
//!    (every mandatory field — `oracle_value`, `raw_capture`,
//!    `source_admission_digest`, `oracle_env_files` — present and well-formed);
//! 2. the produced `oracle_value` decodes to a `TypeExpr` and matches the
//!    Verter-authored projection of the same type (confluence on the value);
//! 3. RE-RUNNING the pipeline over the unchanged spec + corpus yields a
//!    BYTE-IDENTICAL document (idempotence).
//!
//! This is what makes the generator NON-HOLLOW: the per-spec body is exercised
//! against real tsgo, not merely the empty-registry vacuous loop. It SKIPS (does
//! not fail) when tsgo is not installed, mirroring the spike's posture.
//!
//! Gated `#[cfg(all(test, feature = "oracle-gen"))]` (the enclosing `gen` module
//! is `oracle-gen`-only and this is `#[cfg(test)]`), so it is OFF the default
//! gate — run with `cargo test -p verter_session --features oracle-gen`.

use super::*;

/// A clean, single-contributor object alias — every member on the positive
/// allowlist (primitive-typed properties + an optional literal-union), so the
/// two-sided admission ADMITS and the hover lowers losslessly.
const FIXTURE: &str =
    "export type GenProbe = { id: number; label: string; tag?: \"a\" | \"b\" };\n";

const WORKSPACE_FILES: &[super::super::query_specs::WorkspaceFileSpec] =
    &[super::super::query_specs::WorkspaceFileSpec {
        path: "/fixtures/gen_probe.ts",
        source: FIXTURE,
    }];

/// The synthetic registry spec the idempotence proof drives: a `ResolveExpr`
/// query with empty `type_args`, `Expanded` mode (so tsgo prints the structural
/// body), standalone host.
fn synthetic_spec() -> QuerySpec {
    QuerySpec {
        row_file: "gen_idempotence_synthetic.rs",
        row_function: "gen_probe_resolves",
        query_ordinal: 0,
        oracle_family: "utility_composition",
        workspace_files: WORKSPACE_FILES,
        primary_canonical: "/fixtures/gen_probe.ts",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: super::super::query_specs::HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol: "GenProbe",
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
        },
        source_locator: super::super::query_specs::SourceLocatorSpec {
            reference_canonical: "/fixtures/gen_probe.ts",
            reference_name: "GenProbe",
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: super::super::query_specs::OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// Vendor a minimal synthetic corpus into `dir`: the canonical
/// `oracle.tsconfig.json` (the only file the fixture needs — it uses no lib
/// globals). Returns the `GenConfig` rooted at it.
fn synthetic_config(corpus_dir: &std::path::Path, snapshot_dir: &std::path::Path) -> GenConfig {
    const ORACLE_TSCONFIG: &str = "{ \"compilerOptions\": { \"strict\": true, \"exactOptionalPropertyTypes\": true, \"target\": \"es2020\", \"moduleResolution\": \"bundler\" } }\n";
    std::fs::write(corpus_dir.join("oracle.tsconfig.json"), ORACLE_TSCONFIG)
        .expect("write synthetic oracle.tsconfig.json");
    GenConfig {
        corpus_root: corpus_dir.to_path_buf(),
        snapshot_root: snapshot_dir.to_path_buf(),
        env: PinnedEnv {
            tsgo_version: identity::TSGO_VERSION.to_string(),
            oracle_schema_version: identity::ORACLE_SCHEMA_VERSION,
            normalizer_version: normalize::NORMALIZER_VERSION,
            probe_synthesis_version: identity::PROBE_SYNTHESIS_VERSION,
            // Synthetic stable pinned-env constants — not the (empty) registry
            // ones; the proof only needs determinism + schema validity.
            compiler_options_hash:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
            env_corpus_id:
                "blake3:1111111111111111111111111111111111111111111111111111111111111111"
                    .to_string(),
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oracle_gen_is_idempotent() {
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());
    let spec = synthetic_spec();

    // First generation. A tsgo-less environment SKIPS (mirrors the spike).
    let first = match generate_snapshot(&spec, &config).await {
        Ok(doc) => doc,
        Err(GenError::TsgoUnavailable(msg)) => {
            eprintln!("oracle_gen_is_idempotent: SKIP — tsgo not available: {msg}");
            return;
        }
        Err(e) => panic!("first generation failed: {e:?}"),
    };

    // (1) Schema-COMPLETE + strictly decodable.
    let decoded = snapshot::decode_strict(&first)
        .unwrap_or_else(|e| panic!("generated snapshot must strict-decode: {e:?}"));
    assert_eq!(decoded.oracle_value_kind, "structured_type_expr");
    assert_eq!(decoded.row_ref.row_function, "gen_probe_resolves");
    // Mandatory sub-objects are present + non-trivial.
    assert!(
        first.get("source_admission_digest").is_some(),
        "source_admission_digest is mandatory"
    );
    assert_eq!(
        first["source_admission_digest"]["final_verdict"], "Admit",
        "the admitted snapshot's digest must record Admit"
    );
    assert_eq!(
        first["source_admission_digest"]["contributors"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the admitted set is the single-contributor class"
    );
    assert!(
        first["raw_capture"]["hover_contents"]
            .as_str()
            .unwrap_or_default()
            .contains("__oracle_probe__0"),
        "raw_capture must retain the verbatim probe-naming hover"
    );

    // (2) The oracle_value decodes to a TypeExpr AND equals the Verter-authored
    //     projection of the same type (confluence on the value, not just shape).
    let oracle_expr = verter_type_expr::type_expr_from_json(&first["oracle_value"])
        .expect("oracle_value must decode to a TypeExpr");
    let authored = admission::lower_hover_rhs("{ id: number; label: string; tag?: \"a\" | \"b\" }")
        .expect("authored RHS lowers");
    let authored_norm =
        normalize::normalize(&authored, ProjectionModeKind::Expanded).expect("authored normalizes");
    assert_eq!(
        normalize::canonical_json_string(&oracle_expr.to_json_value()),
        normalize::canonical_json_string(&authored_norm.to_json_value()),
        "tsgo hover value must converge with the Verter-authored projection"
    );
    // Negative guard: the value is the structural object, not an opaque any/never.
    assert!(
        !matches!(
            oracle_expr,
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)
                | verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Never)
        ),
        "the generated value must be the resolved object, never a top/bottom shape"
    );

    // (3) Idempotence — a second run over the unchanged spec + corpus is
    //     BYTE-IDENTICAL under the canonical encoding.
    let second = generate_snapshot(&spec, &config)
        .await
        .expect("second generation");
    assert_eq!(
        normalize::canonical_json_string(&first),
        normalize::canonical_json_string(&second),
        "re-running the generator over an unchanged spec + corpus must be byte-identical"
    );
    // And the derived snapshot_id is stable across runs.
    assert_eq!(first["snapshot_id"], second["snapshot_id"]);
}

/// A genuinely-discriminating companion: a fixture carrying a NON-allowlisted
/// construct (a method — a callable member) must be REJECTED by the two-sided
/// admission, so the generator writes NO snapshot for it. Proves the gate is live
/// in the generator, not bypassed. SKIPS without tsgo.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oracle_gen_rejects_non_allowlisted_construct() {
    const METHOD_FIXTURE: &str = "export type HasMethod = { m(): void };\n";
    const FILES: &[super::super::query_specs::WorkspaceFileSpec] =
        &[super::super::query_specs::WorkspaceFileSpec {
            path: "/fixtures/has_method.ts",
            source: METHOD_FIXTURE,
        }];
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());
    let mut spec = synthetic_spec();
    spec.workspace_files = FILES;
    spec.primary_canonical = "/fixtures/has_method.ts";
    spec.query_helper = QueryHelperSpec::ResolveExpr {
        symbol: "HasMethod",
        type_args: &[],
        projection_mode: ProjectionModeSpec::Expanded,
    };
    spec.source_locator = super::super::query_specs::SourceLocatorSpec {
        reference_canonical: "/fixtures/has_method.ts",
        reference_name: "HasMethod",
        symbol_space: SymbolSpace::Type,
    };

    match generate_snapshot(&spec, &config).await {
        Err(GenError::Rejected(_)) => { /* correct — the callable member rejects */ }
        Err(GenError::TsgoUnavailable(msg)) => {
            eprintln!("oracle_gen_rejects_non_allowlisted_construct: SKIP — tsgo: {msg}");
        }
        Ok(_) => panic!("a method-bearing type must be REJECTED by the two-sided gate"),
        Err(e) => panic!("expected Rejected, got {e:?}"),
    }
}
