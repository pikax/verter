//! Discriminating guards for the shared registry driver (§2 consumption / §Q4 /
//! §4). These tests do not invoke `run_row` end-to-end (the live lifted rows in
//! the test tree do that); instead every PURE sub-function the orchestrator is
//! built from is exercised directly, each test built so the VALID case passes
//! and each MUTATION proves the function rejects exactly the drift it guards.

use serde_json::{json, Value};
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::super::identity;
use super::super::normalize::ProjectionModeKind;
use super::super::query_specs::{
    HostProjectSpec, HostSetupKindSpec, OracleValueKindSpec, ProbeRhsSpec, ProjectionModeSpec,
    QueryHelperSpec, QuerySpec, SourceLocatorSpec, SymbolSpace, WorkspaceFileSpec,
};
use super::super::snapshot::{decode_strict, EnvFileEntry};
use super::super::source_digest;
use super::{
    compare_oracle_value, corpus_root, identity_from_spec, lookup_row_entries, pinned_env,
    raw_capture_matches_oracle_value, recompute_oracle_env_hash, row_basename, snapshot_abs_path,
    snapshot_relative_tail, source_admission_digest_consistent, type_argument_values,
    validate_env_corpus, validate_env_pins, DriverError,
};

const FIXTURE_PATH: &str = "/fixtures/util.ts";
const FIXTURE_SOURCE: &str = "export type Foo = { id: number };\n";
const SYMBOL: &str = "Foo";

const WORKSPACE_FILES: &[WorkspaceFileSpec] = &[WorkspaceFileSpec {
    path: FIXTURE_PATH,
    source: FIXTURE_SOURCE,
}];

const HOST_PROJECT: HostProjectSpec = HostProjectSpec {
    project_root: "/",
    workspace_root: "/",
    tsconfig_path: "/oracle.tsconfig.json",
    host_setup_kind: HostSetupKindSpec::Standalone,
};

fn sample_spec(row_file: &'static str, row_function: &'static str, ordinal: u16) -> QuerySpec {
    QuerySpec {
        row_file,
        row_function,
        query_ordinal: ordinal,
        oracle_family: "utility_composition",
        workspace_files: WORKSPACE_FILES,
        primary_canonical: FIXTURE_PATH,
        host_project: HOST_PROJECT,
        query_helper: QueryHelperSpec::ResolveExpr {
            symbol: SYMBOL,
            type_args: &[],
            projection_mode: ProjectionModeSpec::Shallow,
            probe_rhs: ProbeRhsSpec::Bare,
        },
        source_locator: SourceLocatorSpec {
            reference_canonical: FIXTURE_PATH,
            reference_name: SYMBOL,
            symbol_space: SymbolSpace::Type,
        },
        oracle_value_kind: OracleValueKindSpec::StructuredTypeExpr,
    }
}

/// The `oracle_value` for `number`, built through the real codec.
fn number_value() -> Value {
    TypeExpr::Primitive(PrimitiveName::Number).to_json_value()
}

// -- oracle_driver_basenames_file_macro -----------------------------------

/// `row_basename` normalizes a full `file!()` source path to the bare filename
/// the registry/manifest key on, and a full-path lookup finds EXACTLY the same
/// entries a bare-name lookup does. Discriminating: if the driver keyed on the
/// full path it would find NOTHING (the registry stores bare filenames).
#[test]
fn oracle_driver_basenames_file_macro() {
    assert_eq!(
        row_basename("crates/verter_session/src/typeinfo/typeinfo_tests/apparent_types.rs"),
        "apparent_types.rs"
    );
    assert_eq!(row_basename("apparent_types.rs"), "apparent_types.rs");
    assert_eq!(row_basename("a/b/c.rs"), "c.rs");

    let specs = [sample_spec("apparent_types.rs", "fn_a", 0)];
    let full_path = "crates/verter_session/src/typeinfo/typeinfo_tests/apparent_types.rs";
    let from_full = lookup_row_entries(&specs, row_basename(full_path), "fn_a");
    let from_bare = lookup_row_entries(&specs, "apparent_types.rs", "fn_a");
    assert_eq!(from_full.len(), 1, "basenamed full path must find the row");
    assert_eq!(from_bare.len(), 1);
    // A raw full-path key (no basename) finds nothing — proving the normalize is load-bearing.
    assert!(lookup_row_entries(&specs, full_path, "fn_a").is_empty());
}

