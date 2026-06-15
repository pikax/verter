//! Discriminating guards for the snapshot JSON schema + strict decode (§Q1 / §4).
//!
//! Every guard here is exercised with SYNTHETIC snapshot documents (no on-disk
//! snapshot exists until the first row lifts), built so the VALID case
//! round-trips and each MUTATION proves the decoder rejects exactly the drift it
//! is meant to catch.

use std::sync::Arc;

use serde_json::{json, Value};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

use super::super::identity::{
    content_hash, derive_snapshot_id, HostProject, HostSetupKind, OracleValueKind, PinnedEnv,
    ProbeRhsKind, QueryHelperKind, SnapshotIdentity, WorkspaceFileRef, ORACLE_SCHEMA_VERSION,
    PROBE_SYNTHESIS_VERSION, TSGO_VERSION,
};
use super::super::normalize::{canonical_json_string, ProjectionModeKind, NORMALIZER_VERSION};
use super::super::probe::distributive_identity_scaffold;
use super::{
    assemble_snapshot_document, decode_identity, decode_oracle_value_strict, decode_strict,
    redrive_snapshot_id, render_identity_json, ProbeLocator, SnapshotDecodeError,
    KNOWN_VALUE_KINDS,
};

// -- fixtures --------------------------------------------------------------

const FIXTURE_PATH: &str = "/fixtures/utility_composition.ts";
const FIXTURE_SOURCE: &str = "export type ComposedProps = { id: number };\n";
const SYMBOL: &str = "ComposedProps";

/// The `oracle_value` for `{ id: number }`, built through the real `TypeExpr`
/// codec so it is byte-for-byte what `to_json_value` emits (the on-disk form).
fn oracle_value() -> Value {
    let obj = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            "id".to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
            false,
            false,
        ))],
    }));
    obj.to_json_value()
}

fn workspace_file_hash() -> String {
    content_hash(FIXTURE_SOURCE)
}

/// The value-affecting identity the snapshot pins, used both to compute the
/// stored `snapshot_id` and as the body of the snapshot's `identity` object —
/// so the valid case redrives to the same id.
fn identity() -> SnapshotIdentity {
    SnapshotIdentity {
        row_file: "utility_composition.rs".to_string(),
        row_function: "composed_props_resolves".to_string(),
        query_ordinal: 0,
        query_helper_kind: QueryHelperKind::ResolveExpr,
        workspace_files: vec![WorkspaceFileRef {
            path: FIXTURE_PATH.to_string(),
            content_hash: workspace_file_hash(),
        }],
        primary_canonical: FIXTURE_PATH.to_string(),
        symbol_or_expression: SYMBOL.to_string(),
        type_arguments: vec![],
        projection_mode: ProjectionModeKind::Shallow,
        probe_rhs_kind: ProbeRhsKind::Bare,
        host_project: HostProject {
            project_root: "/".to_string(),
            workspace_root: "/".to_string(),
            tsconfig_path: "/oracle.tsconfig.json".to_string(),
            host_setup_kind: HostSetupKind::Standalone,
        },
        oracle_value_kind: OracleValueKind::StructuredTypeExpr,
    }
}

/// The pinned env, sourced from the real constants so
/// `snapshot_env_pin_matches_workspace` passes for the valid case.
/// `compiler_options_hash` / `env_corpus_id` are generation-derived placeholders.
fn pinned_env() -> PinnedEnv {
    PinnedEnv {
        tsgo_version: TSGO_VERSION.to_string(),
        oracle_schema_version: ORACLE_SCHEMA_VERSION,
        normalizer_version: NORMALIZER_VERSION,
        probe_synthesis_version: PROBE_SYNTHESIS_VERSION,
        compiler_options_hash: "sha256:0000".to_string(),
        env_corpus_id: "blake3:0000".to_string(),
    }
}

