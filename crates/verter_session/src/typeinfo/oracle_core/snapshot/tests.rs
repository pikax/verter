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
        properties: vec![ObjectMember::Property(
            ObjectProperty::synthetic_public_key(
                "id".to_string().into(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            ),
        )],
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
        decode_identity("bogus_value_kind", identity),
        Err(SnapshotDecodeError::UnknownValueKind(_))
    ));

    // CROSS-KIND rejection: `relation_verdict` is a KNOWN kind (v4), but a
    // v3-shaped identity under it fails — the v4 identity is a DISTINCT closed
    // shape whose required axes the v3 document does not carry (and vice
    // versa: v4-only axes on a v3 kind are unknown fields).
    assert!(matches!(
        decode_identity("relation_verdict", identity),
        Err(SnapshotDecodeError::Identity(_))
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
    // MUST bump ORACLE_SCHEMA_VERSION. Schema v2 was the capture-strategy
    // field-set change (`identity.probe_rhs_kind` + `raw_capture.probe_scaffold`);
    // schema v3 the migration-fidelity field-set change
    // (`migration_fingerprint_version` + `migration_fingerprint`); schema v4 is
    // the SECOND kind addition (`relation_verdict` — the migration mirror
    // becomes kind-keyed: required for structured_type_expr, forbidden on
    // relation_verdict).
    assert_eq!(KNOWN_VALUE_KINDS.len(), 2);
    assert_eq!(ORACLE_SCHEMA_VERSION, 4);
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

// -- relation_value_strict_rejects_a_graph_node_integer_bound ---------

/// A fully-valid synthetic `relation_verdict` snapshot: `{ value: number }`
/// against `{ value: infer V }`, capturing `assignable` with `V = number`.
/// Built through the REAL synthesis + codec paths (the probe header via the
/// versioned synthesis, the operands via the canonical derivation, the hover
/// carrying the reduced tuple wire the strict rail re-decodes).
fn valid_relation_snapshot() -> Value {
    use super::super::identity::{
        FreshnessTag, InferenceModeTag, RelationKindTag, RelationPolicyRecord,
        RelationVerdictIdentity,
    };
    use super::super::query_specs::{HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec};
    use super::super::relation_probe::{self, RelationVerdict, RELATION_BINDING_PROJECTION};

    let spec = RelationQuerySpec {
        row_file: "relation_verdict_oracle.rs",
        row_function: "relation_synthetic_value",
        query_ordinal: 0,
        oracle_family: "relation_verdict",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        source_text: "{ value: number }",
        target_text: "{ value: infer V }",
        binder_layout: &[RelationBinderSpec {
            ordinal: 0,
            name: "V",
            constraint: None,
        }],
        contract_rows: &["relation_synthetic_contract"],
        engine_pin: None,
    };
    let identity: RelationVerdictIdentity =
        relation_probe::relation_identity_from_spec(&spec).expect("synthetic spec derives");
    assert_eq!(identity.relation, RelationKindTag::Assignable);
    assert_eq!(identity.policy, RelationPolicyRecord::default_record());
    assert_eq!(identity.freshness, FreshnessTag::Regular);
    assert_eq!(identity.inference_mode, InferenceModeTag::TargetPattern);

    // The bound: `number` lowered + normalized under the ONE relation-binding
    // projection (the same the wire decoder applies).
    let bound = super::super::normalize::normalize(
        &super::super::admission::lower_hover_rhs("number").expect("bound lowers"),
        RELATION_BINDING_PROJECTION,
    )
    .expect("bound normalizes")
    .to_json_value();
    let oracle_value = json!({
        "verdict": RelationVerdict::Assignable.tag(),
        "bindings": [{ "ordinal": 0, "name": "V", "bound": bound }],
    });
    let probe_header = relation_probe::relation_probe_header(
        0,
        spec.source_text,
        spec.target_text,
        &identity.binder_layout,
    );
    let hover_contents = format!(
        "```typescript\ntype __oracle_probe__0 = readonly [true, readonly [readonly [0, \"V\", number]]];\n```"
    );
    let env = PinnedEnv {
        tsgo_version: TSGO_VERSION.to_string(),
        oracle_schema_version: ORACLE_SCHEMA_VERSION,
        normalizer_version: NORMALIZER_VERSION,
        probe_synthesis_version: PROBE_SYNTHESIS_VERSION,
        compiler_options_hash: "sha256:deadbeef".to_string(),
        env_corpus_id: "blake3:cafef00d".to_string(),
    };
    let probe = ProbeLocator {
        probe_name: "__oracle_probe__0".to_string(),
        offset: relation_probe::relation_probe_source(
            spec.row_function,
            0,
            spec.source_text,
            spec.target_text,
            &identity.binder_layout,
        )
        .find("__oracle_probe__0")
        .expect("probe name in synthesized source") as u64,
    };
    super::assemble_relation_snapshot_document(
        "relation_verdict",
        &identity,
        &env,
        &oracle_value,
        &probe,
        &json!({
            "probe_name": "__oracle_probe__0",
            "probe_header": probe_header,
            "probe_scaffold": null,
            "hover_contents": hover_contents,
        }),
        &json!({ "manifest": [], "files": [] }),
        "blake3:placeholder",
    )
}

#[test]
fn relation_value_strict_accepts_valid_and_rejects_integer_bound() {
    // The valid relation snapshot strictly decodes AND materializes.
    let valid = valid_relation_snapshot();
    let decoded = decode_strict(&valid).unwrap_or_else(|e| {
        panic!("a valid relation_verdict snapshot must strictly decode: {e:?}")
    });
    let value = super::materialize_relation_value(&decoded).expect("materializes");
    assert_eq!(
        value.verdict,
        super::super::relation_probe::RelationVerdict::Assignable
    );
    assert_eq!(value.bindings.len(), 1);
    assert_eq!(value.bindings[0].ordinal, 0);
    assert_eq!(value.bindings[0].name, "V");
    assert!(matches!(
        value.bindings[0].bound,
        TypeExpr::Primitive(PrimitiveName::Number)
    ));

    // A bare graph-node INTEGER where the normalized TypeExpr JSON
    // belongs FAILS the strict value rail — the persisted record never carries
    // a SemanticNodeId. Asserted DIRECTLY against `decode_relation_value_strict`
    // (the end-to-end `decode_strict` path would reject EARLIER, at the
    // raw-capture re-derivation rail, and so could never prove THIS rail is the
    // rejector): the strict value decode of the integer-bound value against
    // the valid identity's binder layout must fail.
    let valid = valid_relation_snapshot();
    let identity_dto = match super::decode_identity("relation_verdict", &valid["identity"])
        .expect("identity decodes")
    {
        super::DecodedIdentity::RelationVerdict(dto) => dto,
        super::DecodedIdentity::StructuredTypeExpr(_) => panic!("wrong kind"),
    };
    let mut int_bound_value = valid["oracle_value"].clone();
    int_bound_value["bindings"][0]["bound"] = json!(7);
    assert!(
        matches!(
            super::decode_relation_value_strict(&int_bound_value, &identity_dto),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a graph-node integer `bound` must fail the strict VALUE rail itself \
         (red if `type_expr_from_json` ever accepts a bare integer)"
    );

    // End-to-end, the same tamper ALSO fails `decode_strict` (here the
    // raw-capture re-derivation rail is the first rejector — kept as coverage
    // of that rail, not as the value-rail proof).
    let mut bad = valid_relation_snapshot();
    bad["oracle_value"]["bindings"][0]["bound"] = json!(7);
    assert!(
        matches!(
            decode_strict(&bad),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a graph-node integer `bound` must fail strict decode"
    );

    // A binding whose ordinal/name does not match the identity binder layout
    // fails (ordinal AND name must match in preorder).
    let mut wrong_name = valid_relation_snapshot();
    wrong_name["oracle_value"]["bindings"][0]["name"] = json!("Z");
    assert!(matches!(
        decode_strict(&wrong_name),
        Err(SnapshotDecodeError::RelationValue(_))
    ));

    // A false verdict carrying bindings fails.
    let mut false_with_bindings = valid_relation_snapshot();
    false_with_bindings["oracle_value"]["verdict"] = json!("not_assignable");
    assert!(matches!(
        decode_strict(&false_with_bindings),
        Err(SnapshotDecodeError::RelationValue(_))
    ));

    // A cross-kind field (the v3 migration mirror / source-admission digest on
    // a relation snapshot) fails.
    let mut cross = valid_relation_snapshot();
    cross["migration_fingerprint"] = json!("blake3:00");
    cross["migration_fingerprint_version"] = json!(1);
    assert!(matches!(
        decode_strict(&cross),
        Err(SnapshotDecodeError::CrossKindField(_))
    ));
    let mut cross2 = valid_relation_snapshot();
    cross2["source_admission_digest"] = json!({
        "source_locator": { "reference_canonical": "/x.ts", "reference_name": "X", "symbol_space": "Type" },
        "observed_source_files": [],
        "contributors": [],
        "final_verdict": "Admit"
    });
    assert!(matches!(
        decode_strict(&cross2),
        Err(SnapshotDecodeError::CrossKindField(_))
    ));

    // An unknown envelope field fails (the closed envelope).
    let mut unknown = valid_relation_snapshot();
    unknown["surprise"] = json!(true);
    assert!(matches!(
        decode_strict(&unknown),
        Err(SnapshotDecodeError::Envelope(_))
    ));

    // A hand-edited oracle_value (recorded hover left intact) fails the v4
    // raw-capture rail: the hover re-decodes to the ORIGINAL value.
    let mut edited = valid_relation_snapshot();
    edited["oracle_value"]["bindings"][0]["bound"] =
        json!({ "kind": "primitive", "name": "string" });
    assert!(
        matches!(
            decode_strict(&edited),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a hand-edited bound must fail the raw-capture re-derivation rail"
    );
}

// -- failed-infer rows are representable (F3) ----------------------------------

/// A synthetic FAILED-infer registry+snapshot case: `[string] extends
/// [{value: infer V}]` — `not_assignable` with a NON-EMPTY binder layout
/// (`[{0, "V"}]`) and ZERO matched bindings. The wire (`readonly [false,
/// readonly []]`), the identity (inference_mode `target_pattern`, layout
/// `[V]`), and the strict value rail all support this shape; pre-F3 the
/// value rail's unconditional layout-length check rejected it.
#[test]
fn failed_infer_row_with_nonempty_layout_decodes_and_round_trips() {
    use super::super::identity::RelationVerdictIdentity;
    use super::super::query_specs::{HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec};
    use super::super::relation_probe::{self, RelationVerdict};

    // The registry-shaped case derives its v4 identity (inference mode
    // `target_pattern`, binder layout `[{0, "V"}]`).
    let spec = RelationQuerySpec {
        row_file: "relation_verdict_oracle.rs",
        row_function: "relation_failed_infer_synthetic",
        query_ordinal: 0,
        oracle_family: "relation_verdict",
        host_project: super::super::query_specs::HostProjectSpec {
            project_root: "/",
            workspace_root: "/",
            tsconfig_path: "/oracle.tsconfig.json",
            host_setup_kind: HostSetupKindSpec::Standalone,
        },
        source_text: "[string]",
        target_text: "{ value: infer V }",
        binder_layout: &[RelationBinderSpec {
            ordinal: 0,
            name: "V",
            constraint: None,
        }],
        contract_rows: &["relation_failed_infer_synthetic_contract"],
        engine_pin: None,
    };
    let identity: RelationVerdictIdentity =
        relation_probe::relation_identity_from_spec(&spec).expect("failed-infer spec derives");
    assert_eq!(identity.binder_layout.len(), 1);

    // The captured snapshot: not_assignable, ZERO bindings, non-empty layout.
    let oracle_value = json!({
        "verdict": RelationVerdict::NotAssignable.tag(),
        "bindings": [],
    });
    let probe_header = relation_probe::relation_probe_header(
        0,
        spec.source_text,
        spec.target_text,
        &identity.binder_layout,
    );
    let env = PinnedEnv {
        tsgo_version: TSGO_VERSION.to_string(),
        oracle_schema_version: ORACLE_SCHEMA_VERSION,
        normalizer_version: NORMALIZER_VERSION,
        probe_synthesis_version: PROBE_SYNTHESIS_VERSION,
        compiler_options_hash: "sha256:deadbeef".to_string(),
        env_corpus_id: "blake3:cafef00d".to_string(),
    };
    let doc = super::assemble_relation_snapshot_document(
        "relation_verdict",
        &identity,
        &env,
        &oracle_value,
        &ProbeLocator {
            probe_name: "__oracle_probe__0".to_string(),
            offset: relation_probe::relation_probe_source(
                spec.row_function,
                0,
                spec.source_text,
                spec.target_text,
                &identity.binder_layout,
            )
            .find("__oracle_probe__0")
            .expect("probe name in synthesized source") as u64,
        },
        &json!({
            "probe_name": "__oracle_probe__0",
            "probe_header": probe_header,
            "probe_scaffold": null,
            "hover_contents": "```typescript\ntype __oracle_probe__0 = readonly [false, readonly []];\n```",
        }),
        &json!({ "manifest": [], "files": [] }),
        "blake3:placeholder",
    );

    // ACCEPTED: strict decode passes (the raw-capture rail re-decodes the
    // recorded false wire to the stored value; the value rail no longer
    // demands bindings == layout for a false verdict).
    let decoded = decode_strict(&doc).unwrap_or_else(|e| {
        panic!("a failed-infer row (non-empty layout, no bindings) must decode: {e:?}")
    });
    // ROUND-TRIPS through the materializer into the normalized boundary.
    let value = super::materialize_relation_value(&decoded).expect("materializes");
    assert_eq!(value.verdict, RelationVerdict::NotAssignable);
    assert!(value.bindings.is_empty());

    // The inverse rail is still live: a FALSE verdict CARRYING a binding
    // rejects (the failed-infer relaxation never admits false-with-bindings).
    let mut bad = doc.clone();
    bad["oracle_value"]["bindings"] = json!([{
        "ordinal": 0,
        "name": "V",
        "bound": { "kind": "primitive", "name": "string" },
    }]);
    assert!(matches!(
        decode_strict(&bad),
        Err(SnapshotDecodeError::RelationValue(_))
    ));
}

// -- constrained-infer rows: no aliasing, schema validity ---------------

/// The codex failing case: a constrained-infer row (`{ value: number }`
/// against `{ value: infer V extends string }` — `not_assignable`, no
/// bindings, non-empty layout) must NOT alias the unconstrained row, and its
/// snapshot must strictly decode + round-trip.
#[test]
fn constrained_infer_row_does_not_alias_the_unconstrained_row() {
    use super::super::identity::derive_relation_snapshot_id;
    use super::super::query_specs::{HostSetupKindSpec, RelationBinderSpec, RelationQuerySpec};
    use super::super::relation_probe::{self, RelationVerdict};

    fn spec(
        source: &'static str,
        target: &'static str,
        binder_layout: &'static [RelationBinderSpec],
        row_function: &'static str,
    ) -> RelationQuerySpec {
        RelationQuerySpec {
            row_file: "relation_verdict_oracle.rs",
            row_function,
            query_ordinal: 0,
            oracle_family: "relation_verdict",
            host_project: super::super::query_specs::HostProjectSpec {
                project_root: "/",
                workspace_root: "/",
                tsconfig_path: "/oracle.tsconfig.json",
                host_setup_kind: HostSetupKindSpec::Standalone,
            },
            source_text: source,
            target_text: target,
            binder_layout,
            contract_rows: &["relation_constraint_alias_contract"],
            engine_pin: None,
        }
    }
    const CONSTRAINED_LAYOUT: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: Some("string"),
    }];
    const BARE_LAYOUT: &[RelationBinderSpec] = &[RelationBinderSpec {
        ordinal: 0,
        name: "V",
        constraint: None,
    }];

    let constrained = spec(
        "{ value: number }",
        "{ value: infer V extends string }",
        CONSTRAINED_LAYOUT,
        "relation_constrained_infer_synthetic",
    );
    let bare = spec(
        "{ value: number }",
        "{ value: infer V }",
        BARE_LAYOUT,
        "relation_constrained_infer_synthetic",
    );
    let constrained_identity =
        relation_probe::relation_identity_from_spec(&constrained).expect("constrained derives");
    let bare_identity =
        relation_probe::relation_identity_from_spec(&bare).expect("unconstrained derives");

    // NO ALIASING: distinct canonical identities AND distinct snapshot ids.
    assert_ne!(constrained_identity, bare_identity);
    let env = PinnedEnv {
        tsgo_version: TSGO_VERSION.to_string(),
        oracle_schema_version: ORACLE_SCHEMA_VERSION,
        normalizer_version: NORMALIZER_VERSION,
        probe_synthesis_version: PROBE_SYNTHESIS_VERSION,
        compiler_options_hash: "sha256:deadbeef".to_string(),
        env_corpus_id: "blake3:cafef00d".to_string(),
    };
    assert_ne!(
        derive_relation_snapshot_id(&constrained_identity, &env),
        derive_relation_snapshot_id(&bare_identity, &env),
        "the constrained row must not alias the unconstrained row's identity"
    );

    // The constrained not_assignable snapshot strictly decodes + round-trips.
    let probe_header = relation_probe::relation_probe_header(
        0,
        constrained.source_text,
        constrained.target_text,
        &constrained_identity.binder_layout,
    );
    let doc = super::assemble_relation_snapshot_document(
        "relation_verdict",
        &constrained_identity,
        &env,
        &json!({ "verdict": RelationVerdict::NotAssignable.tag(), "bindings": [] }),
        &ProbeLocator {
            probe_name: "__oracle_probe__0".to_string(),
            offset: relation_probe::relation_probe_source(
                constrained.row_function,
                0,
                constrained.source_text,
                constrained.target_text,
                &constrained_identity.binder_layout,
            )
            .find("__oracle_probe__0")
            .expect("probe name in synthesized source") as u64,
        },
        &json!({
            "probe_name": "__oracle_probe__0",
            "probe_header": probe_header,
            "probe_scaffold": null,
            "hover_contents": "```typescript\ntype __oracle_probe__0 = readonly [false, readonly []];\n```",
        }),
        &json!({ "manifest": [], "files": [] }),
        "blake3:placeholder",
    );
    let decoded = decode_strict(&doc)
        .unwrap_or_else(|e| panic!("the constrained failed-infer snapshot must decode: {e:?}"));
    let value = super::materialize_relation_value(&decoded).expect("materializes");
    assert_eq!(value.verdict, RelationVerdict::NotAssignable);
    assert!(value.bindings.is_empty());
    // The stored layout carries the canonical constraint.
    assert_eq!(
        decoded.identity["binder_layout"][0]["constraint"],
        json!({ "kind": "primitive", "name": "string" }),
        "the stored binder layout entry carries the canonical constraint"
    );

    // A bogus (non-TypeExpr) constraint in the stored layout REJECTS.
    let mut bad = doc.clone();
    bad["identity"]["binder_layout"][0]["constraint"] = json!(7);
    assert!(
        matches!(decode_strict(&bad), Err(SnapshotDecodeError::Identity(_))),
        "a graph-node integer constraint must fail strict decode"
    );

    // A constraint field with an unknown sibling key in a layout entry rejects
    // (the layout DTO stays closed).
    let mut bad2 = doc.clone();
    bad2["identity"]["binder_layout"][0]["surprise"] = json!(true);
    assert!(matches!(
        decode_strict(&bad2),
        Err(SnapshotDecodeError::Identity(_))
    ));
}

// -- raw_capture probe identity is bound to the locator + content -------

#[test]
fn raw_capture_probe_identity_is_bound_to_locator_and_content() {
    let valid = valid_relation_snapshot();
    decode_strict(&valid).expect("the valid relation snapshot decodes");

    // (i) Rename EVERY probe `__oracle_probe__0` → `__oracle_probe__9`
    // consistently — raw_capture.probe_name, the header, the hover, AND
    // identity.probe_locator.probe_name. Previously the raw_capture name was its
    // own anchor and this passed.
    let mut renamed = valid.clone();
    let rename = |text: &str| text.replace("__oracle_probe__0", "__oracle_probe__9");
    renamed["raw_capture"]["probe_name"] = json!("__oracle_probe__9");
    renamed["raw_capture"]["probe_header"] = json!(rename(
        valid["raw_capture"]["probe_header"].as_str().unwrap()
    ));
    renamed["raw_capture"]["hover_contents"] = json!(rename(
        valid["raw_capture"]["hover_contents"].as_str().unwrap()
    ));
    renamed["identity"]["probe_locator"]["probe_name"] = json!("__oracle_probe__9");
    assert!(
        matches!(
            decode_strict(&renamed),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a consistent all-probe rename must fail the bound probe-identity rails"
    );

    // (ii) Corrupt identity.probe_locator.offset (previously unvalidated).
    let mut bad_offset = valid.clone();
    bad_offset["identity"]["probe_locator"]["offset"] = json!(
        valid["identity"]["probe_locator"]["offset"]
            .as_u64()
            .unwrap()
            + 1
    );
    assert!(
        matches!(
            decode_strict(&bad_offset),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a corrupted probe_locator offset must fail the offset binding"
    );

    // (iii) Corrupt identity.workspace_files[0].content_hash (the probe-file
    // content binding is recomputed from the versioned synthesis).
    let mut bad_hash = valid.clone();
    bad_hash["identity"]["workspace_files"][0]["content_hash"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    assert!(
        matches!(
            decode_strict(&bad_hash),
            Err(SnapshotDecodeError::RelationValue(_))
        ),
        "a corrupted probe-file content hash must fail the content binding"
    );

    // (iv) A renamed raw_capture.probe_name ALONE (locator untouched) rejects
    // on the locator⇄capture binding even before the header leg.
    let mut half_renamed = valid.clone();
    half_renamed["raw_capture"]["probe_name"] = json!("__oracle_probe__9");
    assert!(matches!(
        decode_strict(&half_renamed),
        Err(SnapshotDecodeError::RelationValue(_))
    ));
}