// -- lookup_row_entries ----------------------------------------------------

#[test]
fn lookup_returns_row_entries_in_ordinal_order() {
    // Deliberately out of order (2, 0, 1) to prove the sort.
    let specs = [
        sample_spec("row.rs", "wanted", 2),
        sample_spec("row.rs", "wanted", 0),
        sample_spec("row.rs", "other", 0),
        sample_spec("row.rs", "wanted", 1),
    ];
    let entries = lookup_row_entries(&specs, "row.rs", "wanted");
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries.iter().map(|e| e.query_ordinal).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    // The `other` function's entry is excluded.
    assert!(entries.iter().all(|e| e.row_function == "wanted"));
    // A wrong file finds nothing.
    assert!(lookup_row_entries(&specs, "nope.rs", "wanted").is_empty());
}

// -- identity_from_spec ----------------------------------------------------

#[test]
fn identity_from_spec_hashes_sorts_and_rejects_duplicate_paths() {
    let spec = sample_spec("row.rs", "fn_a", 0);
    let id = identity_from_spec(&spec).expect("valid single-file spec");
    assert_eq!(id.workspace_files.len(), 1);
    assert_eq!(id.workspace_files[0].path, FIXTURE_PATH);
    assert_eq!(
        id.workspace_files[0].content_hash,
        identity::content_hash(FIXTURE_SOURCE)
    );
    assert_eq!(id.symbol_or_expression, SYMBOL);
    assert_eq!(id.projection_mode, ProjectionModeKind::Shallow);

    // Multi-file: sorted by canonical path regardless of declared order.
    const UNSORTED: &[WorkspaceFileSpec] = &[
        WorkspaceFileSpec {
            path: "/z.ts",
            source: "z",
        },
        WorkspaceFileSpec {
            path: "/a.ts",
            source: "a",
        },
    ];
    let mut multi = sample_spec("row.rs", "fn_a", 0);
    multi.workspace_files = UNSORTED;
    let id = identity_from_spec(&multi).expect("valid multi-file spec");
    assert_eq!(
        id.workspace_files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        vec!["/a.ts", "/z.ts"],
        "workspace files must be canonical-path sorted"
    );

    // Duplicate path is a schema violation.
    const DUP: &[WorkspaceFileSpec] = &[
        WorkspaceFileSpec {
            path: "/a.ts",
            source: "one",
        },
        WorkspaceFileSpec {
            path: "/a.ts",
            source: "two",
        },
    ];
    let mut dup = sample_spec("row.rs", "fn_a", 0);
    dup.workspace_files = DUP;
    assert_eq!(
        identity_from_spec(&dup),
        Err(DriverError::DuplicateWorkspacePath {
            path: "/a.ts".to_string()
        })
    );
}

// -- type_argument_values --------------------------------------------------

#[test]
fn type_argument_values_decodes_or_rejects() {
    // A valid printable TypeExpr-JSON arg decodes.
    const ARGS: &[&str] = &["{\"kind\":\"primitive\",\"name\":\"string\"}"];
    let helper = QueryHelperSpec::ResolveExpr {
        symbol: SYMBOL,
        type_args: ARGS,
        projection_mode: ProjectionModeSpec::Shallow,
        probe_rhs: ProbeRhsSpec::Bare,
    };
    let values = type_argument_values(&helper).expect("printable arg decodes");
    assert_eq!(values.len(), 1);

    // Malformed JSON rejects (never a silent Unknown).
    const BAD: &[&str] = &["{not json"];
    let bad_helper = QueryHelperSpec::ResolveExpr {
        symbol: SYMBOL,
        type_args: BAD,
        projection_mode: ProjectionModeSpec::Shallow,
        probe_rhs: ProbeRhsSpec::Bare,
    };
    assert!(matches!(
        type_argument_values(&bad_helper),
        Err(DriverError::BadTypeArgument { index: 0, .. })
    ));

    // Non-ResolveExpr helpers carry no type args.
    assert_eq!(
        type_argument_values(&QueryHelperSpec::ShallowSurfaceExpr { symbol: SYMBOL }).unwrap(),
        Vec::<Value>::new()
    );
}