/// A fully-valid synthetic snapshot whose stored `snapshot_id` is the real
/// derived id for [`identity`] + [`pinned_env`].
fn valid_snapshot() -> Value {
    let id = identity();
    let env = pinned_env();
    let snapshot_id = derive_snapshot_id(&id, &env);
    let hash = workspace_file_hash();

    json!({
        "oracle_schema_version": env.oracle_schema_version,
        "normalizer_version": env.normalizer_version,
        "probe_synthesis_version": env.probe_synthesis_version,
        "tsgo_version": env.tsgo_version,
        "compiler_options_hash": env.compiler_options_hash,
        "env_corpus_id": env.env_corpus_id,
        "oracle_env_files": {
            "manifest": ["oracle.tsconfig.json", "lib/lib.es2020.d.ts"],
            "files": [
                { "path": "oracle.tsconfig.json", "content_hash": "sha256:c0de" },
                { "path": "lib/lib.es2020.d.ts", "content_hash": "sha256:1f88" }
            ]
        },
        "oracle_env_hash": "blake3:7c4e",
        "oracle_family": "utility_composition",
        "oracle_value_kind": "structured_type_expr",
        "snapshot_id": snapshot_id,
        "migration_fingerprint_version": 1,
        "migration_fingerprint": "blake3:0000000000000000000000000000000000000000000000000000000000000000",
        "row_ref": {
            "row_file": "utility_composition.rs",
            "row_function": "composed_props_resolves",
            "query_ordinal": 0
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
            "probe_locator": { "probe_name": "__oracle_probe__0", "offset": 412 }
        },
        "oracle_value": oracle_value(),
        "raw_capture": {
            "probe_name": "__oracle_probe__0",
            "probe_header": "type __oracle_probe__0 = ComposedProps;",
            "probe_scaffold": null,
            "hover_contents": "```typescript\ntype __oracle_probe__0 = {\n    id: number;\n}\n```"
        },
        "source_admission_digest": {
            "source_locator": {
                "reference_canonical": FIXTURE_PATH,
                "reference_name": SYMBOL,
                "symbol_space": "Type"
            },
            "observed_source_files": [ { "path": FIXTURE_PATH, "content_hash": hash } ],
            "contributors": [
                {
                    "contributor_ordinal": 0,
                    "decl_span": { "file": FIXTURE_PATH, "start": 11, "end": 43 },
                    "decl_canonical": FIXTURE_PATH,
                    "name": SYMBOL,
                    "symbol_space": "Type",
                    "decl_kind": "TypeAlias",
                    "raw_surface": { "raw_member_keys": ["Static(id)"] },
                    "lowered_body": oracle_value(),
                    "verdict": "Admit"
                }
            ],
            "final_verdict": "Admit"
        }
    })
}

/// The probe-locator the valid fixture stores on `identity`.
fn probe_locator() -> ProbeLocator {
    ProbeLocator {
        probe_name: "__oracle_probe__0".to_string(),
        offset: 412,
    }
}

// -- snapshot_encode_assembles_canonical_document --------------------------

/// The ENCODE path (the generator's write step) assembles BYTE-FOR-BYTE the
/// hand-authored canonical fixture from the structured identity + env + the
/// per-stage sub-objects, and the assembled document strictly decodes. The
/// hand-authored [`valid_snapshot`] is the independent oracle: if the assembler
/// emitted a wrong field, a wrong key, a mis-rendered identity axis, or a
/// non-derived `snapshot_id`, it would DIVERGE from the fixture (or fail decode).
#[test]
fn snapshot_encode_assembles_canonical_document() {
    let fixture = valid_snapshot();
    let id = identity();
    let env = pinned_env();
    let ov = oracle_value();

    let assembled = assemble_snapshot_document(
        fixture["oracle_family"].as_str().unwrap(),
        &id,
        &env,
        &ov,
        &probe_locator(),
        &fixture["raw_capture"],
        &fixture["oracle_env_files"],
        fixture["oracle_env_hash"].as_str().unwrap(),
        &fixture["source_admission_digest"],
        fixture["migration_fingerprint_version"].as_u64().unwrap() as u32,
        fixture["migration_fingerprint"].as_str().unwrap(),
    );

    // The assembled document equals the hand-authored canonical fixture.
    assert_eq!(
        canonical_json_string(&assembled),
        canonical_json_string(&fixture),
        "the encode path must assemble the exact canonical snapshot document"
    );
    // …and it strictly decodes (the encode path is the decoder's true inverse).
    assert!(
        decode_strict(&assembled).is_ok(),
        "an assembled document must strictly decode"
    );

    // Discriminating: a changed identity axis (the queried symbol) MUST change
    // the assembled document — both the rendered `identity.symbol_or_expression`
    // AND the derived `snapshot_id` — so the assembler reflects its inputs, not a
    // baked constant.
    let mut other_id = identity();
    other_id.symbol_or_expression = "DifferentSymbol".to_string();
    let assembled_other = assemble_snapshot_document(
        fixture["oracle_family"].as_str().unwrap(),
        &other_id,
        &env,
        &ov,
        &probe_locator(),
        &fixture["raw_capture"],
        &fixture["oracle_env_files"],
        fixture["oracle_env_hash"].as_str().unwrap(),
        &fixture["source_admission_digest"],
        fixture["migration_fingerprint_version"].as_u64().unwrap() as u32,
        fixture["migration_fingerprint"].as_str().unwrap(),
    );
    assert_ne!(
        assembled_other["snapshot_id"], assembled["snapshot_id"],
        "a changed identity must derive a different snapshot_id"
    );
    assert_eq!(
        assembled_other["identity"]["symbol_or_expression"],
        json!("DifferentSymbol")
    );
}

