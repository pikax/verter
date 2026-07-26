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
// `SymbolSpace` lives in the `query_specs` registry module (the sibling
// `SourceLocatorSpec` / `OracleValueKindSpec` fields below reach it by full
// path); bring it into scope so the bare `SymbolSpace::Type` field initialisers
// resolve under the `oracle-gen` lib-test build.
use super::super::query_specs::SymbolSpace;

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
            probe_rhs: ProbeRhsSpec::Bare,
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

/// The synthetic retained-migration provenance the idempotence proof hands to
/// `generate_snapshot_with_migration` — the synthetic spec is NOT a lifted row
/// (it seats no real `LIFTED_ROW_MIGRATIONS` entry), so the proof supplies the
/// record the production lookup would require, mirroring `synthetic_config`'s
/// substitution of the registry's pinned-env constants.
const SYNTHETIC_MIGRATION: super::super::query_specs::LiftMigrationProvenance =
    super::super::query_specs::LiftMigrationProvenance {
        row_file: "gen_idempotence_synthetic.rs",
        row_function: "gen_probe_resolves",
        oracle_query_ordinals: 1,
        migration_fingerprint_version: 1,
        migration_fingerprint:
            "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        workspace_files: &[],
        original_body_tokens: "{}",
    };

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oracle_gen_is_idempotent() {
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());
    let spec = synthetic_spec();

    // First generation. A tsgo-less environment SKIPS (mirrors the spike).
    let first = match generate_snapshot_with_migration(&spec, &config, &SYNTHETIC_MIGRATION).await {
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
    let second = generate_snapshot_with_migration(&spec, &config, &SYNTHETIC_MIGRATION)
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

/// A genuinely-discriminating companion: a fixture whose construct Verter
/// reduces CLEANLY (the reducer preflight passes) but whose source body sits
/// OUTSIDE the two-sided admission allowlist must be REJECTED by the ADMISSION
/// gate — so the generator writes NO snapshot for it and the expected outcome
/// is exactly `GenError::Rejected` (never `PreflightUnclean`). The construct is
/// a TEMPLATE-LITERAL type: Verter reduces it to the clean literal union
/// (`"pfx_a" | "pfx_b"`, operator-free — preflight admits), while admission
/// classifies the `TemplateLiteral` source body as a deferred construct outside
/// the source-root carve-out and rejects. (A method would now be ADMITTED
/// per-signature by admission rule E3, and a mapped/conditional alias dies
/// earlier at the reducer preflight as an opaque miss — neither exercises THIS
/// gate.) Proves the admission gate is live in the generator, not bypassed:
/// with admission admitting everything the pipeline would produce `Ok`.
/// SKIPS without tsgo.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oracle_gen_rejects_non_allowlisted_construct() {
    const TEMPLATE_FIXTURE: &str = "export type HasTemplate = `pfx_${\"a\" | \"b\"}`;\n";
    const FILES: &[super::super::query_specs::WorkspaceFileSpec] =
        &[super::super::query_specs::WorkspaceFileSpec {
            path: "/fixtures/has_template.ts",
            source: TEMPLATE_FIXTURE,
        }];
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());
    let mut spec = synthetic_spec();
    spec.workspace_files = FILES;
    spec.primary_canonical = "/fixtures/has_template.ts";
    spec.query_helper = QueryHelperSpec::ResolveExpr {
        symbol: "HasTemplate",
        type_args: &[],
        projection_mode: ProjectionModeSpec::Expanded,
        probe_rhs: ProbeRhsSpec::Bare,
    };
    spec.source_locator = super::super::query_specs::SourceLocatorSpec {
        reference_canonical: "/fixtures/has_template.ts",
        reference_name: "HasTemplate",
        symbol_space: SymbolSpace::Type,
    };

    // Drive the provenance-explicit body so the lifted-row provenance lookup is
    // not the gate that fires. The preflight MUST pass (Verter reduces the
    // template literal cleanly), so exactly the TWO-SIDED ADMISSION rejects.
    match generate_snapshot_with_migration(&spec, &config, &SYNTHETIC_MIGRATION).await {
        Err(GenError::Rejected(_)) => { /* correct — the admission gate rejects */ }
        Err(GenError::TsgoUnavailable(msg)) => {
            eprintln!("oracle_gen_rejects_non_allowlisted_construct: SKIP — tsgo: {msg}");
        }
        Ok(_) => panic!("a template-literal-bodied type must be REJECTED by the admission gate"),
        Err(e) => panic!(
            "expected Rejected from the ADMISSION gate (the preflight must PASS for this \
             fixture — a PreflightUnclean here means the fixture no longer exercises admission), \
             got {e:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Reducer PREFLIGHT — the generator gate that, before a snapshot
// is written, proves Verter's OWN resolver reduces the query to a CLEAN,
// operator-free value (§Q2 "reducer-preflight before writing carve-out
// snapshots"). These guards run Verter ONLY (no tsgo), so they do NOT skip — they
// directly exercise `preflight_reduces_clean`'s ADMIT and REJECT verdicts.
// ---------------------------------------------------------------------------

/// A self-contained carve-out fixture covering BOTH admitted source-root shapes
/// AND the operator-shell reject:
/// - `PreflightKeyof = keyof <bare ref>` — Verter reduces to the clean
///   string-literal key union (shape 1);
/// - `PreflightIndexed = Root["nested"]["value"]` — Verter reduces to the clean
///   terminal `string` (shape 2 — two of the three lifted rows are this shape);
/// - `PreflightOpenLookup<T> = T["id"]` — resolved with NO type args the object
///   stays an open `TypeParam`, so the bridge PRESERVES the `IndexedAccess`
///   operator shell (an UN-reduced value the preflight must REJECT).
const PREFLIGHT_KEYOF_FIXTURE: &str = "export interface PreflightKeys { a: string; b: number; }\nexport type PreflightKeyof = keyof PreflightKeys;\nexport interface PreflightNestedRoot { nested: { value: string } }\nexport type PreflightIndexed = PreflightNestedRoot[\"nested\"][\"value\"];\nexport type PreflightOpenLookup<T> = T[\"id\"];\n";

const PREFLIGHT_KEYOF_FILES: &[super::super::query_specs::WorkspaceFileSpec] =
    &[super::super::query_specs::WorkspaceFileSpec {
        path: "/fixtures/preflight_keyof.ts",
        source: PREFLIGHT_KEYOF_FIXTURE,
    }];

/// Build a `ResolveExpr`/`Expanded` spec over the preflight fixture for `symbol`.
fn preflight_spec(symbol: &'static str) -> QuerySpec {
    QuerySpec {
        row_file: "preflight_synthetic.rs",
        row_function: "preflight_probe",
        query_ordinal: 0,
        oracle_family: "preflight",
        workspace_files: PREFLIGHT_KEYOF_FILES,
        primary_canonical: "/fixtures/preflight_keyof.ts",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: super::super::query_specs::HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Expanded,
            probe_rhs: ProbeRhsSpec::Bare,
        },
        source_locator: super::super::query_specs::SourceLocatorSpec {
            reference_canonical: "/fixtures/preflight_keyof.ts",
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: super::super::query_specs::OracleValueKindSpec::StructuredTypeExpr,
    }
}

#[test]
fn preflight_admits_a_clean_operator_reduction() {
    // `keyof PreflightKeys` reduces (through the landed operator-reduction bridge)
    // to the clean `"a" | "b"` literal key union — the preflight ADMITs it, so the
    // carve-out's source root is backed by a real, operator-free resolver result.
    assert!(
        preflight_reduces_clean(&preflight_spec("PreflightKeyof")).is_ok(),
        "a keyof source root that Verter reduces to a literal key union must pass the preflight",
    );
}

#[test]
fn preflight_admits_a_clean_indexed_access_chain_reduction() {
    // `PreflightNestedRoot["nested"]["value"]` is the string-literal index
    // chain shape (shape 2 — two of the three lifted carve-out rows are this
    // shape). Verter reduces it through the operator-reduction bridge to the
    // clean terminal `string`, so the preflight ADMITs it. Without this case the
    // preflight guard proved only the `keyof` shape reduces clean.
    assert!(
        preflight_reduces_clean(&preflight_spec("PreflightIndexed")).is_ok(),
        "an indexed-access chain source root Verter reduces to a terminal scalar must pass the preflight",
    );
}

#[test]
fn preflight_rejects_an_unreduced_operator_shell() {
    // The "operator-free value" requirement, the OTHER half of the
    // preflight's contract: an `IndexedAccess` whose object stays an open
    // `TypeParam` (`PreflightOpenLookup<T> = T["id"]` resolved with no args) does
    // NOT reduce — the bridge PRESERVES the operator shell. The preflight must
    // REFUSE that un-reduced shell so no carve-out snapshot can be written for a
    // row whose resolver result is still an operator carrier.
    //
    // DISCRIMINATING: this rejects with the operator-construct reason
    // (`indexed-access`), distinct from the `Unknown`-shell reject the sibling
    // guard pins — proving the preflight rejects an OPERATOR shell, not only a
    // semantic miss.
    match preflight_reduces_clean(&preflight_spec("PreflightOpenLookup")) {
        Err(GenError::PreflightUnclean(msg)) => {
            assert!(
                msg.contains("not operator-free/clean") && msg.contains("indexed-access"),
                "the reject reason must name the un-reduced indexed-access operator shell; got {msg}"
            );
        }
        other => {
            panic!("an un-reduced operator shell must reject as PreflightUnclean, got {other:?}")
        }
    }
}

#[test]
fn preflight_rejects_an_unclean_reduction() {
    // A symbol the resolver cannot satisfy projects to an `Unknown { semanticMiss }`
    // shell — NOT a clean operator-free value. The preflight refuses it through the
    // same positive-allowlist predicate the oracle VALUE must clear, so no snapshot
    // can mask an unresolved (Unknown / operator-shell) reduction behind a clean
    // tsgo answer.
    match preflight_reduces_clean(&preflight_spec("DoesNotExistAnywhere")) {
        Err(GenError::PreflightUnclean(msg)) => {
            assert!(
                msg.contains("not operator-free/clean") && msg.contains("Unknown"),
                "the reject reason must name the unclean (Unknown shell) reduced value; got {msg}"
            );
        }
        other => panic!("an unresolvable symbol must reject as PreflightUnclean, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Generator capture-strategy cross-check — the DECLARED `probe_rhs` strategy
// must agree with the LIVE source-walk carve-out classification BEFORE any
// assembly: `DistributiveIdentity` is admissible ONLY for an Expanded-mode
// query whose single contributor classifies into the keyof carve-out family
// (`KeyofBareRef` / `KeyofSelfIndex`). These guards run Verter ONLY (no tsgo).
// ---------------------------------------------------------------------------

/// The preflight fixture extended with a self-index alias (the KeyofSelfIndex
/// family member) — used only by the strategy cross-check guards.
const STRATEGY_FIXTURE: &str = "export interface PreflightKeys { a: string; b: number; }\nexport type PreflightKeyof = keyof PreflightKeys;\nexport type PreflightSelfIndex = PreflightKeys[keyof PreflightKeys];\nexport interface PreflightNestedRoot { nested: { value: string } }\nexport type PreflightIndexed = PreflightNestedRoot[\"nested\"][\"value\"];\nexport type PreflightPlain = { id: number };\n";

const STRATEGY_FILES: &[super::super::query_specs::WorkspaceFileSpec] =
    &[super::super::query_specs::WorkspaceFileSpec {
        path: "/fixtures/strategy_cross_check.ts",
        source: STRATEGY_FIXTURE,
    }];

/// A `ResolveExpr` spec over the strategy fixture with an explicit strategy +
/// mode.
fn strategy_spec(
    symbol: &'static str,
    projection_mode: ProjectionModeSpec,
    probe_rhs: ProbeRhsSpec,
) -> QuerySpec {
    QuerySpec {
        row_file: "strategy_synthetic.rs",
        row_function: "strategy_probe",
        query_ordinal: 0,
        oracle_family: "strategy_cross_check",
        workspace_files: STRATEGY_FILES,
        primary_canonical: "/fixtures/strategy_cross_check.ts",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: super::super::query_specs::HostSetupKindSpec::Standalone,
        },
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol,
            type_args: &[],
            projection_mode,
            probe_rhs,
        },
        source_locator: super::super::query_specs::SourceLocatorSpec {
            reference_canonical: "/fixtures/strategy_cross_check.ts",
            reference_name: symbol,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: super::super::query_specs::OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// Run the cross-check over the spec's REAL source walk.
fn cross_check(spec: &QuerySpec) -> Result<(), GenError> {
    let walk = source_side_walk(spec);
    cross_check_probe_strategy(spec, &walk)
}

#[test]
fn distributive_identity_only_for_expanded_keyof_carveout() {
    use super::super::query_specs::ProbeRhsSpec::{Bare, DistributiveIdentity};
    use ProjectionModeSpec::{Expanded, Navigate};

    // POSITIVE: the scaffold strategy on an Expanded keyof carve-out row
    // (KeyofBareRef family) passes the cross-check.
    assert!(
        cross_check(&strategy_spec(
            "PreflightKeyof",
            Expanded,
            DistributiveIdentity
        ))
        .is_ok(),
        "DistributiveIdentity on an Expanded keyof carve-out row must pass"
    );
    // POSITIVE: the KeyofSelfIndex family member passes too — the scaffold is
    // applied UNIFORMLY to the admitted keyof carve-out family.
    assert!(
        cross_check(&strategy_spec(
            "PreflightSelfIndex",
            Expanded,
            DistributiveIdentity
        ))
        .is_ok(),
        "DistributiveIdentity on an Expanded Root[keyof Root] row must pass"
    );

    // NEGATIVE (discriminating): the scaffold strategy on a NON-keyof
    // carve-out row (a string-literal index chain) is an over-claim — REJECT.
    assert!(
        matches!(
            cross_check(&strategy_spec(
                "PreflightIndexed",
                Expanded,
                DistributiveIdentity
            )),
            Err(GenError::Rejected(_))
        ),
        "DistributiveIdentity on a string-literal-chain row must reject"
    );
    // NEGATIVE: a plain (non-carve-out) object alias claiming the scaffold.
    assert!(matches!(
        cross_check(&strategy_spec(
            "PreflightPlain",
            Expanded,
            DistributiveIdentity
        )),
        Err(GenError::Rejected(_))
    ));
    // NEGATIVE: the scaffold strategy outside Expanded mode.
    assert!(matches!(
        cross_check(&strategy_spec(
            "PreflightKeyof",
            Navigate,
            DistributiveIdentity
        )),
        Err(GenError::Rejected(_))
    ));

    // CONTROL: `Bare` is never constrained by the cross-check (under-delivery
    // is caught by the existing hover-side gates, not here).
    assert!(cross_check(&strategy_spec("PreflightPlain", Expanded, Bare)).is_ok());
    assert!(cross_check(&strategy_spec("PreflightKeyof", Expanded, Bare)).is_ok());
}

// ---------------------------------------------------------------------------
// The v4 relation capture pipeline — the relation analog of
// `oracle_gen_is_idempotent`: drives `generate_relation_snapshot` end-to-end
// against the PINNED tsgo over a SYNTHETIC relation spec and asserts schema
// completeness + strict decodability + BYTE-IDENTICAL regeneration. SKIPS
// (does not fail) when tsgo is not installed.
// ---------------------------------------------------------------------------

/// The synthetic relation spec: `{ value: number }` against `{ value: infer V }`
/// — one binder, an object-property inference the empirical wire table proves.
fn synthetic_relation_spec() -> super::super::query_specs::RelationQuerySpec {
    super::super::query_specs::RelationQuerySpec {
        row_file: "relation_verdict_oracle.rs",
        row_function: "gen_relation_synthetic",
        query_ordinal: 0,
        oracle_family: "relation_verdict",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: super::super::query_specs::HostSetupKindSpec::Standalone,
        },
        source_text: "{ value: number }",
        target_text: "{ value: infer V }",
        binder_layout: &[super::super::query_specs::RelationBinderSpec {
            ordinal: 0,
            name: "V",
            constraint: None,
        }],
        contract_rows: &["relation_gen_synthetic_contract"],
        engine_pin: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relation_gen_is_idempotent() {
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());
    let spec = synthetic_relation_spec();

    // First generation. A tsgo-less environment SKIPS (mirrors the spike).
    let first = match generate_relation_snapshot(&spec, &config).await {
        Ok(doc) => doc,
        Err(GenError::TsgoUnavailable(msg)) => {
            eprintln!("relation_gen_is_idempotent: SKIP — tsgo not available: {msg}");
            return;
        }
        Err(e) => panic!("first relation generation failed: {e:?}"),
    };

    // (1) Schema-COMPLETE + strictly decodable, kind-keyed envelope.
    let decoded = snapshot::decode_strict(&first)
        .unwrap_or_else(|e| panic!("generated relation snapshot must strict-decode: {e:?}"));
    assert_eq!(decoded.oracle_value_kind, "relation_verdict");
    assert_eq!(decoded.row_ref.row_function, "gen_relation_synthetic");
    assert!(
        decoded.migration_fingerprint.is_none() && decoded.source_admission_digest.is_none(),
        "a capture-only relation row carries no lift provenance and no source digest"
    );
    assert_eq!(first["oracle_value"]["verdict"], "assignable");
    assert_eq!(first["oracle_value"]["bindings"][0]["name"], "V");
    assert_eq!(first["oracle_value"]["bindings"][0]["ordinal"], 0);
    assert!(
        first["raw_capture"]["hover_contents"]
            .as_str()
            .unwrap_or_default()
            .contains("__oracle_probe__0"),
        "raw_capture must retain the verbatim probe-naming hover"
    );
    assert!(
        first["raw_capture"]["probe_scaffold"].is_null(),
        "the relation tuple-wire probe has no scaffold"
    );
    // The materialized value rides the normalized boundary: V = number.
    let value = snapshot::materialize_relation_value(&decoded).expect("materializes");
    assert_eq!(value.bindings.len(), 1);
    assert!(matches!(
        value.bindings[0].bound,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
    ));

    // (2) Idempotence — a second run over the unchanged spec + corpus is
    //     BYTE-IDENTICAL under the canonical encoding.
    let second = generate_relation_snapshot(&spec, &config)
        .await
        .expect("second relation generation");
    assert_eq!(
        normalize::canonical_json_string(&first),
        normalize::canonical_json_string(&second),
        "re-running the relation generator over an unchanged spec + corpus must be byte-identical"
    );
    assert_eq!(first["snapshot_id"], second["snapshot_id"]);
}

// ---------------------------------------------------------------------------
// The v3→v4 re-key proof: tsgo-free + byte-deterministic +
// relation-safe. Runs `upgrade_snapshots_to_v4_in` over a SYNTHETIC temp tree
// (never the checked-in snapshots) and asserts: (a) ONLY
// `oracle_schema_version` + `snapshot_id` change per re-keyed file (every
// tsgo-derived byte identical); (b) the new stored id redrives from the
// stored identity; (c) re-running is byte-idempotent; (d) a `relation_verdict`
// file is SKIPPED untouched. NO tsgo anywhere in this test.
// ---------------------------------------------------------------------------

/// A synthetic v3 `structured_type_expr` snapshot document (hand-built valid —
/// the strict decoder must accept it after the re-key).
fn synthetic_v3_snapshot(root_family: &str) -> (serde_json::Value, String) {
    let spec = synthetic_spec();
    let env = GenConfig::checked_in().env;
    let identity = build_identity(&spec);
    let v3_env = identity::PinnedEnv {
        oracle_schema_version: 3,
        ..env
    };
    let probe_locator = snapshot::ProbeLocator {
        probe_name: "__oracle_probe__0".to_string(),
        offset: 0,
    };
    let doc = snapshot::assemble_snapshot_document(
        root_family,
        &identity,
        &v3_env,
        &verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number)
            .to_json_value(),
        &probe_locator,
        &serde_json::json!({
            "probe_name": "__oracle_probe__0",
            "probe_header": "type __oracle_probe__0 = GenProbe;",
            "probe_scaffold": null,
            "hover_contents": "```typescript\ntype __oracle_probe__0 = number;\n```",
        }),
        &serde_json::json!({ "manifest": [], "files": [] }),
        "blake3:placeholder",
        &serde_json::json!({
            "source_locator": {
                "reference_canonical": "/fixtures/gen_probe.ts",
                "reference_name": "GenProbe",
                "symbol_space": "Type",
            },
            "observed_source_files": [],
            "contributors": [],
            "final_verdict": "Admit",
        }),
        1,
        "blake3:0000000000000000000000000000000000000000000000000000000000000000",
    );
    let snapshot_id = doc["snapshot_id"].as_str().expect("id").to_string();
    (doc, snapshot_id)
}

#[test]
fn upgrade_to_v4_in_is_tsgo_free_deterministic_and_relation_safe() {
    let dir = tempfile::tempdir().expect("temp root");
    let root = dir.path();
    let family_dir = root.join("utility_composition");
    std::fs::create_dir_all(&family_dir).expect("family dir");

    // A v3 snapshot on disk + a relation_verdict file beside it.
    let (v3_doc, v3_id) = synthetic_v3_snapshot("utility_composition");
    let v3_path = family_dir.join(format!("{v3_id}.json"));
    std::fs::write(&v3_path, normalize::canonical_json_string(&v3_doc)).expect("write v3");
    let relation_dir = root.join("relation_verdict");
    std::fs::create_dir_all(&relation_dir).expect("relation dir");
    let relation_path = relation_dir.join("u_relation.json");
    let relation_bytes = serde_json::to_string(&serde_json::json!({
        "oracle_value_kind": "relation_verdict",
        "note": "skipped before any v3-only field is read"
    }))
    .expect("relation json");
    std::fs::write(&relation_path, &relation_bytes).expect("write relation");

    // First run: re-keys the v3 file, skips the relation file.
    let (written, deleted) = upgrade_snapshots_to_v4_in(root).expect("first re-key");
    assert_eq!((written, deleted), (1, 1), "one v3 file re-keyed + removed");
    assert!(!v3_path.exists(), "the stale v3 file is removed");
    assert_eq!(
        std::fs::read_to_string(&relation_path).expect("relation file"),
        relation_bytes,
        "the relation_verdict file is SKIPPED untouched"
    );

    // Exactly one v4 file, differing from the v3 doc in ONLY
    // oracle_schema_version + snapshot_id.
    let entries: Vec<_> = std::fs::read_dir(&family_dir)
        .expect("family dir")
        .flatten()
        .collect();
    assert_eq!(entries.len(), 1);
    let v4_text = std::fs::read_to_string(entries[0].path()).expect("read v4");
    let v4_doc: serde_json::Value = serde_json::from_str(&v4_text).expect("parse v4");
    assert_eq!(v4_doc["oracle_schema_version"], 4);
    assert_eq!(v3_doc["oracle_schema_version"], 3);
    assert_ne!(v4_doc["snapshot_id"], v3_doc["snapshot_id"]);
    for key in v3_doc.as_object().expect("v3 object").keys() {
        if key == "oracle_schema_version" || key == "snapshot_id" {
            continue;
        }
        assert_eq!(
            v4_doc.get(key),
            v3_doc.get(key),
            "field `{key}` must be byte-identical after the re-key (only the \
             schema version + the derived id change)"
        );
    }
    // The new file name IS the new stored id, and the stored id redrives from
    // the stored identity (decode_strict doubles as the well-formedness gate).
    let decoded = snapshot::decode_strict(&v4_doc).expect("v4 decodes");
    let redriven = snapshot::redrive_snapshot_id(&decoded).expect("redrive");
    assert_eq!(redriven, v4_doc["snapshot_id"].as_str().expect("id"));
    assert_eq!(
        entries[0].path().file_stem().and_then(|s| s.to_str()),
        Some(redriven.as_str()),
        "the file name is the redriven id"
    );

    // Second run: byte-idempotent (overwrites itself with identical bytes,
    // deletes nothing).
    let (written2, deleted2) = upgrade_snapshots_to_v4_in(root).expect("second re-key");
    assert_eq!((written2, deleted2), (1, 0));
    let v4_text2 = std::fs::read_to_string(entries[0].path()).expect("read v4 again");
    assert_eq!(
        v4_text, v4_text2,
        "re-running the re-key is byte-idempotent"
    );
}

// ---------------------------------------------------------------------------
// Constrained-infer generation: the bound is verified against the
// declared constraint through a second tuple-wire probe drive, and the
// constrained row never aliases the unconstrained one. SKIPS without tsgo.
// ---------------------------------------------------------------------------

fn synthetic_constrained_spec(
    row_function: &'static str,
    source_text: &'static str,
) -> super::super::query_specs::RelationQuerySpec {
    super::super::query_specs::RelationQuerySpec {
        row_file: "relation_verdict_oracle.rs",
        row_function,
        query_ordinal: 0,
        oracle_family: "relation_verdict",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: super::super::query_specs::HostSetupKindSpec::Standalone,
        },
        source_text,
        target_text: "{ value: infer V extends string }",
        binder_layout: &[super::super::query_specs::RelationBinderSpec {
            ordinal: 0,
            name: "V",
            constraint: Some("string"),
        }],
        contract_rows: &["relation_constrained_gen_contract"],
        engine_pin: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relation_gen_constrained_infer_verifies_bound_against_constraint() {
    let corpus = tempfile::tempdir().expect("corpus tempdir");
    let snapshots = tempfile::tempdir().expect("snapshot tempdir");
    let config = synthetic_config(corpus.path(), snapshots.path());

    // (A) The satisfying source: `{ value: string }` binds V = string, which
    // SATISFIES the declared `extends string` — the constraint-check probe
    // drive passes and the snapshot generates.
    let spec_a = synthetic_constrained_spec("gen_constrained_assignable", "{ value: string }");
    let first = match generate_relation_snapshot(&spec_a, &config).await {
        Ok(doc) => doc,
        Err(GenError::TsgoUnavailable(msg)) => {
            eprintln!("relation_gen_constrained_infer: SKIP — tsgo not available: {msg}");
            return;
        }
        Err(e) => panic!("constrained generation failed: {e:?}"),
    };
    assert_eq!(first["oracle_value"]["verdict"], "assignable");
    assert_eq!(
        first["identity"]["binder_layout"][0]["constraint"],
        serde_json::json!({ "kind": "primitive", "name": "string" }),
        "the stored layout entry carries the canonical constraint"
    );
    let decoded = snapshot::decode_strict(&first).expect("strict decode");
    let value = snapshot::materialize_relation_value(&decoded).expect("materialize");
    assert!(matches!(
        value.bindings[0].bound,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String)
    ));
    // Idempotent.
    let second = generate_relation_snapshot(&spec_a, &config)
        .await
        .expect("second constrained generation");
    assert_eq!(
        normalize::canonical_json_string(&first),
        normalize::canonical_json_string(&second),
        "constrained regeneration is byte-identical"
    );

    // (B) The violating source: `{ value: number }` FAILS the constraint at
    // inference — tsgo answers `not_assignable` with NO bindings (there is no
    // violating bound to check), and the row generates cleanly with an
    // identity DISTINCT from (A) and from the unconstrained variant (no
    // aliasing escape).
    let spec_b = synthetic_constrained_spec("gen_constrained_failed_infer", "{ value: number }");
    let failed = generate_relation_snapshot(&spec_b, &config)
        .await
        .expect("failed-infer constrained generation");
    assert_eq!(failed["oracle_value"]["verdict"], "not_assignable");
    assert_eq!(failed["oracle_value"]["bindings"], serde_json::json!([]));
    assert_eq!(
        failed["identity"]["binder_layout"][0]["constraint"],
        serde_json::json!({ "kind": "primitive", "name": "string" }),
        "the failed-infer row still records the constraint in its identity"
    );
    assert_ne!(
        failed["snapshot_id"], first["snapshot_id"],
        "the constrained failed-infer row must not alias the satisfying row"
    );
}