// -- snapshot path building ------------------------------------------------

#[test]
fn snapshot_path_uses_full_manifest_rooted_infix() {
    assert_eq!(
        snapshot_relative_tail("utility_composition", "u_abc"),
        "utility_composition/u_abc.json"
    );
    let path = snapshot_abs_path("utility_composition", "u_abc");
    // Normalize separators so the forward-slash assertion holds on Windows too.
    let s = path.to_string_lossy().replace('\\', "/");
    // The FULL infix is required — joining only `oracle_snapshots/` would read
    // the wrong directory (§Q1).
    assert!(
        s.ends_with("src/typeinfo/typeinfo_tests/oracle_snapshots/utility_composition/u_abc.json"),
        "path was {s}"
    );
    assert!(s.contains("verter_session"), "rooted at the crate dir");
}

// -- validate_env_pins -----------------------------------------------------

/// Build a synthetic snapshot JSON whose stored pins/id match the spec + env, so
/// the valid case validates and a mutation fails the matching field.
fn synthetic_snapshot(spec: &QuerySpec) -> Value {
    let id = identity_from_spec(spec).expect("valid spec");
    let env = pinned_env();
    let snapshot_id = identity::derive_snapshot_id(&id, &env);
    let hash = identity::content_hash(FIXTURE_SOURCE);
    json!({
        "oracle_schema_version": env.oracle_schema_version,
        "normalizer_version": env.normalizer_version,
        "probe_synthesis_version": env.probe_synthesis_version,
        "tsgo_version": env.tsgo_version,
        "compiler_options_hash": env.compiler_options_hash,
        "env_corpus_id": env.env_corpus_id,
        "oracle_env_files": { "manifest": [], "files": [] },
        "oracle_env_hash": "blake3:placeholder",
        "oracle_family": spec.oracle_family,
        "oracle_value_kind": "structured_type_expr",
        "snapshot_id": snapshot_id,
        "migration_fingerprint_version": 1,
        "migration_fingerprint": "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        "row_ref": {
            "row_file": spec.row_file,
            "row_function": spec.row_function,
            "query_ordinal": spec.query_ordinal
        },
        "identity": {
            "query_helper_kind": "ResolveExpr",
            "workspace_files": [ { "path": FIXTURE_PATH, "content_hash": hash } ],
            "primary_canonical": FIXTURE_PATH,
            "symbol_or_expression": SYMBOL,
            "type_arguments": [],
            "projection_mode": "Shallow",
            "probe_rhs_kind": "bare",
            "host_project": {
                "project_root": "/",
                "workspace_root": "/",
                "tsconfig_path": "/oracle.tsconfig.json",
                "host_setup_kind": "standalone"
            },
            "probe_locator": { "probe_name": "__oracle_probe__0", "offset": 0 }
        },
        "oracle_value": number_value(),
        "raw_capture": {
            "probe_name": "__oracle_probe__0",
            "probe_header": "type __oracle_probe__0 = Foo;",
            "probe_scaffold": null,
            "hover_contents": "```typescript\ntype __oracle_probe__0 = number;\n```"
        },
        "source_admission_digest": {
            "source_locator": { "reference_canonical": FIXTURE_PATH, "reference_name": SYMBOL, "symbol_space": "Type" },
            "observed_source_files": [ { "path": FIXTURE_PATH, "content_hash": hash } ],
            "contributors": [],
            "final_verdict": "Admit"
        }
    })
}