// -- render_identity_json_sorts_workspace_files ----------------------------

/// `render_identity_json` emits `workspace_files` PATH-SORTED regardless of the
/// `SnapshotIdentity`'s stored order, matching the canonical order `snapshot_id`
/// hashes (upsert order is never an identity input).
#[test]
fn render_identity_json_sorts_workspace_files() {
    let mut id = identity();
    id.workspace_files = vec![
        WorkspaceFileRef {
            path: "/fixtures/z.ts".to_string(),
            content_hash: "sha256:aaaa".to_string(),
        },
        WorkspaceFileRef {
            path: "/fixtures/a.ts".to_string(),
            content_hash: "sha256:bbbb".to_string(),
        },
    ];
    let rendered = render_identity_json(&id, &probe_locator());
    let paths: Vec<&str> = rendered["workspace_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        paths,
        vec!["/fixtures/a.ts", "/fixtures/z.ts"],
        "workspace_files must be path-sorted in the rendered identity"
    );
}

// -- strict_snapshot_decode ------------------------------------------------

#[test]
fn strict_snapshot_decode() {
    // Valid snapshot decodes.
    let ok = decode_strict(&valid_snapshot());
    assert!(ok.is_ok(), "valid snapshot must decode: {ok:?}");

    // An unknown envelope field FAILS (deny_unknown_fields), not silently ignored.
    let mut extra = valid_snapshot();
    extra
        .as_object_mut()
        .unwrap()
        .insert("totally_unexpected".to_string(), json!(true));
    assert!(
        matches!(decode_strict(&extra), Err(SnapshotDecodeError::Envelope(_))),
        "an unknown envelope field must FAIL decode"
    );

    // A missing required field FAILS.
    let mut missing = valid_snapshot();
    missing.as_object_mut().unwrap().remove("tsgo_version");
    assert!(
        matches!(
            decode_strict(&missing),
            Err(SnapshotDecodeError::Envelope(_))
        ),
        "a missing required envelope field must FAIL decode"
    );

    // v3: the migration-fidelity mirror fields are REQUIRED — a v2-shaped snapshot
    // (missing `migration_fingerprint` / `migration_fingerprint_version`) FAILS
    // strict decode, so no pre-v3 snapshot can warm-validate under the new schema.
    for field in ["migration_fingerprint", "migration_fingerprint_version"] {
        let mut v2_shaped = valid_snapshot();
        v2_shaped.as_object_mut().unwrap().remove(field);
        assert!(
            matches!(
                decode_strict(&v2_shaped),
                Err(SnapshotDecodeError::Envelope(_))
            ),
            "a snapshot missing the required v3 field `{field}` must FAIL decode"
        );
    }

    // A snapshot whose oracle_value carries a MALFORMED member (the member is
    // dropped by the shared `filter_map` decoder) FAILS rather than decoding to a
    // smaller TypeExpr.
    let mut dropped = valid_snapshot();
    let props = dropped["oracle_value"]["properties"]
        .as_array_mut()
        .unwrap();
    // Append a member missing its `ty` — `json_to_object_member` returns None and
    // filter_map silently drops it.
    props.push(
        json!({ "memberKind": "property", "name": "ghost", "optional": false, "readonly": false }),
    );
    assert!(
        matches!(
            decode_strict(&dropped),
            Err(SnapshotDecodeError::OracleValueLossyDecode)
        ),
        "a dropped/malformed oracle_value member must FAIL strict decode"
    );
}

// -- oracle_value_decodes_to_type_expr_strict ------------------------------

