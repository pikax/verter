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
    QueryHelperKind, SnapshotIdentity, WorkspaceFileRef, ORACLE_SCHEMA_VERSION,
    PROBE_SYNTHESIS_VERSION, TSGO_VERSION,
};
use super::super::normalize::{ProjectionModeKind, NORMALIZER_VERSION};
use super::{
    decode_identity, decode_oracle_value_strict, decode_strict, redrive_snapshot_id,
    SnapshotDecodeError, KNOWN_VALUE_KINDS,
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

    // The known-kinds set size is tied to the schema version: a new kind MUST
    // bump ORACLE_SCHEMA_VERSION. At schema v1 there is exactly one kind.
    assert_eq!(KNOWN_VALUE_KINDS.len(), 1);
    assert_eq!(ORACLE_SCHEMA_VERSION, 1);
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