#[test]
fn validate_env_pins_accepts_matching_and_rejects_drift() {
    let spec = sample_spec("util.rs", "foo_resolves", 0);
    let id = identity_from_spec(&spec).unwrap();
    let env = pinned_env();
    let decoded = decode_strict(&synthetic_snapshot(&spec)).expect("valid snapshot");
    let derived = validate_env_pins(&decoded, &spec, &id, &env).expect("matching pins");
    assert_eq!(derived, decoded.snapshot_id);

    // A drifted tsgo_version fails on exactly that field.
    let mut tampered = synthetic_snapshot(&spec);
    tampered["tsgo_version"] = json!("9.9.9-wrong");
    let decoded = decode_strict(&tampered).expect("decodes (env-pin checked separately)");
    assert!(matches!(
        validate_env_pins(&decoded, &spec, &id, &env),
        Err(DriverError::EnvPinMismatch { ref field, .. }) if field == "tsgo_version"
    ));

    // A drifted snapshot_id (identity no longer redrives) fails.
    let mut wrong_id = synthetic_snapshot(&spec);
    wrong_id["snapshot_id"] = json!("u_deadbeef");
    let decoded = decode_strict(&wrong_id).expect("decodes");
    assert!(matches!(
        validate_env_pins(&decoded, &spec, &id, &env),
        Err(DriverError::EnvPinMismatch { ref field, .. }) if field == "snapshot_id"
    ));

    // A mismatched row_ref.query_ordinal fails.
    let mut wrong_ord = synthetic_snapshot(&spec);
    wrong_ord["row_ref"]["query_ordinal"] = json!(7);
    let decoded = decode_strict(&wrong_ord).expect("decodes");
    assert!(matches!(
        validate_env_pins(&decoded, &spec, &id, &env),
        Err(DriverError::EnvPinMismatch { ref field, .. }) if field == "row_ref.query_ordinal"
    ));
}

// -- validate_env_corpus ---------------------------------------------------

#[test]
fn validate_env_corpus_catches_membership_and_content_drift() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("lib")).unwrap();
    std::fs::write(root.join("oracle.tsconfig.json"), "{\"x\":1}\n").unwrap();
    std::fs::write(
        root.join("lib/lib.es2020.d.ts"),
        "declare const x: number;\n",
    )
    .unwrap();

    let files = vec![
        EnvFileEntry {
            path: "oracle.tsconfig.json".to_string(),
            content_hash: identity::content_hash("{\"x\":1}\n"),
        },
        EnvFileEntry {
            path: "lib/lib.es2020.d.ts".to_string(),
            content_hash: identity::content_hash("declare const x: number;\n"),
        },
    ];
    let env_hash = recompute_oracle_env_hash(&files, root);

    let spec = sample_spec("util.rs", "foo_resolves", 0);
    let mut snap_json = synthetic_snapshot(&spec);
    snap_json["oracle_env_files"] = json!({
        "manifest": ["oracle.tsconfig.json", "lib/lib.es2020.d.ts"],
        "files": [
            { "path": "oracle.tsconfig.json", "content_hash": files[0].content_hash },
            { "path": "lib/lib.es2020.d.ts", "content_hash": files[1].content_hash }
        ]
    });
    snap_json["oracle_env_hash"] = json!(env_hash);
    let decoded = decode_strict(&snap_json).expect("valid snapshot");

    // Valid: on-disk set-equals manifest AND content re-hashes equal.
    validate_env_corpus(&decoded, root).expect("matching corpus");

    // Membership drift: an unlisted file appears under the corpus root.
    std::fs::write(root.join("intruder.d.ts"), "export {};\n").unwrap();
    assert!(matches!(
        validate_env_corpus(&decoded, root),
        Err(DriverError::CorpusMembershipDrift { .. })
    ));
    std::fs::remove_file(root.join("intruder.d.ts")).unwrap();

    // Content drift: a listed file's bytes change → recomputed hash differs.
    std::fs::write(root.join("oracle.tsconfig.json"), "{\"x\":999}\n").unwrap();
    assert!(matches!(
        validate_env_corpus(&decoded, root),
        Err(DriverError::OracleEnvHashDrift { .. })
    ));
}