#[test]
fn oracle_value_decodes_to_type_expr_strict() {
    // Valid value decodes + round-trips byte-equal.
    assert!(decode_oracle_value_strict(&oracle_value()).is_ok());

    // A value with NO `kind` discriminant is not a TypeExpr at all.
    assert!(matches!(
        decode_oracle_value_strict(&json!({ "no_kind_field": 1 })),
        Err(SnapshotDecodeError::OracleValueNotTypeExpr)
    ));

    // An unknown `kind` decodes to `Unknown { raw }` whose re-encode (`"unknown"`)
    // differs from the stored `kind`, so the round-trip catches it as lossy —
    // strict decode never warm-validates a value that does not re-encode to
    // itself.
    assert!(matches!(
        decode_oracle_value_strict(&json!({ "kind": "bogus_node" })),
        Err(SnapshotDecodeError::OracleValueLossyDecode)
    ));

    // A value that decodes but drops a member is rejected as lossy.
    let mut v = oracle_value();
    v["properties"].as_array_mut().unwrap().push(
        json!({ "memberKind": "property", "name": "ghost", "optional": false, "readonly": false }),
    );
    assert!(matches!(
        decode_oracle_value_strict(&v),
        Err(SnapshotDecodeError::OracleValueLossyDecode)
    ));
}

// -- identity_is_kind_specific_schema_bumped -------------------------------

#[test]
fn identity_is_kind_specific_schema_bumped() {
    let snap = valid_snapshot();
    let identity = &snap["identity"];

    // structured_type_expr identity decodes strictly.
    assert!(decode_identity("structured_type_expr", identity).is_ok());

    // An unknown oracle_value_kind is rejected (a future kind is a closed-tagged
    // addition that bumps the schema version, never silently accepted).
    assert!(matches!(
        decode_identity("relation_verdict", identity),
        Err(SnapshotDecodeError::UnknownValueKind(_))
    ));

    // An identity with an unknown field FAILS (deny_unknown_fields).
    let mut bad = identity.clone();
    bad.as_object_mut()
        .unwrap()
        .insert("extra_axis".to_string(), json!(1));
    assert!(matches!(
        decode_identity("structured_type_expr", &bad),
        Err(SnapshotDecodeError::Identity(_))
    ));

    // A garbage embedded enum tag is caught at decode.
    let mut bad_tag = identity.clone();
    bad_tag["query_helper_kind"] = json!("NotAHelper");
    assert!(matches!(
        decode_identity("structured_type_expr", &bad_tag),
        Err(SnapshotDecodeError::BadTag(_))
    ));

    // The known-kinds set size stays tied to the schema version: a new kind
    // MUST bump ORACLE_SCHEMA_VERSION. There is still exactly one kind; schema
    // v2 was the capture-strategy field-set change (`identity.probe_rhs_kind` +
    // `raw_capture.probe_scaffold`); schema v3 is the migration-fidelity field-set
    // change (`migration_fingerprint_version` + `migration_fingerprint`), not a
    // kind addition.
    assert_eq!(KNOWN_VALUE_KINDS.len(), 1);
    assert_eq!(ORACLE_SCHEMA_VERSION, 3);
}

// -- probe_scaffold_recorded_and_rederivable --------------------------------

/// A fully-valid DISTRIBUTIVE-IDENTITY snapshot: `identity.probe_rhs_kind` is
/// `"distributive_identity"`, `raw_capture` records the versioned helper decl
/// as `probe_scaffold`, and the probe header carries the WRAPPED RHS.
fn valid_dist_snapshot() -> Value {
    let mut id = identity();
    id.probe_rhs_kind = ProbeRhsKind::DistributiveIdentity;
    let env = pinned_env();
    let snapshot_id = derive_snapshot_id(&id, &env);
    let scaffold = distributive_identity_scaffold(0, SYMBOL);

    let mut snap = valid_snapshot();
    snap["snapshot_id"] = json!(snapshot_id);
    snap["identity"]["probe_rhs_kind"] = json!("distributive_identity");
    snap["raw_capture"]["probe_scaffold"] = json!(scaffold.helper_decl);
    snap["raw_capture"]["probe_header"] =
        json!(format!("type __oracle_probe__0 = {};", scaffold.rhs));
    snap
}