#[test]
fn corpus_root_is_manifest_rooted() {
    // The final component is `env_corpus_dir_name(id)`, not the raw id —
    // the `:` tag separator maps to `-` at the path boundary.
    let p = corpus_root("blake3:abc");
    // Normalize separators so the forward-slash assertion holds on Windows too.
    let s = p.to_string_lossy().replace('\\', "/");
    assert!(
        s.ends_with("src/typeinfo/typeinfo_tests/oracle_env/blake3-abc"),
        "{s}"
    );
}

// -- compare_oracle_value --------------------------------------------------

#[test]
fn compare_oracle_value_passes_on_equal_and_fails_on_divergence() {
    let spec = sample_spec("util.rs", "foo_resolves", 0);
    let decoded = decode_strict(&synthetic_snapshot(&spec)).expect("valid");
    // synthetic_snapshot's oracle_value is `number`.

    // Verter resolved the SAME type → parity.
    let verter = TypeExpr::Primitive(PrimitiveName::Number);
    compare_oracle_value(&verter, &decoded, ProjectionModeKind::Shallow).expect("equal values");

    // Verter resolved a DIFFERENT primitive → a real divergence fails.
    let diverged = TypeExpr::Primitive(PrimitiveName::String);
    assert!(matches!(
        compare_oracle_value(&diverged, &decoded, ProjectionModeKind::Shallow),
        Err(DriverError::ValueMismatch { .. })
    ));
}

// -- run_row over the empty registry --------------------------------------

/// A `run_row` for a `(file, function)` with NO registry entries panics loudly
/// (the registry is the coverage authority; a stray lift must not silently pass).
#[test]
#[should_panic(expected = "NoRegistryEntries")]
fn run_row_panics_when_no_registry_entries() {
    super::run_row(
        "crates/verter_session/src/typeinfo/typeinfo_tests/nonexistent.rs",
        "no_such_row",
    );
}

// -- raw_capture_matches_oracle_value (F1) ---------------------------------

/// The consume-time oracle-VALUE fidelity guard re-derives the oracle truth from
/// the snapshot's RECORDED hover and rejects a hand-edited `oracle_value`. This
/// is the F1 hole: `compare_oracle_value` only compares Verter to the STORED
/// `oracle_value`, so a hand edit to `oracle_value` (leaving `raw_capture`
/// unchanged) warm-validates against a fabricated answer today. The valid case
/// proves the recorded `number` hover re-derives `number`; the mutation proves a
/// `string` `oracle_value` over an unchanged `number` hover FAILS.
#[test]
fn raw_capture_matches_oracle_value_catches_hand_edited_oracle_value() {
    let spec = sample_spec("util.rs", "foo_resolves", 0);
    // synthetic_snapshot records hover `number` and oracle_value `number`.
    let decoded = decode_strict(&synthetic_snapshot(&spec)).expect("valid snapshot");
    raw_capture_matches_oracle_value(&decoded, ProjectionModeKind::Shallow)
        .expect("recorded hover re-derives the stored oracle_value");

    // Hand-edit ONLY oracle_value → string, leaving raw_capture.hover_contents
    // (still `number`) intact. The snapshot stays strictly decodable.
    let mut tampered_json = synthetic_snapshot(&spec);
    tampered_json["oracle_value"] = TypeExpr::Primitive(PrimitiveName::String).to_json_value();
    let tampered = decode_strict(&tampered_json).expect("string oracle_value still decodes");
    assert!(
        matches!(
            raw_capture_matches_oracle_value(&tampered, ProjectionModeKind::Shallow),
            Err(DriverError::RawCaptureValueMismatch { .. })
        ),
        "a hand-edited oracle_value over an unchanged `number` hover must fail the \
         raw-capture fidelity guard",
    );

    // Symmetric direction: tamper the recorded HOVER (→ string) while leaving
    // oracle_value (`number`) intact — the re-derived value diverges too.
    let mut tampered_hover = synthetic_snapshot(&spec);
    tampered_hover["raw_capture"]["hover_contents"] =
        json!("```typescript\ntype __oracle_probe__0 = string;\n```");
    let tampered_hover = decode_strict(&tampered_hover).expect("decodes");
    assert!(matches!(
        raw_capture_matches_oracle_value(&tampered_hover, ProjectionModeKind::Shallow),
        Err(DriverError::RawCaptureValueMismatch { .. })
    ));
}

// -- source_admission_digest_consistent (F1) -------------------------------

/// The consume-time source-digest fidelity guard re-derives
/// `source_admission_digest` from the CURRENT registry source bytes through the
/// shared source-side walk and rejects a hand-edited digest. The valid case
/// proves the re-derivation produces the real single `Foo` contributor; the
/// mutations prove a tampered verdict / content hash FAILS — neither
/// `decode_strict` nor `compare_oracle_value` re-parses the digest, so this is
/// the F1 closure for the source-admission rail.
#[test]
fn source_admission_digest_consistent_catches_digest_drift() {
    let spec = sample_spec("util.rs", "foo_resolves", 0);
    // sample_spec's fixture is `export type Foo = { id: number };`.
    let digest = source_digest::rederive_source_digest(&spec)
        .expect("Foo resolves to a single source contributor");

    // The re-derivation produced real, meaningful content (not a vacuous empty
    // digest) — so the consistency compare below is discriminating.
    assert_eq!(
        digest["contributors"].as_array().map(Vec::len),
        Some(1),
        "Foo is a single-contributor type alias"
    );
    assert_eq!(digest["contributors"][0]["name"], json!("Foo"));
    assert_eq!(digest["contributors"][0]["decl_kind"], json!("TypeAlias"));
    assert_eq!(digest["contributors"][0]["verdict"], json!("Admit"));
    assert_eq!(digest["source_locator"]["reference_name"], json!("Foo"));
    assert_eq!(digest["source_locator"]["symbol_space"], json!("Type"));

    // The TRUE digest is consistent with the live re-derivation.
    source_admission_digest_consistent(&spec, &digest)
        .expect("the true re-derived digest is consistent");

    // A hand-edited contributor verdict fails.
    let mut bad_verdict = digest.clone();
    bad_verdict["contributors"][0]["verdict"] = json!("Reject");
    assert!(
        matches!(
            source_admission_digest_consistent(&spec, &bad_verdict),
            Err(DriverError::SourceDigestMismatch { .. })
        ),
        "a hand-edited contributor verdict must fail the source-digest guard",
    );

    // A hand-edited observed-source content hash fails.
    let mut bad_hash = digest.clone();
    bad_hash["observed_source_files"][0]["content_hash"] = json!("sha256:deadbeef");
    assert!(matches!(
        source_admission_digest_consistent(&spec, &bad_hash),
        Err(DriverError::SourceDigestMismatch { .. })
    ));

    // A hand-edited lowered body (the silently-erased-construct rail) fails.
    let mut bad_body = digest.clone();
    bad_body["contributors"][0]["lowered_body"] =
        TypeExpr::Primitive(PrimitiveName::String).to_json_value();
    assert!(matches!(
        source_admission_digest_consistent(&spec, &bad_body),
        Err(DriverError::SourceDigestMismatch { .. })
    ));
}

#[test]
fn corpus_root_of_current_env_corpus_exists_on_disk() {
    let root = corpus_root(super::super::query_specs::CURRENT_ENV_CORPUS_ID);
    assert!(
        root.is_dir(),
        "vendored corpus root must exist on disk: {}",
        root.display()
    );
    assert!(
        root.join("oracle.tsconfig.json").is_file(),
        "vendored corpus must contain oracle.tsconfig.json: {}",
        root.display()
    );
}