#[test]
fn probe_scaffold_recorded_and_rederivable() {
    // (a) A BARE snapshot records `probe_scaffold: null` and decodes strictly.
    let bare = decode_strict(&valid_snapshot()).expect("bare snapshot decodes");
    assert_eq!(
        bare.raw_capture.probe_scaffold, None,
        "a bare capture records no scaffold"
    );

    // (b) A DISTRIBUTIVE-IDENTITY snapshot records the helper decl, decodes
    //     strictly, and the stored scaffold is RE-DERIVABLE from version + spec
    //     (a pure function of the query ordinal — the offline audit needs no
    //     tsgo and no stored secret).
    let dist = valid_dist_snapshot();
    let decoded = decode_strict(&dist).expect("distributive-identity snapshot decodes");
    let expected = distributive_identity_scaffold(0, SYMBOL);
    assert_eq!(
        decoded.raw_capture.probe_scaffold.as_deref(),
        Some(expected.helper_decl.as_str()),
        "the recorded scaffold equals the versioned synthesis"
    );

    // (c) Cross-field strictness: a `distributive_identity` snapshot MISSING
    //     its scaffold fails decode…
    let mut missing = valid_dist_snapshot();
    missing["raw_capture"]["probe_scaffold"] = json!(null);
    assert!(
        matches!(
            decode_strict(&missing),
            Err(SnapshotDecodeError::ScaffoldInconsistent(_))
        ),
        "a distributive_identity snapshot without a recorded scaffold must FAIL"
    );

    //     …a BARE snapshot CARRYING a scaffold fails decode…
    let mut stray = valid_snapshot();
    stray["raw_capture"]["probe_scaffold"] =
        json!("type __oracle_probe_dist__0<T> = T extends never ? never : T;");
    assert!(
        matches!(
            decode_strict(&stray),
            Err(SnapshotDecodeError::ScaffoldInconsistent(_))
        ),
        "a bare snapshot carrying a stray scaffold must FAIL"
    );

    //     …and a TAMPERED scaffold (not the versioned synthesis for the
    //     ordinal) fails the re-derivation check.
    let mut tampered = valid_dist_snapshot();
    tampered["raw_capture"]["probe_scaffold"] =
        json!("type __oracle_probe_dist__9<T> = T extends never ? never : T;");
    assert!(
        matches!(
            decode_strict(&tampered),
            Err(SnapshotDecodeError::ScaffoldInconsistent(_))
        ),
        "a scaffold that is not the versioned synthesis for the ordinal must FAIL"
    );

    //     The WRAPPED probe header is re-checked too: a dist snapshot whose
    //     header carries the BARE rhs is inconsistent.
    let mut bare_header = valid_dist_snapshot();
    bare_header["raw_capture"]["probe_header"] = json!("type __oracle_probe__0 = ComposedProps;");
    assert!(
        matches!(
            decode_strict(&bare_header),
            Err(SnapshotDecodeError::ScaffoldInconsistent(_))
        ),
        "a distributive_identity snapshot with a bare probe header must FAIL"
    );
}

// -- snapshot_id_redrives_from_identity (snapshot-backed form) -------------

#[test]
fn snapshot_id_redrives_from_identity() {
    let snap = valid_snapshot();
    let decoded = decode_strict(&snap).expect("valid");

    // Redrive from the STORED identity equals the stored snapshot_id.
    let redrived = redrive_snapshot_id(&decoded).expect("redrive");
    assert_eq!(
        redrived, decoded.snapshot_id,
        "redrive from identity must equal the stored snapshot_id"
    );

    // Discriminating: a snapshot whose identity symbol was tampered (without
    // re-deriving the stored id) redrives to a DIFFERENT id — proving the redrive
    // depends on the identity, not just echoes the stored field.
    let mut tampered = snap.clone();
    tampered["identity"]["symbol_or_expression"] = json!("DifferentSymbol");
    let decoded_tampered = decode_strict(&tampered).expect("still strict-decodes");
    let redrived_tampered = redrive_snapshot_id(&decoded_tampered).expect("redrive");
    assert_ne!(
        redrived_tampered, decoded_tampered.snapshot_id,
        "a tampered identity must redrive to a different id than the stale stored one"
    );
}

// -- snapshot_env_pin_matches_workspace ------------------------------------

#[test]
fn snapshot_env_pin_matches_workspace() {
    let decoded = decode_strict(&valid_snapshot()).expect("valid");
    assert_eq!(decoded.tsgo_version, TSGO_VERSION);
    assert_eq!(decoded.oracle_schema_version, ORACLE_SCHEMA_VERSION);
    assert_eq!(decoded.normalizer_version, NORMALIZER_VERSION);
    assert_eq!(decoded.probe_synthesis_version, PROBE_SYNTHESIS_VERSION);
}
